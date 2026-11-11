# Multiplayer-stream contract (M2)

Normative for every M2 slice. RFC 2119: **MUST** = required for acceptance,
**SHOULD** = default unless a slice records a reasoned deviation.

Derived from `docs/architecture.md` §2/§3/§4, the retired video-stream
handoff (measured feasibility numbers, archived on `tk-qma`), and the M2
issue plan on `tk-qma`. Statuses cited as `tk-<id>` refer to the repo task
tracker (`.tasks/`).

## 0. Product shape

One authoritative Rust server. N browser seats render the real client
headlessly server-side; M viewers receive a live JPEG video stream of a seat
and inject input back. Players are identified by server-issued actor identity;
placed structures record ownership; cursors and drag ghosts of every seat are
broadcast to all seats so any viewer sees other players' cursors and drags.

## 1. Player identity

- **Rust**: `crates/protocol` defines `ActorId(String)` wire alias and
  `ActorLabel(String)` (lowercase name used for random seat identities).
  All actor references on the wire MUST use `ActorId` strings.
- **Wire**: `Hello` carries `you: ActorId`. **MUST** be sent **before**
  `HelloWorld` on a fresh connection (client resume depends on ordering).
- **Server**: seat manager obtains a UUID from `GET /api/seed` (the handshake
  key) and passes it as `SEED` boot env. Server treats it as the player id.
  Lease TTL `CITY_PLAYER_TTL_SECS` (default 1800) re-arms on every command and
  input packet from that actor.
- **Label**: player label persisted server-side in a `players` table so
  reconnecting seats keep their name; default label derived from the UUID.

### 1.1 Frame order and shapes (normative)

