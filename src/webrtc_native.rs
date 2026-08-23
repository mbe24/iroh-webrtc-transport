//! Native WebRTC datagram backend (Milestone 1b) via webrtc-rs 0.17.
//!
//! Proves the concept the browser milestone depends on: iroh's QUIC running
//! over a **real WebRTC data channel** (DTLS + SCTP, unreliable/unordered — the
//! UDP-like shape QUIC wants), not an in-memory shortcut. Two peer connections
//! complete a full offer/answer + ICE handshake over loopback; iroh then binds
//! and exchanges a Loro document across the resulting channels.
//!
//! Signaling happens *before* iroh starts (here: direct in-process SDP/ICE
//! exchange), so iroh only ever moves bytes — exactly the browser model, minus
//! the WebSocket signaling and web-sys bindings.

use std::sync::Arc;

use anyhow::{anyhow, Result};
use bytes::Bytes;
use tokio::sync::{mpsc, oneshot};

use webrtc::api::interceptor_registry::register_default_interceptors;
use webrtc::api::media_engine::MediaEngine;
use webrtc::api::setting_engine::SettingEngine;
use webrtc::api::{APIBuilder, API};
use webrtc::data_channel::data_channel_init::RTCDataChannelInit;
use webrtc::data_channel::data_channel_message::DataChannelMessage;
use webrtc::data_channel::RTCDataChannel;
use webrtc::ice_transport::ice_candidate::{RTCIceCandidate, RTCIceCandidateInit};
use webrtc::interceptor::registry::Registry;
use webrtc::peer_connection::configuration::RTCConfiguration;
use webrtc::peer_connection::RTCPeerConnection;

use std::task::{Context, Poll};

use crate::datagram::{DatagramBackend, DatagramReceiver, DatagramSender, Inbound, PeerKey};

// ---------------------------------------------------------------------------
// Loopback data-channel primitive (webrtc-rs 0.17.2 classic callback API).
// ---------------------------------------------------------------------------

/// One side of a connected pair: the peer connection (kept alive — dropping it
/// tears down the transport), its data channel, and a receiver bridged from the
/// callback-based `on_message`.
struct DcEndpoint {
    pc: Arc<RTCPeerConnection>,
    dc: Arc<RTCDataChannel>,
    incoming: mpsc::UnboundedReceiver<Bytes>,
}

/// A single API mints many peer connections; both loopback peers share one.
fn new_api() -> Result<API> {
    let mut media = MediaEngine::default();
    media.register_default_codecs()?;

    let mut registry = Registry::new();
    registry = register_default_interceptors(registry, &mut media)?;

    // Empty ice_servers => host candidates only (loopback needs no STUN).
    // include_loopback_candidate keeps this working in containers/CI whose only
    // interface is 127.0.0.1 (otherwise the ICE agent discards loopback).
    let mut setting = SettingEngine::default();
    setting.set_include_loopback_candidate(true);

    Ok(APIBuilder::new()
        .with_media_engine(media)
        .with_interceptor_registry(registry)
        .with_setting_engine(setting)
        .build())
}

fn wire_recv(dc: &Arc<RTCDataChannel>, tx: mpsc::UnboundedSender<Bytes>) {
    dc.on_message(Box::new(move |msg: DataChannelMessage| {
        let tx = tx.clone();
        Box::pin(async move {
            let _ = tx.send(msg.data);
        })
    }));
}

fn wire_open(dc: &Arc<RTCDataChannel>) -> oneshot::Receiver<()> {
    let (open_tx, open_rx) = oneshot::channel::<()>();
    dc.on_open(Box::new(move || {
        Box::pin(async move {
            let _ = open_tx.send(());
        })
    }));
    open_rx
}

fn wire_ice(pc: &Arc<RTCPeerConnection>, tx: mpsc::UnboundedSender<RTCIceCandidateInit>) {
    pc.on_ice_candidate(Box::new(move |c: Option<RTCIceCandidate>| {
        let tx = tx.clone();
        Box::pin(async move {
            if let Some(c) = c {
                if let Ok(init) = c.to_json() {
                    let _ = tx.send(init);
                }
            }
        })
    }));
}

