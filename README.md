# city-game

A browser-based cooperative city simulation played over a live video stream:
one authoritative Rust server, headless browser seats rendering the game
server-side, and pixel-only viewers watching and playing through the bridge.

- Start: `task install && task run` (game on :8090, stream bridge on :8092).
- Architecture: [docs/architecture.md](docs/architecture.md)
- Software organization: [docs/software-organization.md](docs/software-organization.md)
- Real two-player walkthrough: [docs/example-walkthrough.md](docs/example-walkthrough.md)
- Wire + persistence contract: [docs/contracts/multiplayer-stream.md](docs/contracts/multiplayer-stream.md)
- Product intent: [GENESIS.md](GENESIS.md)
