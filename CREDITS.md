# Credits & prior art

This crate is an independent, clean-room implementation for **iroh 1.0.x**, but
it stands on the shoulders of earlier work that proved the concept and taught us
the mechanics. Thank you.

## Prior art (iroh + WebRTC)

- **[SuddenlyHazel/iroh-webrtc-transport](https://github.com/SuddenlyHazel/iroh-webrtc-transport)** — MIT, © 2026 Hazel.
  The primary reference for the browser WebRTC + wasm mechanics. We learned the
  web-sys data-channel patterns from it (ArrayBuffer↔bytes conversion, unordered/
  unreliable channel config, `send_with_u8_array`, closure wiring). Targets iroh
  0.98; we re-implemented for iroh 1.0.x behind our own datagram seam. A short
  MIT-attributed acknowledgement is included below.

- **[anchalshivank/iroh-webrtc-transport](https://github.com/anchalshivank/iroh-webrtc-transport)** —
  no license file (all rights reserved). Referenced as prior art / inspiration
  only (the str0m-based native approach and its standalone signaling server); no
  code was copied from it.

## Upstream building blocks

- **[iroh](https://github.com/n0-computer/iroh)** (n0-computer) — Apache-2.0 OR MIT.
  Our iroh-facing adapter follows the shape of iroh's own
  `test_utils::test_transport` from the `unstable-custom-transports` feature.
- **[webrtc-rs](https://github.com/webrtc-rs/webrtc)** — MIT OR Apache-2.0.
  The native backend's loopback data-channel setup follows the crate's own
  `data-channels` examples.

## MIT acknowledgement (SuddenlyHazel/iroh-webrtc-transport)

```
MIT License

Copyright (c) 2026 Hazel

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction ... (full text at the repository above).
```
