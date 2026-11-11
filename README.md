# city-game

A cooperative city simulation delivered as a live video stream: one
authoritative Rust server that simulates, renders (wgpu) and encodes, and a
thin Svelte viewer that only plays the video and sends raw input.

- Start: `task run` (or `cargo run -p city-server`) — one binary serves the
  embedded viewer, API and stream on http://127.0.0.1:8090. `cargo` builds the
  viewer via `build.rs`, so you need `bun`, `cargo`, and a Vulkan/Metal device.
- Test: `task test`.
- Architecture: [docs/architecture.md](docs/architecture.md)
- Wire + persistence contract: [docs/contracts/multiplayer-stream.md](docs/contracts/multiplayer-stream.md)
- Product intent: [GENESIS.md](GENESIS.md)
