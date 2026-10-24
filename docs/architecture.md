# Architecture decisions

Status: chosen starting architecture and explicit design targets; no implementation or benchmark yet. [Genesis](../GENESIS.md) defines intent, [research](research.md) supplies evidence, and the current M2 contract orders implementation.

## 1. Stack decision

| Responsibility | Choice | Reason / boundary |
|---|---|---|
| Authoritative simulation | Rust, focused domain crate | Explicit ownership, predictable storage, strong types, native server execution; not a full rendering/game-engine application |
| Server | Rust, Tokio + Axum, WebSockets | Async network I/O around a separately owned synchronous world runner |
| Browser application | TypeScript, Vite, React for panels/forms only | Browser/tooling ecosystem and maintainable editor UI; React does not own world entities or the frame loop |
| Renderer | Babylon.js; WebGPU preferred, explicit WebGL2 fallback | Integrated picking, instances, LOD and diagnostics reduce project-owned engine infrastructure |
| Visual representation | Orthographic low-poly 3D, selective sprites/impostors | Fits the references and integrated building/terrain editing without full sprite-baking constraints |
| Wire contract | Rust DTOs, Serde JSON initially, generated TypeScript via ts-rs | Small understandable first protocol; no manually duplicated wire types |
| Persistence | SQLite + rusqlite, one database/writer per world initially | Transactional durable state without a distributed database deployment |
| Asset source | Versioned declarative building/terrain recipes | Editable, reproducible, bounded content rather than executable scripts |
| Asset delivery | Engine-compiled geometry/metadata; glTF/GLB interchange and KTX2 when useful | Separate source from efficient runtime artifacts; do not force every procedural edit through file export |
| WebAssembly | Not a starting requirement | Reserve for proven CPU-heavy kernels or shared Rust logic that benefits from browser execution |

Pin compatible dependency versions during implementation. This is not a lockfile, and `latest` documentation does not establish a tested dependency set.

### Why not Rust/Wasm for all browser code?

WebAssembly runs CPU-side code; WebGPU/WebGL submit work to the GPU. They are complementary, not alternatives. TypeScript can already submit instanced draws through WebGPU. Rust/Wasm cannot by itself increase GPU memory, triangle throughput, pixel fill rate, or texture limits.

Wasm is a credible later choice for mesh generation, simplification, route analysis in a tool, or other measured kernels. Use coarse calls over bulk buffers and include transfer/copy costs in the measurement. Do not duplicate the server's simulation in the browser merely to justify Wasm. It is not needed for clients to inspect server-produced citizen state.

### Why Babylon rather than Three or a custom wgpu renderer?

This is a maintainability/performance decision, **not a claim Babylon is universally fastest**. Babylon thin instances avoid a scene object for every repeated mesh. Its integrated facilities reduce infrastructure we must own. Thin instances are particularly suitable for repeated mostly static buildings and props.

**The strongest challenger is Three.js `BatchedMesh`:** it explicitly supports distinct geometries with shared materials, per-object culling and sorting. A building designer can produce many unique shapes, so a repeated-cube benchmark is insufficient. Step 1 compares this heterogeneous case before content volume makes reversal expensive. Switch if a controlled representative comparison shows meaningfully better frame/edit latency without an unacceptable maintenance burden. Do not implement and maintain two product renderers.

A custom Rust/wgpu renderer offers control, but requires us to own resource management, shaders, batching, picking, LOD, browser capability handling and debugging. Do not pay that cost before profiling identifies a limit the chosen engine cannot reasonably address.

## 2. Code organization and dependency direction

Create these incrementally; the tree describes ownership, not permission to scaffold empty modules.

