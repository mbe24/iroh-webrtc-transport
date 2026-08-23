//! Shared round-trip harness: send a byte payload from one endpoint to another
//! over whatever custom transport the caller built, and assert it echoes back
//! intact. Backend-agnostic and dependency-free, so the in-memory (1a) and
//! WebRTC (1b) tests assert exactly the same behaviour.
//!
//! The payload is opaque bytes — a real app (e.g. flowcontrol) would put a Loro
//! delta here, but the transport neither knows nor cares.

use iroh::{
    endpoint::Connection,
    protocol::{AcceptError, ProtocolHandler, Router},
    Endpoint, EndpointAddr, EndpointId, TransportAddr,
};
use iroh_webrtc_transport::addr_of;
use tokio::sync::mpsc;

pub const ALPN: &[u8] = b"iroh-webrtc-transport/echo/0";
pub const PAYLOAD: &[u8] = b"payload over a custom iroh transport \x00\x01\x02\xff";

/// Server protocol: read the client's payload off a bi-stream, echo it back,
/// and report what it received so the test can assert it survived the transport.
#[derive(Debug, Clone)]
struct Echo {
    got: mpsc::UnboundedSender<Vec<u8>>,
}

impl ProtocolHandler for Echo {
    async fn accept(&self, connection: Connection) -> Result<(), AcceptError> {
        let (mut send, mut recv) = connection.accept_bi().await?;
        let bytes = recv
            .read_to_end(1 << 20)
            .await
            .map_err(AcceptError::from_err)?;
        send.write_all(&bytes).await.map_err(AcceptError::from_err)?;
        send.finish()?;
        let _ = self.got.send(bytes);
        connection.closed().await;
        Ok(())
    }
}

/// Send [`PAYLOAD`] from `client` to `server` over a QUIC bi-stream and assert
/// both the server-side receipt and the echo. `server_id` is the server
/// endpoint's public key; the client dials by an explicit `EndpointAddr`
/// carrying the custom address (no discovery).
pub async fn echo_round_trip(client: Endpoint, server: Endpoint, server_id: EndpointId) {
    let (got_tx, mut got_rx) = mpsc::unbounded_channel();
    let router = Router::builder(server)
        .accept(ALPN, Echo { got: got_tx })
        .spawn();

    let server_addr =
        EndpointAddr::from_parts(server_id, [TransportAddr::Custom(addr_of(server_id.as_bytes()))]);
    let conn = client.connect(server_addr, ALPN).await.unwrap();

    let (mut send, mut recv) = conn.open_bi().await.unwrap();
    send.write_all(PAYLOAD).await.unwrap();
    send.finish().unwrap();
    let echoed = recv.read_to_end(1 << 20).await.unwrap();
    assert_eq!(echoed, PAYLOAD, "echo mismatch");

    let received = got_rx.recv().await.expect("server reported no receipt");
    assert_eq!(received, PAYLOAD, "server received wrong bytes");

    conn.close(0u32.into(), b"done");
    router.shutdown().await.unwrap();
}
