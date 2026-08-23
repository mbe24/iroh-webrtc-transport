//! Milestone 1b: byte round-trip with iroh's QUIC running over a real
//! webrtc-rs data channel (DTLS + SCTP, unreliable/unordered) on loopback.

mod common;

use iroh::SecretKey;
use iroh_webrtc_transport::{build_endpoint, webrtc_native};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn echo_round_trips_over_webrtc_data_channel() {
    let client_sk = SecretKey::from_bytes(&[7u8; 32]);
    let server_sk = SecretKey::from_bytes(&[9u8; 32]);
    let client_key = *client_sk.public().as_bytes();
    let server_id = server_sk.public();
    let server_key = *server_id.as_bytes();

    // Signaling-equivalent: open the WebRTC channels before iroh binds.
    let (client_backend, server_backend) = webrtc_native::connected_backends(client_key, server_key)
        .await
        .expect("establish webrtc data channels");

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