```text
crates/
  simulation/src/
    lib.rs                 narrow domain entry points
    world/                 IDs, coordinates, terrain, clock, entity ownership
    construction/          placement, occupancy, demolition, upgrades
    assets/                approved gameplay definitions and validation
    utilities/             electricity and water topology/supply
    citizens/              individual state, needs, activities, lifecycle
    housing/               households, suitability, occupancy, demand
    economy/               money, jobs, inventory, transactions
    transport/             access, routes, travel progress
  server/src/
    main.rs                composition root
    worlds/                world runners, sequencing, tick/commit lifecycle
    protocol/              external DTOs and generated-schema export
    sessions/              membership, permissions, connection lifecycle
    replication/           snapshots, deltas, subscriptions, backpressure
    persistence/           save records, SQLite mapping and migrations
    assets/                publish/load approved asset revisions
apps/web/src/
  app/                     composition, routing, panels
  protocol/                generated wire declarations and runtime decoding
  connection/              transport, reconnect, pending command receipts
  world/                   authoritative replica and read-only selectors
  renderer/                Babylon resources, batches, culling, LOD, picking
  assets/                  recipe compiler, catalogue, derived visual data
  tools/                   construction/terrain intentions and local previews
  editors/                 building and terrain designer UI
  inspectors/              citizen, building, economy and network explanations
```

Dependency direction:

- Server composition → orchestration/adapters → simulation.
- Server protocol ↔ explicit adapter conversions ↔ domain commands/read models. Simulation never imports protocol DTOs, sockets, SQL, Tokio, or the renderer.
- Web tools → command submission; incoming protocol → replica; renderer/inspectors consume projections of the replica.
- Editor UI → asset compiler → renderer preview; publishing → server asset validation. The renderer never defines gameplay capacities or building occupancy.
- One web app contains game and designer modes. Share their real compiler/renderer modules; extract a package only when an independent consumer/build needs it.

No general `engine`, `common`, or `shared` dumping ground; no microservice per domain. The engine is the combination of clearly owned domain and visual systems, not a universal framework. Store public boundary contracts near the owner and keep implementation details private.

## 3. Domain representation

### Coordinates and identity

Use integer grid coordinates and explicit height units for authoritative construction. Convert to floating-point render coordinates at the adapter boundary. Begin with one height per terrain sample: caves, overhangs and full volumetric terrain are outside the initial model. Document cell size, height unit, origin, orientation and entrance/socket conventions before authoring content.

Stable `PersonId`, `HouseholdId`, `BuildingId` and asset revision references survive compaction, save/reload and network reconnection. Internal array slots, ECS handles and instance indices may change and never become persistent identity. Maintain explicit lookup mappings.

### Storage and ECS

Start with typed dense collections and chunked terrain. Keep hot fields (need levels, next decision time, location/activity) separate from cold biography and story history. Do not create one task, mutex, heap-heavy object tree, or renderer object per person.

Do not invent an ECS framework. If heterogeneous components and cross-cutting queries justify one, `bevy_ecs` can be used standalone behind the domain API. Table versus sparse-set storage and component churn have tradeoffs; ECS is not a population-scale guarantee. No full Bevy application is required for the server.

### Time and causality

A proposed initial base cadence is **10 simulation ticks per real second**, with an explicit amount of game time per tick. This is a tuning hypothesis. Render independently at the display rate; publish projections at a separately bounded cadence. The owner controls pause/speed; clients cannot advance time independently.

An explicit tick order begins with admitted commands, affected topology repair, supply allocation, scheduled activities/travel, needs/economy/lifecycle updates, then derived demand/explanations. Define one-tick lags and read/write phases rather than allowing accidental circular system dependencies. For example, newly occupied housing changes the next declared utility allocation, not an undefined same-tick loop.

Use elapsed-time integration for simple progress, and scheduled due events for decisions. Stagger work deterministically. Never skip consequential thresholds when batching long intervals. Bounded route/decision queues need fairness so the same residents cannot be deferred forever. At overload, show a slower/paused simulation clock rather than silently discarding economically meaningful ticks. Cap catch-up work. Initially a stopped server pauses the world; no implicit offline catch-up.

Camera position and connected observer count do not change authoritative needs or outcomes. Far-away people keep identities, homes, jobs, intentions and travel progress even when drawn as markers or not drawn at all.

### Utilities, people, and economy