Client → server, first message on every fresh connection:
`{"type":"identify","key":"<uuid>"}`. The server MUST treat `key` as the
actor id (it is the seat's `/api/seed` UUID).

Server → client, in order, on every fresh connection:
`{"type":"hello","you":"<actor-id>"}` then
`{"type":"helloworld","world":{...}}`. No world data may precede `hello`.

Client → server afterwards: `{"type":"command","command":{...}}` |
`{"type":"input","input":{...}}`. Commands keep their M1 shapes; the server
resolves the actor from the connection — the client payload never names it.

The web client derives its key from the `?seed=` query param when present
(seat boot), else generates a random key persisted in `localStorage`
(direct dev use). Reconnects MUST re-send `identify` with the same key.

## 2. Ownership of structures

- **Rust**: `crates/simulation/src/world.rs` — `Building` gains `owner:
  ActorId`. `insert_building`/`remove_building` remain the only mutators and
  MUST keep the occupancy index consistent with the owner field.
- **Construction**: `Placement` carries the owner; `apply_placement` MUST copy
  it onto the building. Demolition MUST record the demolishing actor.
- **Persistence**: `crates/server/src/persistence.rs` — `save/buildings` table
  gains `owner` column, schema_version 2. Receipt row shape unchanged; place
  outcome binds `owner` (actor). Reload MUST restore owners exactly.
- **Wire**: `BuildingDto` gains `owner: Option<ActorId>`; `CommandResult`
  accepted arm carries `owner`. `PlaceBuilding` command carries the actor
  (server-resolved, never trusted from the client payload).

## 3. Presence (cursors + drag ghosts)

- **Input path**: `crates/protocol` defines `InputMessage` (client → server):
  `move`, `down`, `up` with cell or pixel coords and `actor`.
  `PresenceMessage` (server → clients): `PresenceState` per actor:
  `idle | drag { def_id, rotation, cell }`.
- **Events**: `Event::presence` per actor; `PlayerJoined`/`PlayerLeft` events
  keyed by `ActorId`. Drag lifecycle: `down` → drag state, `up` → idle.
- **Runner**: `runner.rs` gains `presence.go(actor, state)`/`stop(actor)`.
  Presence/drag packets MUST NOT create receipts and MUST NOT persist.
  Disconnected actors are removed at the next tick after
  `CITY_PRESENCE_TTL_SECS` (default 30) of silence.
- **Hub**: `hub.rs` broadcasts `presence` events to **all** connections
  (world events continue to fan out; command results stay direct).
- **Rendering**: each seat (crates/server/src/seat) draws one cursor per remote
  actor; drag state draws the armed building ghost at the remote cell in the
  actor's colour. The browser only plays video.

### 3.1 Input → presence mapping (normative)

- `InputMessage { kind, cell, def_id, rotation }`: `kind` ∈ `move | down |
  up`; `cell` always present; `def_id`/`rotation` present iff the placement
  tool is armed. The server MUST overwrite any client-supplied actor with the
  connection's own actor. `down`/`up` bracket the pointer press.
- Runner per-actor state: last `cell`; drag iff the latest input carries
  `def_id`; `up` clears the drag flag. Presence events fire on state CHANGE
  only, never per packet.
- Remote seats render `idle` actors as a cursor at `cell` and `drag` actors as
  the armed-building ghost (§3 rendering).

## 4. Seats and video stream (crates/server/src/seat)

The browser has **no game, no game state and no rendering**: it plays video
and sends raw input. Everything else happens in `city-server`.

- `GET /stream?seed=<id>` upgrades to a **seat**: one viewer = one in-process
  player. The seat joins the world like any game-wire client (`identify`,
  replica from `helloworld` + events, presence, commands) and owns its camera
  and armed tool. `seed` is the actor identity (same seed = same player).
- Rendering: `crates/render` (wgpu, headless, one shared device behind a lock,
  fixed 1280x720). A seat renders only when its picture changed (idle gating),
  at most 30 fps, then JPEG-encodes (`CITY_STREAM_QUALITY`, default 75).
- Viewer -> server (JSON text, coordinates in frame pixels):
  `{"type":"pointer","action":"move|down|up","x","y","button"}`,
  `{"type":"wheel","delta_y"}`, `{"type":"key","code"}`. Keys: `Digit1-9`
  arm definition N, `KeyR` rotate, `KeyX`/`Delete` demolish under cursor,
  `Escape` disarm; left click places, right/middle drag pans, wheel zooms.
  The server derives cell, command and actor; the client names none of them.
  Malformed input is reported in the caption, never silently dropped.
- Server -> viewer: binary frames `u32 BE meta length | meta JSON | JPEG`
  (meta `{seq, format:"jpeg", w, h}`), text `{"type":"caption","text"}`
  (armed tool, last rejection reason, key help) and `{"type":"error","message"}`.
  A seat the hub sheds as a slow consumer is told and disconnected.
- Viewer: `apps/viewer` (Svelte + TypeScript): canvas, input forwarding,
  reconnect with bounded backoff, drops stale frames instead of queueing.
- Tests: `crates/server/tests/stream.rs` (real binary, real GPU, raw game-wire
  observer: place, overlap rejection, demolish, idle seat renders nothing),
  `crates/render/tests`, `apps/viewer/test`.

## 5b. Production topology (city-game.syscd.live)

One image, one process (`Dockerfile`): `city-server` serves the viewer page,
`/api`, `/ws` (game wire) and `/stream` (video) on :8080. It renders on the CPU
with Mesa lavapipe (`mesa-vulkan-swrast`; the cluster has no GPU) and exits at
boot if no Vulkan adapter exists. The binary links dynamically against musl
(`-crt-static`) because wgpu must `dlopen` libvulkan. CI (`build.yml`) builds
amd64+arm64 images per commit, pins `chart/values.yaml` to `sha-<git sha>`, and
ArgoCD syncs `chart/`.

The chart runs one pod with one container (SQLite on longhorn). One Istio
VirtualService routes both hosts (`city-game.syscd.live` behind Cloudflare
Access, `city-game.syscd.tech` on the tailnet) to the `city-game` service.
Resource sizes are provisional until frame time per seat is measured.

## 6. Verification (m2-verify)

- `cargo test --workspace` (needs a GPU adapter; fails rather than skips) and
  `bun test --cwd apps/viewer`.
- The stream test drives two clients: a seat viewer sending raw input and a raw
  game-wire observer asserting what the world committed, plus decoded JPEG
  pixels for what the viewer saw.
- Ownership round-trip: place → `save/buildings.owner` persisted → demolish
  → owner gone; receipts still idempotent.