/// Establish a connected pair of unreliable/unordered data channels over
/// in-process loopback (offerer, answerer).
async fn connect_pair() -> Result<(DcEndpoint, DcEndpoint)> {
    let api = new_api()?;
    let offerer = Arc::new(api.new_peer_connection(RTCConfiguration::default()).await?);
    let answerer = Arc::new(api.new_peer_connection(RTCConfiguration::default()).await?);

    let (off_cand_tx, mut off_cand_rx) = mpsc::unbounded_channel::<RTCIceCandidateInit>();
    let (ans_cand_tx, mut ans_cand_rx) = mpsc::unbounded_channel::<RTCIceCandidateInit>();
    wire_ice(&offerer, off_cand_tx);
    wire_ice(&answerer, ans_cand_tx);

    // Unreliable + unordered = UDP-datagram-like, which is what QUIC wants.
    let off_dc = offerer
        .create_data_channel(
            "iroh",
            Some(RTCDataChannelInit {
                ordered: Some(false),
                max_retransmits: Some(0),
                ..Default::default()
            }),
        )
        .await?;
    let (off_in_tx, off_in_rx) = mpsc::unbounded_channel::<Bytes>();
    wire_recv(&off_dc, off_in_tx);
    let off_open_rx = wire_open(&off_dc);

    // The answerer's channel arrives via on_data_channel; wire it inside the callback.
    let (ans_in_tx, ans_in_rx) = mpsc::unbounded_channel::<Bytes>();
    let (ans_open_tx, ans_open_rx) = oneshot::channel::<()>();
    let (ans_dc_tx, ans_dc_rx) = oneshot::channel::<Arc<RTCDataChannel>>();
    let ans_open_tx = Arc::new(std::sync::Mutex::new(Some(ans_open_tx)));
    let ans_dc_tx = Arc::new(std::sync::Mutex::new(Some(ans_dc_tx)));
    {
        let ans_in_tx = ans_in_tx.clone();
        let ans_open_tx = ans_open_tx.clone();
        let ans_dc_tx = ans_dc_tx.clone();
        answerer.on_data_channel(Box::new(move |dc: Arc<RTCDataChannel>| {
            let ans_in_tx = ans_in_tx.clone();
            let ans_open_tx = ans_open_tx.clone();
            let ans_dc_tx = ans_dc_tx.clone();
            Box::pin(async move {
                wire_recv(&dc, ans_in_tx);
                if let Some(open_tx) = ans_open_tx.lock().unwrap().take() {
                    dc.on_open(Box::new(move || {
                        Box::pin(async move {
                            let _ = open_tx.send(());
                        })
                    }));
                }
                if let Some(dc_tx) = ans_dc_tx.lock().unwrap().take() {
                    let _ = dc_tx.send(dc);
                }
            })
        }));
    }

    // Offer/answer dance. Create the DC before the offer so its m-line is in the SDP.
    let offer = offerer.create_offer(None).await?;
    offerer.set_local_description(offer.clone()).await?;
    answerer.set_remote_description(offer).await?;

    let answer = answerer.create_answer(None).await?;
    answerer.set_local_description(answer.clone()).await?;
    offerer.set_remote_description(answer).await?;

    // Only now that both remote descriptions are set is add_ice_candidate legal.
    {
        let answerer = answerer.clone();
        tokio::spawn(async move {
            while let Some(init) = off_cand_rx.recv().await {
                let _ = answerer.add_ice_candidate(init).await;
            }
        });
    }
    {
        let offerer = offerer.clone();
        tokio::spawn(async move {
            while let Some(init) = ans_cand_rx.recv().await {
                let _ = offerer.add_ice_candidate(init).await;
            }
        });
    }

    let ans_dc = ans_dc_rx
        .await
        .map_err(|_| anyhow!("answerer never produced a data channel"))?;
    off_open_rx
        .await
        .map_err(|_| anyhow!("offerer data channel open signal lost"))?;
    ans_open_rx
        .await
        .map_err(|_| anyhow!("answerer data channel open signal lost"))?;

    Ok((
        DcEndpoint {
            pc: offerer,
            dc: off_dc,
            incoming: off_in_rx,
        },
        DcEndpoint {
            pc: answerer,
            dc: ans_dc,
            incoming: ans_in_rx,
        },
    ))
}

// ---------------------------------------------------------------------------
// DatagramBackend over a webrtc-rs data channel.
// ---------------------------------------------------------------------------

/// A backend for one endpoint that talks to exactly one peer over an open data
/// channel. Build a matched pair with [`connected_backends`].
pub struct WebRtcBackend {
    local: PeerKey,
    remote: PeerKey,
    end: DcEndpoint,
}

/// Establish a loopback pair and wrap each side as a backend, tagging datagrams
/// with the given peer keys (which must equal each endpoint's iroh public key,
/// since [`crate::adapter`] routes by public key).
pub async fn connected_backends(
    key_a: PeerKey,
    key_b: PeerKey,
) -> Result<(WebRtcBackend, WebRtcBackend)> {
    let (a, b) = connect_pair().await?;
    Ok((
        WebRtcBackend {
            local: key_a,
            remote: key_b,
            end: a,
        },
        WebRtcBackend {
            local: key_b,
            remote: key_a,
            end: b,
        },
    ))
}

impl DatagramBackend for WebRtcBackend {
    type Receiver = WebRtcReceiver;
    type Sender = WebRtcSender;

    fn local_key(&self) -> PeerKey {
        self.local
    }

    fn split(self) -> (WebRtcReceiver, WebRtcSender) {
        (
            WebRtcReceiver {
                remote: self.remote,
                incoming: self.end.incoming,
                _pc: self.end.pc.clone(),
            },
            WebRtcSender {
                remote: self.remote,
                dc: self.end.dc,
                _pc: self.end.pc,
            },
        )
    }
}

pub struct WebRtcReceiver {
    remote: PeerKey,
    incoming: mpsc::UnboundedReceiver<Bytes>,
    // Keep the peer connection alive for the endpoint's lifetime.
    _pc: Arc<RTCPeerConnection>,
}

impl std::fmt::Debug for WebRtcReceiver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WebRtcReceiver").finish_non_exhaustive()
    }
}

impl DatagramReceiver for WebRtcReceiver {
    fn poll_recv(&mut self, cx: &mut Context<'_>) -> Poll<Option<Inbound>> {
        let from = self.remote;
        self.incoming
            .poll_recv(cx)
            .map(|opt| opt.map(|data| Inbound { data, from }))
    }
}

#[derive(Clone)]
pub struct WebRtcSender {
    remote: PeerKey,
    dc: Arc<RTCDataChannel>,
    _pc: Arc<RTCPeerConnection>,
}

impl std::fmt::Debug for WebRtcSender {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WebRtcSender").finish_non_exhaustive()
    }
}

impl DatagramSender for WebRtcSender {
    fn send(&self, dst: &PeerKey, data: &[u8]) {
        if dst != &self.remote {
            return;
        }
        // dc.send is async; the seam's send is sync + best-effort, so fire a
        // task and don't wait. Reordering is fine — QUIC handles it.
        let dc = self.dc.clone();
        let buf = Bytes::copy_from_slice(data);
        tokio::spawn(async move {
            let _ = dc.send(&buf).await;
        });
    }
}