- Maintain separate road-access, electricity and water graphs; sharing a corridor does not make them the same network.
- First utility model: affected connected components, available production/source capacity, consumer demand and a documented deterministic shortage-allocation rule. Recompute topology on edits; update supply/demand when state changes. Connectivity alone is not service delivery.
- Use declared units and conservation: allocated consumption cannot exceed available supply; demand is separate from actual consumption. Initially omit electrical load-flow physics and hydraulic pressure. Introduce line capacity, storage, sewage and contamination as later playable loops.
- Store explicit household membership and housing slots; occupancy and resident references must agree. Separate needs, service conditions and derived well-being.
- Use stable home/job assignments. Activity choice considers needs and access; a failed trip is a state with a consequence, not a silent teleport or instant success.
- Start food supply as an explicit external source with inventory and costs, not a hidden infinite refill. Add local supply chains only after the basic loop works.
- Use integer monetary units and explicit transactions. Demand must identify households, eligibility and limiting causes; no unexplained growth multiplier stands in for a simulation.

### Routing and parallel work

Begin with A* on the road/walkability graph, cached routes and topology-version invalidation. Rendering a grid does not require routing across every terrain cell. Replan on changed intent or relevant connectivity, not each frame. Use travel-time progress before lane-by-lane traffic simulation.

Hierarchical routing is a later optimization for measured long-route search cost. It does not solve congestion by itself. Worker threads may compute read-only route proposals; the world owner commits a result only if person, intent and topology versions still match. Stable ordering and explicit RNG streams support reproducibility. Parallel completion order, floating point, hash iteration and changing content can break replay; Rust and a fixed timestep do not guarantee determinism.

## 4. Authoritative multiplayer

### Ownership and command lifecycle

One logical writer owns each world. Initially use a dedicated world runner outside Tokio's async I/O workers; synchronous simulation and SQLite work must not block unrelated socket tasks.

1. Authenticate a session and resolve world membership. Enforce message/collection sizes, rates and a bounded ingress queue.
2. Receive a versioned command with world ID, unique command ID, operation, and relevant expected object/chunk/asset revisions. Identity comes from the session, never a trusted client actor field.
3. Assign server order and tick boundary. Do not order by client clocks or reject all commands simply because an unrelated world tick advanced.
4. Check durable command receipts scoped by actor/world. Same ID and payload returns the recorded outcome; same ID with a different payload is an error.
5. Validate against state after earlier ordered commands: permissions, footprint, terrain, entrance, content revision, resources, occupants and costs. Apply the entire operation atomically. Conflicting placement has one winner; large terrain brushes are bounded atomic transactions initially.
6. Commit state changes, authoritative tick/revision, ID allocation and command receipts together. Only after durable commit publish authoritative updates and committed results.
7. Replace the client's local ghost with the accepted entity, or remove it and display a structured rejection. Undo is a new validated compensating command, never a rewind of another player's work.

Queue acceptance is not durable success. A storage failure pauses/fails the world without broadcasting provisional changes. If provisional memory cannot safely roll back, reload last committed state. Do not solve this with a full-world copy every tick.

Keep command receipts across reconnect/restart. If retention is later bounded, expired retry identities must be rejected through an explicit epoch/expiration contract, not silently reapplied.

### Replication

Use HTTP for app/assets and WebSocket state replication. Initial small worlds may send bounded whole-world projections; add chunk subscriptions as measurements demand. Citizen details can be requested on inspection rather than broadcasting every biography.

- Snapshot: consistent baseline, world epoch, tick, content/protocol versions and stream sequence. Install only after complete assembly.
- Delta: explicit base/next stream sequence, changed records, and removals. Simulation tick, command order and subscription sequence are distinct concepts.
- Reconnect: resume a retained valid delta chain or receive a fresh snapshot. Retrieve pending command outcomes; do not resend with new IDs.
- Bound outbound messages **and bytes**, snapshot chunks and in-flight unacknowledged application updates. A count-bounded channel does not bound arbitrarily large messages.
- World simulation never waits on a slow socket. Dropping a middle delta requires resynchronization; only explicitly superseding updates may be coalesced. Presence/cursor packets may be dropped separately.
- Use client processed-sequence acknowledgements: a socket accepting bytes does not prove the browser applied them. Overflow leads to resync/disconnect, not unbounded browser queues.

