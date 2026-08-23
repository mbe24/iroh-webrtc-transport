//! Browser chat entrypoint (wasm32): a persistent, bidirectional, multi-message
//! chat over iroh's QUIC running on a browser WebRTC data channel.
//!
//! Signaling is the *page's* job (BroadcastChannel between tabs, or manual link
//! exchange cross-device) and happens entirely in JS. Once the page has an open
//! `RTCDataChannel` it calls [`start_chat`], which builds the WebRTC backend,
//! binds an iroh endpoint, opens **one** QUIC bi-stream, and then reads and
//! writes length-framed messages on it concurrently — so both peers type freely
//! like a real chat. [`Chat::send`] enqueues an outbound message; each inbound
//! message invokes the JS `on_message` callback.

use std::sync::{Arc, Mutex};

use futures::{
    channel::mpsc::{unbounded, UnboundedReceiver, UnboundedSender},
    StreamExt,
};
use iroh::{
    endpoint::{Connection, RecvStream, SendStream},
    protocol::{AcceptError, ProtocolHandler, Router},
    Endpoint, EndpointAddr, SecretKey, TransportAddr,
};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::spawn_local;
use web_sys::RtcDataChannel;

use crate::{addr_of, build_endpoint, webrtc_wasm::WebRtcBackend, PeerKey};

const ALPN: &[u8] = b"iroh-webrtc-transport/chat/0";
const MAX_MSG: usize = 1 << 20;

/// Live chat handle returned to JS. Keeps the iroh endpoint/connection alive and
/// exposes [`send`](Chat::send).
#[wasm_bindgen]
pub struct Chat {
    out: UnboundedSender<String>,
    _anchor: Anchor,
}

#[wasm_bindgen]
impl Chat {
    /// Enqueue a text message to the peer (fire-and-forget; delivered in order).
    pub fn send(&self, text: String) {
        let _ = self.out.unbounded_send(text);
    }
}

/// Keeps the transport alive for the page lifetime.
enum Anchor {
    Initiator {
        _endpoint: Endpoint,
        _conn: Connection,
    },
    Responder {
        _router: Router,
    },
}

/// Start a chat over an already-open data channel.
///
/// * `dc` — the open `RTCDataChannel` (the page did the signaling).
/// * `my_seed` / `remote_seed` — deterministic identity seeds, swapped between
///   the two sides, so each knows both public keys without discovery.
/// * `is_initiator` — the offerer side dials; the answerer side accepts.
/// * `on_message` — JS callback invoked with each inbound message (one string arg).
#[wasm_bindgen]
pub async fn start_chat(
    dc: RtcDataChannel,
    my_seed: u8,
    remote_seed: u8,
    is_initiator: bool,
    on_message: js_sys::Function,
) -> Result<Chat, JsValue> {
    let my_sk = SecretKey::from_bytes(&[my_seed; 32]);
    let remote_pub = SecretKey::from_bytes(&[remote_seed; 32]).public();
    let remote_key = *remote_pub.as_bytes();

    // Bridge the data channel's inbound datagrams into the backend.
    let (dg_tx, dg_rx) = crate::webrtc_wasm::inbound_channel();
    crate::webrtc_wasm::wire_inbound(&dc, remote_key, dg_tx).forget();

    let channels = std::rc::Rc::new(std::cell::RefCell::new(
        std::collections::HashMap::<PeerKey, RtcDataChannel>::new(),
    ));
    channels.borrow_mut().insert(remote_key, dc);

    let backend = WebRtcBackend::new(*my_sk.public().as_bytes(), dg_rx, channels);
    let endpoint = build_endpoint(my_sk, backend).bind().await.map_err(to_js)?;

    // Chat message plumbing: `in_*` carries inbound messages to the JS callback;
    // `out_*` carries outbound messages from `Chat::send` to the writer. Both
    // carry `String` (Send), so the responder's ProtocolHandler stays Send+Sync.
    let (in_tx, mut in_rx) = unbounded::<String>();
    let (out_tx, out_rx) = unbounded::<String>();

    let anchor = if is_initiator {
        let addr =
            EndpointAddr::from_parts(remote_pub, [TransportAddr::Custom(addr_of(&remote_key))]);
        let conn = endpoint.connect(addr, ALPN).await.map_err(to_js)?;
        let (mut send, recv) = conn.open_bi().await.map_err(to_js)?;
        // Prime the stream so the responder's accept_bi resolves before either
        // user has typed (QUIC opens a bi-stream lazily on first write).
        send.write_all(&0u32.to_be_bytes()).await.map_err(to_js)?;
        spawn_local(read_loop(recv, in_tx));
        spawn_local(write_loop(send, out_rx));
        Anchor::Initiator {
            _endpoint: endpoint.clone(),
            _conn: conn,
        }
    } else {
        let handler = ChatProto {
            inbound: in_tx,
            outbound: Arc::new(Mutex::new(Some(out_rx))),
        };
        let router = Router::builder(endpoint).accept(ALPN, handler).spawn();
        Anchor::Responder { _router: router }
    };

    // Deliver inbound messages to JS on the main task (the JS Function need not
    // be Send, so it lives here rather than inside the transport tasks).
    spawn_local(async move {
        while let Some(msg) = in_rx.next().await {
            let _ = on_message.call1(&JsValue::NULL, &JsValue::from_str(&msg));
        }
    });

    Ok(Chat {
        out: out_tx,
        _anchor: anchor,
    })
}

/// Responder protocol: on the first (only) connection, take the bi-stream and
/// drive the same read/write loops as the initiator.
#[derive(Clone)]
struct ChatProto {
    inbound: UnboundedSender<String>,
    outbound: Arc<Mutex<Option<UnboundedReceiver<String>>>>,
}

impl std::fmt::Debug for ChatProto {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChatProto").finish_non_exhaustive()
    }
}

impl ProtocolHandler for ChatProto {
    async fn accept(&self, connection: Connection) -> Result<(), AcceptError> {
        let (send, recv) = connection.accept_bi().await?;
        spawn_local(read_loop(recv, self.inbound.clone()));
        if let Some(out_rx) = self.outbound.lock().unwrap().take() {
            spawn_local(write_loop(send, out_rx));
        }
        connection.closed().await;
        Ok(())
    }
}

/// Read length-framed messages off the QUIC stream into `tx`. A zero-length
/// frame is the priming frame — skipped.
async fn read_loop(mut recv: RecvStream, tx: UnboundedSender<String>) {
    loop {
        let mut len = [0u8; 4];
        if recv.read_exact(&mut len).await.is_err() {
            break;
        }
        let n = u32::from_be_bytes(len) as usize;
        if n == 0 {
            continue;
        }
        if n > MAX_MSG {
            break;
        }
        let mut buf = vec![0u8; n];
        if recv.read_exact(&mut buf).await.is_err() {
            break;
        }
        if tx
            .unbounded_send(String::from_utf8_lossy(&buf).into_owned())
            .is_err()
        {
            break;
        }
    }
}

/// Write queued messages to the QUIC stream, length-framed.
async fn write_loop(mut send: SendStream, mut rx: UnboundedReceiver<String>) {
    while let Some(msg) = rx.next().await {
        let bytes = msg.as_bytes();
        if send
            .write_all(&(bytes.len() as u32).to_be_bytes())
            .await
            .is_err()
        {
            break;
        }
        if send.write_all(bytes).await.is_err() {
            break;
        }
    }
    let _ = send.finish();
}

fn to_js<E: std::fmt::Display>(e: E) -> JsValue {
    JsValue::from_str(&e.to_string())
}
