//! In-memory datagram backend (Milestone 1a reference).
//!
//! A shared registry of `tokio::mpsc` channels keyed by peer. Reliable and
//! ordered — stronger than a real datagram carrier — which is fine: it isolates
//! the iroh/QUIC layering from any transport flakiness so the seam itself is
//! what's under test. The native/browser WebRTC backends implement the same
//! [`DatagramBackend`] and drop straight into [`crate::adapter`].

use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    task::{Context, Poll},
};

use bytes::Bytes;
use tokio::sync::mpsc;

use crate::datagram::{DatagramBackend, DatagramReceiver, DatagramSender, Inbound, PeerKey};

/// A shared in-memory "wire". Cloning shares the same underlying registry.
#[derive(Debug, Clone, Default)]
pub struct MemNetwork {
    inner: Arc<Mutex<BTreeMap<PeerKey, mpsc::Sender<Inbound>>>>,
}

impl MemNetwork {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register `key` on the wire and return its backend, ready to install on an
    /// endpoint builder.
    pub fn backend(&self, key: PeerKey) -> MemBackend {
        let (tx, rx) = mpsc::channel(1024);
        self.inner.lock().unwrap().insert(key, tx);
        MemBackend {
            key,
            net: self.clone(),
            rx,
        }
    }
}

/// The backend handed to [`crate::adapter::BackendTransport::new`].
#[derive(Debug)]
pub struct MemBackend {
    key: PeerKey,
    net: MemNetwork,
    rx: mpsc::Receiver<Inbound>,
}

impl DatagramBackend for MemBackend {
    type Receiver = MemReceiver;
    type Sender = MemSender;

    fn local_key(&self) -> PeerKey {
        self.key
    }

    fn split(self) -> (MemReceiver, MemSender) {
        (
            MemReceiver { rx: self.rx },
            MemSender {
                from: self.key,
                net: self.net,
            },
        )
    }
}

#[derive(Debug)]
pub struct MemReceiver {
    rx: mpsc::Receiver<Inbound>,
}

impl DatagramReceiver for MemReceiver {
    fn poll_recv(&mut self, cx: &mut Context<'_>) -> Poll<Option<Inbound>> {
        self.rx.poll_recv(cx)
    }
}

#[derive(Debug, Clone)]
pub struct MemSender {
    from: PeerKey,
    net: MemNetwork,
}

impl DatagramSender for MemSender {
    fn send(&self, dst: &PeerKey, data: &[u8]) {
        let guard = self.net.inner.lock().unwrap();
        if let Some(tx) = guard.get(dst) {
            // Drop-on-full is correct: QUIC retransmits.
            let _ = tx.try_send(Inbound {
                data: Bytes::copy_from_slice(data),
                from: self.from,
            });
        }
    }
}