Server authority avoids browser simulation lockstep, so a disconnected editor does not stall the world. CRDT convergence does not enforce non-overlap or prevent two purchases spending the same funds; do not use CRDTs for authoritative city state. Collaborative draft editing can be reconsidered separately.

### Protocol details

Rust wire DTOs are the source of truth; generate TypeScript declarations with `ts-rs`. Keep wire, save and asset versions separate. Runtime decode/validate untrusted values on both sides; generated types are not validators. Start with JSON for clarity. Move high-volume projections to a binary representation only after measuring payload and decoding costs; version that change explicitly.

Represent arbitrary 64-bit IDs, ticks and sequences as decimal strings in the **actual wire DTO/serialization**. JavaScript numbers cannot safely represent every `u64`; a TypeScript `bigint` annotation alone does not fix JSON. Do not expose internal entity layouts over the wire.

## 5. Persistence and deployment

Start with one server process/host, persistent local disk, and one SQLite database/writer per world. WAL allows concurrent readers but still only one writer. Configure durability intentionally (`synchronous=FULL` for acknowledged durable commits), verify the actual linked SQLite is patched, and use supported online backup facilities. Do not copy just a live main database file or put WAL databases on a shared network filesystem.

Persist changed domain records transactionally: people/households, homes/jobs, needs, scheduled work, travel timing, terrain/construction, networks or their authoritative inputs, assets, RNG state, tick and command receipts. Derivable caches may rebuild. Do not persist every interpolated visual coordinate. Avoid expensive long-lived readers that prevent WAL checkpoints.

A full simulation save, a client projection snapshot and a WAL checkpoint are different artifacts. Prefer restoring committed current state rather than promising that a command-only log reconstructs autonomous life. Store schema, simulation and content versions; migrate explicitly before loading. Audit/replay tooling can be added with a precisely tested version-scoped contract.

Scale independent worlds across processes/hosts before distributing one city. Later ownership transfer requires fencing/leases and explicit save/transfer/start; two owners must never mutate one world. A client-server database can be introduced for actual operational needs without changing domain rules. No initial Redis, Kafka, distributed ECS, service mesh or world sharding.

## 6. Integrated asset engine

### Three distinct artifacts

| Artifact | Owns | Does not own |
|---|---|---|
| Editable recipe | Footprint/masses, floors, roofs, facade/palette, terrain parameters, named archetype, deterministic variation seed | Arbitrary trusted code or final authoritative city state |
| Approved gameplay definition | Canonical footprint, entrances, terrain constraints, occupancy rules, utility sockets, validated/derived costs and capacities | Babylon meshes, GPU buffers, UI state |
| Compiled visual artifact | Mesh/instance prototypes, textures/material references, bounds, LODs, picking metadata, build hash | Housing occupancy, money, simulation results |

First compiler: bounded declarative mass/facade operations, not a new scripting language. Editable geometry can be richer than collision/buildability data. Server approval derives/verifies gameplay numbers from allowed archetypes and dimensions; a client cannot publish free buildings with arbitrary capacity.

The Asset Studio (`../building-studio`, separate process on `:8200`) is the
authoring front end for these recipes. Its MCP server exposes part-level
authoring tools; `city_publish` converts a studio scene into a `Recipe` with
an optional `parts` array (metre-space boxes/prisms relative to the footprint
min corner) and submits it through the same `POST /api/assets` path as the
web designer. Empty `parts` means the compiler derives the procedural
single-mass prototype; non-empty parts compile to a merged multi-part mesh
with one material per color. Definitions are removed only while no placed
building references them (`DELETE /api/assets/{id}` → `definition_removed`).

Content key includes recipe revision, compiler version, palette/material versions, and seed. Publish immutable definitions; placed instances pin a revision. Keep referenced old content available. Existing buildings change only through an explicit upgrade/migration that validates occupants, dimensions, entrances and costs.

### Building compilation

