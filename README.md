# city-game

A cooperative city simulation delivered as a live video stream: one
authoritative Rust server, headless Chromium seats rendering the game
server-side, and pixel-only viewers watching and playing through the bridge.

- Start: `task run` — builds, starts the server (:8090) and video bridge
  (:8092), and opens the stream in your browser. Needs `bun`, `cargo`, Chrome.
- Architecture: [docs/architecture.md](docs/architecture.md)
- Wire + persistence contract: [docs/contracts/multiplayer-stream.md](docs/contracts/multiplayer-stream.md)
- Product intent: [GENESIS.md](GENESIS.md)
