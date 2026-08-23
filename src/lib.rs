//! A droppable iroh custom-transport plugin: iroh owns identity + QUIC; a
//! pluggable datagram backend (headline: WebRTC) carries the bytes, so a
//! browser can be a first-class peer. No flowcontrol logic lives here.
//!
//! iroh normally carries its QUIC over IP (with a relay fallback). The
//! `unstable-custom-transports` feature lets us substitute our own datagram
//! carrier. This crate defines the [`datagram`] carrier abstraction (a
//! `DatagramBackend`), a generic [`adapter`] that turns any backend into an iroh
//! custom transport, and concrete backends:
//!
//! * [`in_memory`] — an in-memory network (Milestone 1a): the known-good reference.
//! * native WebRTC (Milestone 1b) and browser WebRTC (Milestone 2) plug into the
//!   same seam.
//!
//! Across all of them iroh owns identity (the `EndpointId` *is* the public key),
//! the QUIC/TLS handshake, and streams; the backend only moves opaque bytes.

use std::sync::Arc;

use iroh::{
    endpoint::{presets, Builder},
    Endpoint, RelayMode, SecretKey,
};

pub mod adapter;
pub mod datagram;

/// In-memory backend (1a). Native only — it uses tokio channels.
#[cfg(not(all(target_family = "wasm", target_os = "unknown")))]
pub mod in_memory;

/// Native WebRTC backend (Milestone 1b) via webrtc-rs. Native, opt-in.
#[cfg(all(
    not(all(target_family = "wasm", target_os = "unknown")),
    feature = "webrtc-native"
))]
pub mod webrtc_native;

/// Browser WebRTC backend (Milestone 2). wasm32 only.
#[cfg(all(target_family = "wasm", target_os = "unknown"))]
pub mod webrtc_wasm;

/// Browser chat entrypoint (`start_chat` / `Chat`). wasm32 only.
#[cfg(all(target_family = "wasm", target_os = "unknown"))]
pub mod chat;

pub use adapter::{addr_of, BackendTransport, TRANSPORT_ID};
pub use datagram::{DatagramBackend, DatagramReceiver, DatagramSender, Inbound, PeerKey};

/// Build an endpoint whose *only* transport is `backend`: no IP, no relay.
///
/// `presets::Minimal` installs just the rustls crypto provider (needs the
/// `tls-ring` feature); QUIC/TLS still runs, authenticated by the raw public
/// key. Callers dial by an explicit `EndpointAddr` carrying [`addr_of`], so no
/// discovery service is consulted.
pub fn build_endpoint<B>(sk: SecretKey, backend: B) -> Builder
where
    B: DatagramBackend,
{
    let builder = Endpoint::builder(presets::Minimal)
        .secret_key(sk)
        .relay_mode(RelayMode::Disabled)
        .add_custom_transport(Arc::new(BackendTransport::new(backend)));
    // IP transports only exist off-wasm; under `wasm_browser` there is nothing
    // to clear (and the method is cfg'd away), so the custom transport is
    // already the sole path.
    #[cfg(not(all(target_family = "wasm", target_os = "unknown")))]
    let builder = builder.clear_ip_transports();
    builder
}