1. Validate finite values, dimensions, footprint, slope/entrance constraints, permitted archetype and complexity quotas.
2. Generate low-poly masses and recognizable roofs; use repeating facade textures/trim layouts for windows rather than one mesh per window.
3. Share materials and geometry families; preserve unique silhouettes. Cache reusable compiled prototypes. Pack atlas UVs with mip-safe padding and inspect facade readability at zoomed-out sizes.
4. Produce conservative bounds, stable entity-picking mappings and lower-detail representations. Report triangles, material groups, texture footprint and compiled size before publication.
5. Preview through the same renderer/material/lighting path used in the city. Publish only validated revisions; invalid assets receive actionable errors, not silent substitution.

Use glTF/GLB for exchange and authored external assets; the editable recipe remains source. KTX2/Basis can reduce texture download/GPU-memory costs, but adds transcoding/tooling costs: evaluate visual quality and load time for crisp facade patterns. Geometry or texture compression is not free and does not reduce every runtime cost.

### Terrain compilation

Begin with bounded heightfield chunks, surface material indices, water level/source metadata, and buildability derived from authoritative rules. Brush changes invalidate affected geometry, bounds and topology—not the whole map. Handle borders/normals and later LOD seams consistently.

Initial height changes overlapping occupied footprints are rejected. Later engineering/remediation is an explicit operation. Water graphics remain separate from utility supply and later hydrology. Geometry clipmaps are a useful large-terrain precedent, not a required first implementation.

## 7. Dense-city rendering pipeline

1. Partition world data into measured-size spatial chunks. Frustum-cull using conservative bounds that include tall roofs. Chunk size trades draw count against false visibility and rebuild latency.
2. Group repeated static visuals by chunk × prototype × material × LOD. Use thin-instance buffers without per-building scene objects where reuse makes sense.
3. Thin instances are visible **all-or-none per parent mesh**; do not place every city object into one global instance group. They have insertion/removal costs; mutable pools must use updateable buffers, explicit updates and correct bounds.
4. Merge stable unique geometry only within bounded material/spatial chunks. Keep actively edited/selected objects in a small overlay, rebuild affected resources after changes, then swap safely. Do not turn the entire city into one immutable mesh.
5. Select LOD by projected screen size/zoom with hysteresis. Orthographic distance alone is inadequate. Do not assume automatic per-thin-instance LOD: assign compatible instances to explicit LOD batches.
6. Render moving people through pooled mutable batches. Update bulk transforms, not per-person UI components. Compare regular versus thin instances for heavy churn. Use simple animation/markers at distance; persistent people continue on the server.
7. Preserve stable entity → batch-slot mappings and repair them on compaction. Broad-phase picking uses spatial candidates, not every building each pointer move. GPU picking is optional and has readback latency.
8. Prefer opaque facades, baked/stylized windows, limited transparent surfaces, bounded shadow coverage, and modest postprocessing. Repeated transparent quads can be fill-rate-bound despite few polygons. Frustum culling does not remove every occluded tower.
9. Keep immutable data resident. Reuse typed arrays/buffers and upload dirty regions where supported. Account for simultaneous old/new buffers during rebuilds. Dispose replaced GPU resources explicitly.
10. Feature-detect and successfully initialize WebGPU; otherwise create a WebGL2 engine before scene construction. Keep core placement/selection on both paths; compute-only enhancements need another path. Handle context/device loss by rebuilding owned resources or clearly reconnecting/reloading. No silent loss of editing functionality.

Do not globally freeze active meshes or enable snapshot rendering merely because a static demo improves: camera movement and editing invalidate those assumptions. An engine optimization flag is not an architectural performance strategy.

## 8. Performance acceptance, not marketing numbers

**Every number below is a proposed target or research workload, not a measured result.** No universal “maximum polygons” or “maximum elements” exists. Vertex cost, fragment cost, materials, draw calls, animation, resolution, memory, CPU uploads and simulation are separate limits.

Define a target integrated-GPU laptop and an older WebGL2-capable minimum device before sign-off; record exact GPU/CPU/RAM, OS, browser/version, power mode, resolution/DPR and engine/build revision. The development machine alone is not a support matrix.

### Reproducible rendering workloads

