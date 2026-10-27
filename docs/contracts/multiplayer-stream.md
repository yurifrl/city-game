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
- **Client**: `apps/web/src/presence/presence.ts` renders one cursor view per
  remote actor; drag state renders the armed building ghost at the remote
  cell with the actor's color; label shows on hover.

### 3.1 Input → presence mapping (normative)

- `InputMessage { kind, cell, def_id, rotation }`: `kind` ∈ `move | down |
  up`; `cell` always present; `def_id`/`rotation` present iff the placement
  tool is armed. The server MUST overwrite any client-supplied actor with the
  connection's own actor. `down`/`up` bracket the pointer press.
- Runner per-actor state: last `cell`; drag iff the latest input carries
  `def_id`; `up` clears the drag flag. Presence events fire on state CHANGE
  only, never per packet.
- Remote seats render `idle` actors as a labelled cursor at `cell` and `drag`
  actors as the armed-building ghost (per §3 client). A seat learns it is
  fully booted when it has applied `helloworld`; it then sets
  `window.__cityReady = true` (the seat manager's boot gate polls this).

## 4. Seat manager and video bridge (apps/stream)

- Bun + TypeScript service, port 8092. Manages a pool of `CITY_SEATS` (default
  2) warm headless Chrome seats, each booting `apps/web/dist` with `SEED`
  until the game socket is up (boot gate).
- `POST /api/seat` claims a seat, `POST /api/seat/{id}` updates it (viewport,
  DPR), `DELETE /api/seat/{id}` removes it. Status at `/api/status.json`.
- Viewer page connects to `/ws/{seatId}`; bridge relays CDP screencast JPEG
  frames downstream and viewer input upstream at native pixel scale.
- Tests: `test/happy-path.test.ts` (boot gate, pool readiness, 2-viewer claim
  + input + frame round-trip, chrome-log filter, auto-destroy teardown).

## 5. Consolidation (m2-consolidate)

Everything not serving the above is deleted: `apps/site`, `poc/` (after
evidence extraction), old `.agents/plans/` M1 plans,
`docs/contracts/milestone-1.md`. `Taskfile.yml` drops `site`/`docker` targets
and gains `stream`. `.gitignore` keeps `shots/`. `README.md` is a stub
pointing to `docs/`.

## 5b. Production topology (city-game.syscd.live)

One image, three modes (`Dockerfile`): default = `city-server` (static build
+ `/api` + `/ws` on :8080), `MODE=web` = busybox static hosting, `MODE=stream`
= the seat bridge on :8092 beside Alpine's `chromium-browser` (SwiftShader
rendering, `--no-sandbox` under the non-root uid). CI (`build.yml`) builds
amd64+arm64 images per commit, pins `chart/values.yaml` to `sha-<git sha>`,
and ArgoCD syncs `chart/`.

The chart runs one pod with both containers: `city-game` (authoritative
world, SQLite on longhorn) and `stream` (`CITY_GAME_URL=http://127.0.0.1:8080`,
`CITY_SEATS=2`). Istio routes the public host `city-game.syscd.live` to the
bridge service (viewer at `/view`), so the public experience is the streaming
viewer; the direct client stays on the tailnet host `city-game.syscd.tech`.
Cloudflare Access fronts the `.live` name; `syscd.tech` rides MagicDNS
without Access.

## 6. Verification (m2-verify)

- `task test:e2e` spawns server + seat manager, waits for pool readiness.
- Headless-chromium CDP flow: two viewers claim seats, both place cursor
  movement, one drags a building, the other sees the ghost; screenshots to
  `shots/`. Evidence attached to the responsible tk issue.
- Ownership round-trip: place → `save/buildings.owner` persisted → demolish
  → owner gone; receipts still idempotent.
