# iroh-webrtc-transport

Carry **iroh's QUIC over a WebRTC data channel**, so a **browser is a first-class
iroh peer** — browser↔browser (and anything↔anything) with **no relay in the data
path**. A small, droppable [iroh](https://github.com/n0-computer/iroh) custom
transport for **iroh 1.0.x**.

iroh gives you identity (the `EndpointId` *is* an ed25519 public key), the
QUIC/TLS handshake, and streams. This crate lets that QUIC ride over a datagram
carrier *you* choose. The headline carrier is a **WebRTC data channel** (the one
path that reaches into browsers today); an in-memory carrier and a native
`webrtc-rs` carrier are included behind the same seam.

> **Status: experimental.** Rides iroh's *unstable* `unstable-custom-transports`
> feature; the browser backend uses a single `unsafe` Send/Sync shim (sound
> because wasm is single-threaded). Tested on loopback/LAN. Meant to be
> **dropped** the day iroh ships native browser↔browser — see
> [Droppability](#droppability).

## Live chat demo (browser↔browser, serverless)

The `examples/chat` demo runs iroh over a browser WebRTC channel and lets two
browsers chat directly. **Signaling is serverless**, so it works on static
hosting (GitHub Pages):

- **Two tabs, same browser** → auto-pair over `BroadcastChannel`. Zero setup.
- **Across devices** → exchange an offer/answer link (`?manual`). No server.

```bash
npm install      # optional: gets wasm-pack for rebuilding
npm start        # serves examples/chat on http://127.0.0.1:8091
```

Open <http://127.0.0.1:8091/?room=demo> in **two tabs** and chat. The built wasm
bundle is committed, so `npm start` works with no Rust toolchain. To rebuild it:

```bash
npm run build    # wasm-pack build → examples/chat/pkg   (needs Rust + clang)
```

> **On "everyone in one room" and rendezvous:** a shared room name doesn't remove
> the need for peers to *discover each other*. The two serverless modes cover the
> realistic cases — same-browser tabs (BroadcastChannel) and a shared link
> (manual). A public "join a room with strangers" lobby would need one small
> hosted signaling relay (Pages can't run it); that's deliberately out of scope
> here to keep the demo backend-free.

## How it works

| Layer | Owner |
|-------|-------|
| Signaling / discovery (SDP + ICE) | **the app** (demo: BroadcastChannel or manual link — in JS) |
| WebRTC data channel | the browser (or `webrtc-rs` natively) |
| Datagram carrier (`DatagramBackend`) | **this crate** |
| QUIC + identity + streams | **iroh** |
| Application bytes | your app |

Signaling is intentionally *not* the transport's job: the app opens a data
channel however it likes, then hands it to this crate, which runs iroh's QUIC
over it. One seam, three backends:

```
src/datagram.rs       DatagramBackend / DatagramReceiver / DatagramSender  (iroh-agnostic)
src/adapter.rs        turns any backend into an iroh custom transport       (the only iroh-facing code)
src/in_memory.rs      in-memory backend (reference / tests)                 [native]
src/webrtc_native.rs  webrtc-rs data channel                                [native, feature `webrtc-native`]
src/webrtc_wasm.rs    browser RTCDataChannel                                [wasm32]
src/chat.rs           start_chat / Chat — the browser chat entrypoint        [wasm32]
```

## Using the crate (Rust)

Every backend plugs into one entry point:

```rust
use iroh_webrtc_transport::{build_endpoint, in_memory::MemNetwork};

let net = MemNetwork::new();
let backend = net.backend(*secret_key.public().as_bytes());
let endpoint = build_endpoint(secret_key, backend).bind().await?; // a normal iroh Endpoint
// …then use iroh's usual connect / accept / streams.
```

Implement [`DatagramBackend`](src/datagram.rs) to carry iroh over any datagram
transport of your own.

**Features:** `default = ["webrtc-native"]`. Use `--no-default-features` to drop
`webrtc-rs` and keep just the seam + adapter + in-memory backend. The browser
backend is compiled automatically for `wasm32-unknown-unknown`.

## Verification

- `cargo test` — in-memory (1a) and native `webrtc-rs` (1b) round-trips: iroh's
  QUIC + a bi-stream over a real WebRTC data channel (DTLS + SCTP).
- Browser↔browser (2) — verified live in two tabs: multi-message chat over iroh
  QUIC on a browser `RTCDataChannel`, BroadcastChannel signaling, no data relay.

## Droppability

This exists because iroh can't yet do direct browser↔browser without relaying.
It's built to be removed: it's pure iroh-transport code with no app logic, so
when iroh ships native browser↔browser you delete the crate and swap the
endpoint builder — nothing app-side changes.

## Credits & license

Independent clean-room work, but indebted to prior art — see [CREDITS.md](CREDITS.md)
(notably **SuddenlyHazel/iroh-webrtc-transport**, MIT, for the browser wasm
mechanics, and **anchalshivank/iroh-webrtc-transport** for the concept).

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.
