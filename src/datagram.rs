//! The datagram-carrier abstraction.
//!
//! iroh's custom transport is datagram-oriented: it hands us opaque UDP-like
//! packets to deliver and expects opaque packets back, each tagged with a peer.
//! This module names that contract as three small traits so the iroh adapter in
//! [`crate::adapter`] is generic over *how* datagrams travel. A concrete carrier
//! — the in-memory network ([`crate::in_memory`]), a native WebRTC data channel, or a
//! browser `RTCDataChannel` — implements these and nothing about iroh.
//!
//! Peers are addressed by their iroh endpoint public key ([`PeerKey`]); a
//! WebRTC backend maps that key to an already-established data channel.

use std::{
    fmt::Debug,
    task::{Context, Poll},
};

use bytes::Bytes;

/// Routing key for a peer — its iroh endpoint public key (32 bytes).
pub type PeerKey = [u8; 32];

/// An inbound datagram plus the peer it came from.
#[derive(Debug, Clone)]
pub struct Inbound {
    pub data: Bytes,
    pub from: PeerKey,
}

/// Receive half of a datagram carrier. Owned by the bound endpoint and polled
/// by a single consumer (iroh's recv driver), hence `&mut self`.
///
/// Return `Poll::Pending` (registering the waker) when idle. Return
/// `Poll::Ready(None)` only on permanent closure — the adapter treats it as idle,
/// never as an error, so a transient hiccup won't tear the endpoint down.
///
/// Requires `Sync` because iroh holds the bound endpoint behind a `Sync`
/// bound; since `poll_recv` is `&mut self` (single consumer) there are no
/// shared-reference methods, so `Sync` is trivially satisfiable (the in-memory
/// `mpsc::Receiver` and a tokio-channel-backed WebRTC receiver both qualify).
pub trait DatagramReceiver: Debug + Send + Sync + 'static {
    fn poll_recv(&mut self, cx: &mut Context<'_>) -> Poll<Option<Inbound>>;
}

/// Send half of a datagram carrier. Shared behind an `Arc` and callable
/// concurrently, hence `&self`.
///
/// Sends are **best-effort and non-blocking**: on backpressure or a path that
/// isn't established yet, drop the datagram. QUIC (running above) retransmits,
/// so a dropped datagram is a delay, never data loss — and never an error that
/// could kill the transport.
pub trait DatagramSender: Debug + Send + Sync + 'static {
    fn send(&self, dst: &PeerKey, data: &[u8]);
}

/// A datagram carrier bound to one local identity: a receive half, a cloneable
/// send half, and the local key. This is the whole seam a concrete transport
/// implements.
pub trait DatagramBackend: Send + 'static {
    type Receiver: DatagramReceiver;
    type Sender: DatagramSender + Clone;

    /// The local peer key iroh will advertise as this transport's address.
    fn local_key(&self) -> PeerKey;

    /// Consume the backend into its receive and send halves. Called once, when
    /// iroh binds the endpoint.
    fn split(self) -> (Self::Receiver, Self::Sender);
}
