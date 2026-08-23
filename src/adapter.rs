//! Generic iroh custom-transport adapter.
//!
//! Wraps any [`DatagramBackend`] as iroh's three-trait custom transport
//! (factory -> endpoint -> sender). This is the only code that touches the
//! `unstable-custom-transports` API; backends stay iroh-agnostic. The
//! `poll_recv` / `poll_send` contracts mirror iroh's own
//! `test_utils::test_transport`.

use std::{
    io,
    sync::{Arc, Mutex},
    task::{Context, Poll},
};

use iroh::endpoint::transports::{CustomEndpoint, CustomSender, CustomTransport, RecvInfo, Transmit};
use iroh_base::CustomAddr;

use crate::datagram::{DatagramBackend, DatagramReceiver, DatagramSender, PeerKey};

/// Transport-type discriminator shared by every backend in this crate. The
/// `data` bytes of a [`CustomAddr`] with this id are a [`PeerKey`].
pub const TRANSPORT_ID: u64 = 0x77_65_62_72; // "webr"

/// Build the iroh custom address for a peer key.
pub fn addr_of(key: &PeerKey) -> CustomAddr {
    CustomAddr::from_parts(TRANSPORT_ID, key)
}

fn key_of(addr: &CustomAddr) -> Option<PeerKey> {
    addr.data().try_into().ok()
}

/// iroh `CustomTransport` factory over a backend. The backend's split halves are
/// parked here until iroh calls [`bind`](CustomTransport::bind) exactly once.
pub struct BackendTransport<B: DatagramBackend> {
    local: CustomAddr,
    parts: Mutex<Option<(B::Receiver, B::Sender)>>,
}

impl<B: DatagramBackend> BackendTransport<B> {
    pub fn new(backend: B) -> Self {
        let local = addr_of(&backend.local_key());
        Self {
            local,
            parts: Mutex::new(Some(backend.split())),
        }
    }
}

// Hand-written to avoid a spurious `B: Debug` bound the derive would add (the
// backend value is consumed in `new`; only the split halves live here).
impl<B: DatagramBackend> std::fmt::Debug for BackendTransport<B> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BackendTransport")
            .field("local", &self.local)
            .finish_non_exhaustive()
    }
}

impl<B: DatagramBackend> CustomTransport for BackendTransport<B> {
    fn bind(&self) -> io::Result<Box<dyn CustomEndpoint>> {
        let (rx, tx) = self
            .parts
            .lock()
            .unwrap()
            .take()
            .ok_or_else(|| io::Error::other("backend already bound"))?;
        Ok(Box::new(BackendEndpoint {
            local: self.local.clone(),
            rx,
            tx,
            watch: n0_watcher::Watchable::new(vec![self.local.clone()]),
        }))
    }
}

#[derive(Debug)]
struct BackendEndpoint<R, S> {
    local: CustomAddr,
    rx: R,
    tx: S,
    watch: n0_watcher::Watchable<Vec<CustomAddr>>,
}

impl<R: DatagramReceiver, S: DatagramSender + Clone> CustomEndpoint for BackendEndpoint<R, S> {
    fn watch_local_addrs(&self) -> n0_watcher::Direct<Vec<CustomAddr>> {
        self.watch.watch()
    }

    fn create_sender(&self) -> Arc<dyn CustomSender> {
        Arc::new(BackendSenderAdapter {
            inner: self.tx.clone(),
        })
    }

    fn poll_recv(
        &mut self,
        cx: &mut Context,
        bufs: &mut [io::IoSliceMut<'_>],
        metas: &mut [noq_udp::RecvMeta],
        recv_infos: &mut [RecvInfo],
    ) -> Poll<io::Result<usize>> {
        let cap = bufs.len();
        if cap == 0 {
            return Poll::Pending;
        }
        let local = self.local.clone();
        let mut count = 0;
        while count < cap {
            match self.rx.poll_recv(cx) {
                Poll::Ready(Some(inb)) => {
                    let len = inb.data.len();
                    if bufs[count].len() < len {
                        continue; // datagram too big for the slot; drop, QUIC retransmits
                    }
                    bufs[count][..len].copy_from_slice(&inb.data);
                    metas[count].len = len;
                    metas[count].stride = len;
                    recv_infos[count] = RecvInfo::new(addr_of(&inb.from), Some(local.clone()));
                    count += 1;
                }
                // Idle or closed both mean "nothing to hand up right now". Never
                // surface Ok(0) (iroh's soft-close) from a live endpoint.
                Poll::Ready(None) | Poll::Pending => break,
            }
        }
        if count == 0 {
            Poll::Pending
        } else {
            Poll::Ready(Ok(count))
        }
    }
}

#[derive(Debug)]
struct BackendSenderAdapter<S> {
    inner: S,
}

impl<S: DatagramSender> CustomSender for BackendSenderAdapter<S> {
    fn is_valid_send_addr(&self, addr: &CustomAddr) -> bool {
        addr.id() == TRANSPORT_ID
    }

    fn poll_send(
        &self,
        _cx: &mut Context,
        dst: &CustomAddr,
        _src: Option<&CustomAddr>,
        transmit: &Transmit<'_>,
    ) -> Poll<io::Result<()>> {
        if let Some(key) = key_of(dst) {
            // A GSO batch is `contents` cut into `segment_size` datagrams.
            let seg = transmit
                .segment_size
                .unwrap_or(transmit.contents.len())
                .max(1);
            for chunk in transmit.contents.chunks(seg) {
                self.inner.send(&key, chunk);
            }
        }
        Poll::Ready(Ok(()))
    }
}
