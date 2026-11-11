# city-game

A cooperative city simulation delivered as a live video stream: one
authoritative Rust server that simulates, renders (wgpu) and encodes, and a
thin Svelte viewer that only plays the video and sends raw input.

- Start: `task run` — builds, starts the server (:8090), and opens the viewer
  in your browser. Needs `bun`, `cargo`, and a Vulkan/Metal device.
- Test: `task test`.
- Architecture: [docs/architecture.md](docs/architecture.md)
- Wire + persistence contract: [docs/contracts/multiplayer-stream.md](docs/contracts/multiplayer-stream.md)
- Product intent: [GENESIS.md](GENESIS.md)
