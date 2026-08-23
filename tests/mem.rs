//! Milestone 1a: byte round-trip over the in-memory datagram backend.

mod common;

use iroh::SecretKey;
use iroh_webrtc_transport::{build_endpoint, in_memory::MemNetwork};

#[tokio::test]
async fn echo_round_trips_over_in_memory_transport() {
    let net = MemNetwork::new();
    let client_sk = SecretKey::from_bytes(&[1u8; 32]);
    let server_sk = SecretKey::from_bytes(&[2u8; 32]);
    let server_id = server_sk.public();

    let client_backend = net.backend(*client_sk.public().as_bytes());
    let server_backend = net.backend(*server_id.as_bytes());

    let client = build_endpoint(client_sk, client_backend)
        .bind()
        .await
        .unwrap();
    let server = build_endpoint(server_sk, server_backend)
        .bind()
        .await
        .unwrap();

    common::echo_round_trip(client, server, server_id).await;
}