- Repeated control: 10k, 50k and 100k instances across chunks. Diagnostic only, not a city-capacity claim.
- Heterogeneous downtown: 10k visible buildings, half from 100 prototypes and half unique; sweep toward all unique. Sweep source geometry around 200/1k/5k triangles and 8/32/128 material groups. Record actual submitted geometry after LOD and per pass.
- Moving citizens: 1k/10k/50k visual instances, multiple prototypes, all transforms active, and spawn/despawn churn. Count simulated residents separately.
- Editing: 1/10/100 asset replacements per second, ordinary selection/preview, terrain brushes, and a burst replacing 1% of visible buildings. Separate immediate preview feedback from final chunk-rebuild completion.
- Camera/fragment stress: pan across chunks, zoom quickly, densely occluded skyline, 1080p and higher DPR, shadows/water on/off, transparency stress, forced WebGL2 and device-loss recovery.

Use identical content/camera/material/effect/update streams for the Babylon/Three comparison. Warm up, repeat deterministic runs, separate cold shader/asset loading, and report p50/p95/p99 rather than average FPS alone.

### Proposed gates

| Scope | Proposed gate |
|---|---|
| Target desktop visual tier | p95 frame interval ≤16.7 ms; p99 ≤25 ms on the agreed representative workload |
| Minimum/fallback visual tier | p95 ≤33.3 ms at declared reduced visual detail; same editing semantics |
| Local interaction | p95 input-to-visible preview/selection <100 ms; no steady-state main-thread task >50 ms |
| Server base cadence | At 10 Hz, p95 simulation + commit ≤50 ms and p99 <100 ms, with no growing due-work backlog |
| Networked command | On controlled 100 ms RTT, p95 committed-result latency ≤300 ms at declared load; measure storage and scheduling separately |
| Scale experiments | Start at 1k residents; grow through 10k toward 100k, testing 2/8/32 editors separately; no claim these loads are already supportable |
| Resource stability | Repeated 10-minute edit/pan/zoom cycles have bounded retained CPU/GPU resources after disposal; set numeric memory budgets from measured device headroom |
| Correctness under load | No lost durable results, duplicate effects, invalid picks, missing chunks, stale-route commits or replica divergence |

Measure CPU render preparation, culling, picking, allocations/GC, GPU timestamps when available, draws/passes, material changes, upload bytes, estimated owned GPU resources, server system timings, path expansions/backlog, memory per resident, persistence latency, snapshot size and bytes/client. GPU and CPU overlap; do not add their timings naively. GPU buffer limits are not a VRAM budget, and universal accurate browser GPU-memory telemetry is not assumed.

If gates fail, identify the bottleneck and reduce unnecessary work or revise explicit supported capacity. Do not reduce simulation fidelity secretly, freeze an interactive workload to make it pass, or claim Wasm is the fix without an end-to-end comparison.

## 9. Milestone 1 implementation deviations

Recorded at integration of milestone 1 ("author a building and place it"); revisit when the deferred items return to scope.

- **Plain TypeScript DOM instead of React for panels.** The M1 web slices shipped without React to keep the vertical slice small; `src/main.ts` wires catalogue/designer/inspector directly. Reintroduce React (or another panel framework) when panel state genuinely outgrows manual DOM updates — the renderer stays framework-independent either way.
- **Hand-mirrored `src/protocol/types.ts` kept, ts-rs bindings not imported.** Generated `*.ts` exists (`apps/web/src/protocol/generated/`, byte-pinned by protocol tests) but the client uses the hand mirror: ts-rs emits `bigint` for u64/`WireId` where the client needs `number`-on-`JSON.parse` semantics and per-type files with imports. Keep the mirror and the wire tests in lockstep; swap only when the generated output is strictly drop-in.
- **Receipt of rejected-but-well-formed commands.** Rejected commands are receipted so a resubmission replays the same verdict instead of re-deciding; `duplicate_identity_conflict` is its own reject code rather than a `bad_request` variant.
- **Asset publish flows through the single-writer command queue** because id allocation and the one rusqlite connection are owned by the world runner.
- **Definition revision is constant 1 in M1** (definitions immutable; placements with another revision are rejected); schema gains a revision column when revisions become real.
- **Known cosmetic follow-up fixed at integration:** ESC now clears the catalogue card's armed highlight via `onDisarm` (previously the tool disarmed but the highlight persisted).
