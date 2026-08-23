//! Browser WebRTC datagram backend (Milestone 2).
//!
//! The same [`DatagramBackend`](crate::datagram::DatagramBackend) as the
//! in-memory and native backends, but the carrier is a browser
//! `RTCDataChannel` reached through `web-sys`. iroh (compiled to wasm) runs its
//! QUIC over that channel; the channel is opened by WebRTC signaling *before*
//! iroh starts, so iroh only ever moves bytes — it is not the signaler.
//!
//! ## Send/Sync
//!
//! iroh's custom-transport traits are unconditionally `Send + Sync`, but
//! `web-sys` handles (`RtcDataChannel`, closures) are `!Send`. wasm runs on a
//! single thread, so wrapping the JS handles in [`send_wrapper::SendWrapper`]
//! is sound: it hands out the value only on the thread that created it and
//! panics otherwise — which, on wasm, can never happen.
//!
//! ## Verification status
//!
//! This module is **compile-verified against iroh 1.0.3's real traits on
//! `wasm32-unknown-unknown`**. The end-to-end QUIC-over-`RTCDataChannel`
//! handshake can only be exercised in a real browser (see the `webrtc-iroh-wasm`
//! harness); the datagram/channel semantics are identical to the native WebRTC
//! backend (Milestone 1b), which *is* run in Docker.

use std::{
    cell::RefCell,
    collections::HashMap,
    rc::Rc,
    task::{Context, Poll},
};

use bytes::Bytes;
use futures::{
    channel::mpsc::{unbounded, UnboundedReceiver, UnboundedSender},
    stream::StreamExt,
};
use send_wrapper::SendWrapper;
use wasm_bindgen::{prelude::Closure, JsCast};
use web_sys::{MessageEvent, RtcDataChannel};

use crate::datagram::{DatagramBackend, DatagramReceiver, DatagramSender, Inbound, PeerKey};

/// Map of peer key -> its open data channel. `Rc<RefCell<..>>` because it is
/// shared between the sender and the signaling closures on one thread.
type Channels = Rc<RefCell<HashMap<PeerKey, RtcDataChannel>>>;

/// Backend built from already-open data channels. Construct via [`WebRtcBackend::new`]
/// after signaling completes, then hand to `BackendTransport::new`.
pub struct WebRtcBackend {
    local: PeerKey,
    // Bundled so the whole non-Send payload crosses the trait boundary behind a
    // single wrapper. Taken apart in `split`.
    inner: SendWrapper<(UnboundedReceiver<Inbound>, Channels)>,
}

impl WebRtcBackend {
    /// `local` is this endpoint's public key; `rx` receives datagrams that the
    /// data-channel `onmessage` closures push in; `channels` maps each peer to
    /// its open channel for sending.
    pub fn new(local: PeerKey, rx: UnboundedReceiver<Inbound>, channels: Channels) -> Self {
        Self {
            local,
            inner: SendWrapper::new((rx, channels)),
        }
    }
}

impl DatagramBackend for WebRtcBackend {
    type Receiver = WebRtcReceiver;
    type Sender = WebRtcSender;

    fn local_key(&self) -> PeerKey {
        self.local
    }

    fn split(self) -> (WebRtcReceiver, WebRtcSender) {
        let local = self.local;
        let (rx, channels) = self.inner.take();
        (
            WebRtcReceiver {
                rx: SendWrapper::new(rx),
            },
            WebRtcSender {
                channels: SendWrapper::new(channels),
                _local: local,
            },
        )
    }
}

/// Receive half: drains datagrams delivered by data-channel `onmessage`.
pub struct WebRtcReceiver {
    rx: SendWrapper<UnboundedReceiver<Inbound>>,
}

impl std::fmt::Debug for WebRtcReceiver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WebRtcReceiver").finish_non_exhaustive()
    }
}

impl DatagramReceiver for WebRtcReceiver {
    fn poll_recv(&mut self, cx: &mut Context<'_>) -> Poll<Option<Inbound>> {
        (*self.rx).poll_next_unpin(cx)
    }
}

/// Send half: looks up the destination's data channel and writes the datagram.
#[derive(Clone)]
pub struct WebRtcSender {
    channels: SendWrapper<Channels>,
    _local: PeerKey,
}

impl std::fmt::Debug for WebRtcSender {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WebRtcSender").finish_non_exhaustive()
    }
}

impl DatagramSender for WebRtcSender {
    fn send(&self, dst: &PeerKey, data: &[u8]) {
        if let Some(dc) = self.channels.borrow().get(dst) {
            // Best-effort: drop on error/backpressure, QUIC retransmits.
            let _ = dc.send_with_u8_array(data);
        }
    }
}

/// Wire a data channel's inbound path into the backend: set binary type to
/// ArrayBuffer and route `onmessage` into `tx`, tagged with the peer `from`.
///
/// The returned [`Closure`] must be kept alive for as long as the channel; drop
/// it and messages stop arriving. (Callers that run for the page lifetime may
/// `.forget()` it.)
#[must_use]
pub fn wire_inbound(
    dc: &RtcDataChannel,
    from: PeerKey,
    tx: UnboundedSender<Inbound>,
) -> Closure<dyn FnMut(MessageEvent)> {
    dc.set_binary_type(web_sys::RtcDataChannelType::Arraybuffer);
    let on_msg = Closure::wrap(Box::new(move |e: MessageEvent| {
        let data = e.data();
        // Payload is an ArrayBuffer (binary_type = Arraybuffer); tolerate a
        // Uint8Array too, matching the reference facade.
        let bytes = if let Some(buf) = data.dyn_ref::<js_sys::ArrayBuffer>() {
            js_sys::Uint8Array::new(buf).to_vec()
        } else if let Some(arr) = data.dyn_ref::<js_sys::Uint8Array>() {
            arr.to_vec()
        } else {
            return;
        };
        let _ = tx.unbounded_send(Inbound {
            data: Bytes::from(bytes),
            from,
        });
    }) as Box<dyn FnMut(MessageEvent)>);
    dc.set_onmessage(Some(on_msg.as_ref().unchecked_ref()));
    on_msg
}

/// Create the inbound channel pair used by [`wire_inbound`] and [`WebRtcBackend::new`].
pub fn inbound_channel() -> (UnboundedSender<Inbound>, UnboundedReceiver<Inbound>) {
    unbounded()
}
