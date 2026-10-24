# Research

Status: research supporting the [genesis](../GENESIS.md) and [architecture](architecture.md). Facts were verified by opening primary documentation where noted; performance targets and capacity numbers remain **unmeasured proposals** until the workloads in [architecture §8](architecture.md#8-performance-acceptance-not-marketing-numbers) run. The repository contained no code when this research was gathered.

Method: primary sources preferred (official manuals, developer diaries, engine documentation, papers). Every source carries a version caveat. Secondary reporting is used only for clearly attributed developer statements. Recommendations are separated from verified facts.

## 1. Reference games: mechanics lessons

Scope: what to adopt, adapt, postpone, or avoid. The table records evidence, not claims that reference games implement our design.

| Reference | Verified evidence | Adopt / adapt | Postpone / avoid |
|---|---|---|---|
| SimCity Classic (1989 family) | Housing growth depends on jobs, powered zones, access, quality of life; blackouts interrupt development and prolonged outages cause decline; traffic is generated zone-to-zone trips, not documented persistent commuters [G1] | Compact loop: build → connect → attract residents/jobs → earn → expand capacity | Inferring person-level simulation from animated traffic; importing its fixed zone ratios/tax/crime numbers as universal truths |
| SimCity 2013 (GlassBox era) | Contemporary report of a Maxis GDC talk: complexity was curtailed and some pedestrians teleport when numbers grow too large — an explicit compromise, not current-build proof [G3]; GlassBox links animations to agent activity (Sims, water, power as agents) [G11] | Keep cause-and-effect legible; simplify what creates no decision | Promising persistent personal lives while people behave as interchangeable tokens |
| Cities: Skylines I (2015 manual) | Citizens have age/education/health/happiness interacting with services and development; upkeep competes with expansion; service vehicles tie layout to function [G4] | Dashboards linked to buildings; staged services; visible operating costs | Copying all service categories at once; endgame monuments that delete constraints |
| Cities: Skylines II (2023 diaries vs Economy 2.0) | Lifepath diary: households, education, employment, illness, leisure, followed-citizen journal; pets cosmetic [G5]. Economy 2.0 changed demand, removed city-budget subsidies, made imports paid opt-in because hidden safeguards weakened player control [G6]; patch warns of transition demographics [G7] | Persistent people + household economics; explain why housing is wanted; make aid/imports explicit choices | Citing the 2023 economy as current; building an economy whose automatic corrections erase player-visible consequences |
| Dwarf Fortress | Persistent worlds retain history; dwarves have thoughts/needs supported by places (taverns, temples, libraries) [G8]; developer discussion of personality, long-term memories, cross-fort attachment, and admitted signposting/proportion problems [G9]; needs reduce/improve focus, replacing "on break" [G12] | Stable identity, few consequential preferences, remembered events, inspectable causes, meaningful places | Anatomy, exhaustive materials, world history, full breadth before the city is fun; treating historical bugs or roadmap items as current specs |
| Multi Player SimCity for X11 (1993) | Original announcement: several users edit the same city simultaneously with communication, annotations, and voting dialogs [G10] | Shared construction, plans/annotations, edit attribution, agreement only for disruptive actions | Separate-player economies, elections, mandatory roles as the starting point |

### Core lessons (condensed)

1. **First success is a complete causal loop** — place homes, connect power/water, named households move in, income funds the next constraint. Demand explains what is missing; it is not a colored bar.
2. **Identity before personality breadth** — name, household, home, job, short history from the first inhabited slice; only preferences that change decisions.
3. **Separate personal needs from household purchasing** — individuals need rest/health/social; households share costs and purchases; different situations produce different housing choices.
4. **Supply ≠ connection ≠ service received** — show production, connected capacity, and who lacks service as separate facts (Classic's power checks; Skylines I production vs operation).
5. **Grow depth by connecting new rules to existing consequences** — e.g. contaminated water → illness → missed work → lower income → unaffordable housing → relocation; each hop reuses existing entities. Twenty independent meters are not depth.
6. **Explainability is part of each mechanic** — every inspector answers what is wrong, why, since when, what can change; story feeds report real events.
7. **Assistance is explicit** — labeled grants/imports/modes with costs, never silent rebalancing (Economy 2.0's stated reason for removing subsidies).
8. **Emotions need proportion and recovery** — separate transient discomfort from major remembered events; small nuisances must not outweigh bereavement (DF talk's own caution).
9. **Player-authored buildings are places, not decoration** — capacity, access, costs, service requirements; terrain decides reachability and supply.
10. **Cooperation starts with complementary action** — one builds housing while another extends utilities; shared markers and attribution early; agreement for demolition/major spending.
11. **Add friction only when it buys a choice** — simplify travel/production when detail changes no decision or story; keep resource and identity rules honest.

### Ordered mechanics progression

1. **First inhabited block** — terrain prep, one editable home, access, one power and one water source, connectivity/capacity, construction/upkeep costs, transparent starter income, persistent named households. Place → connect → move in → earn → expand; two players contribute to the same block.
2. **Reasons to live here** — workplace, wages, affordability, one consumption category, explicit vacancies and demand reasons; persistent home/work even with simple travel.
3. **Layout trade-offs** — upkeep/upgrades, limited utility capacity, pollution or contamination, one health response, accessibility costs; density and cheap infrastructure get legible downsides.
4. **Residents become people** — few traits, work/rest/leisure behavior, relationships, significant memories, person inspector, event history.
5. **Neighborhoods and generations** — education, skill-linked work, household composition change, aging, births/deaths, moves, neighborhood identity.
6. **Selective deeper systems** — richer goods chains, transit, disruptions, environment, cooperative governance — only after earlier interactions are fun, each with an explanation view.

### Known failure modes to design against

Opaque demand/unexplained decline (CS2 Economy 2.0 admission); invisible assistance making decisions irrelevant; names promising lives while citizens stay disposable tokens; cascading failures that are arbitrary or emotionally disproportionate; synchronized demographic shocks from balance changes (CS2 patch warning); losing the player's personal imprint to automatic simulation (Wright/Hopkins on SimEarth); cooperative-editing social friction (X11's voting/annotations existed for a reason).

## 2. Rendering: technology decision

Question: which browser stack renders the most elements while staying maintainable?

### Decision matrix

| Option | Strengths | Costs / limits | Verdict |
|---|---|---|---|
| TypeScript + Babylon.js | Integrated picking/instancing/LOD/instrumentation; thin instances avoid a scene object per repeated item; WebGPU + WebGL2 maintained together | Regular instances still traverse per-object; thin instances are all-or-none visible per parent mesh, insertion/removal is costly, buffers static by default [R1]; freezing/snapshot optimizations invalidate moving-camera/edit assumptions [R2] | **Chosen start.** Best infrastructure-per-line-of-owned-code; benchmark, never claim fastest |
| TypeScript + Three.js | `InstancedMesh` for shared geometry; `BatchedMesh` explicitly renders many different geometries sharing a material with per-object frustum culling and sorting [R3]; WebGPURenderer documents WebGL2 fallback | BatchedMesh still culls/sorts and manages capacity; geometry/material diversity and uploads cost work | **Strongest challenger.** Prefer it if the controlled heterogeneous benchmark (architecture §8) shows materially better frame/edit latency |
| Rust + wgpu + Wasm | Control over layout/submission/culling; wgpu supports native + browser backends | All engine facilities become project-owned; wgpu downlevel (WebGL2) can reject valid baseline WebGPU operations (`is_webgpu_compliant` exists precisely for this) [R4]; no GPU-capacity gain from the language | Do not start here; revisit only on a proven engine-level bottleneck |
| Sprite/pre-rendered isometric world | Simple geometry; cheap fixed views | Overlapping transparent quads become fill-rate/order-bound; user-created buildings need baking + invalidation; rotation/elevation/shadows complicate representation | Distant LOD/impostors only; never the primary representation for integral designers |

### WebAssembly answer

WebAssembly accelerates **CPU** code; it does not increase GPU triangle throughput, memory capacity, texture limits, or fill rate — shader execution is GPU work regardless of whether TypeScript or Rust submits it [R5, R6]. Wasm helps render more elements only when a measured CPU kernel (e.g. coarse-grained mesh generation/decimation over bulk buffers) is the actual bottleneck; per-object JS↔Wasm calls and buffer copies can erase the gain. GPU limits (textures, buffers, bindings, compute workgroups) are per-feature ceilings, not a universal building count or a VRAM budget [R7]. **No whole-renderer Wasm migration is justified by current evidence.**

### No defensible universal maximum exists

A million repeated tiny opaque props, a thousand animated individuals, and a thousand overlapping glass towers are different workloads. Publish only measured envelopes: visible instances, unique geometries, material groups, submitted triangles per pass, resolution, draw calls, upload bytes, animation, shadows, camera path, hardware, browser, backend, frame-time percentiles. CPU and GPU overlap; their times do not sum.

### Verified capability notes (primary docs)

- Thin instances: thousands of instances without per-instance objects, but all-or-none visibility per parent mesh, costlier add/remove, static-by-default buffers (explicit `static=false` + `thinInstanceBufferUpdated`), single bounding info for collision, and broken rendering for mixed-determinant matrices [R1].
- `BatchedMesh`: constructor-bounded instance/vertex/index capacity, `addGeometry`/`addInstance`, `perObjectFrustumCulled`, `sortObjects` (front-to-back opaque, back-to-front transparent), explicit bounding computation, `optimize()` repacking [R3].
- WebGPU: adapter/device creation required (secure context), compute pipelines, per-device limits; browser/OS/GPU support remains uneven — feature-detect and attempt real initialization rather than trusting name/version checks; keep a WebGL2 tier [R5, R8].
- Material/world-matrix freezing and render-bundle/snapshot caching trade automatic updates for speed; unsafe as blanket switches for an editable city [R2].

## 3. Simulation and multiplayer architecture

Full decisions in [architecture §3–§5](architecture.md#3-domain-representation). Key evidence:

- **Server-authoritative replication over lockstep**: lockstep requires identical deterministic simulation on all replicas and stalls on missing input [M2]; command ordering needs no client-side simulation, so a disconnected editor cannot stall the world.
- **CRDTs are the wrong tool for authoritative city state**: convergence guarantees merged representation, not city invariants — two overlapping placements or two spends of the same funds do not become valid by converging [M3]. Ordered transactional commands enforce the invariants. CRDTs could serve mergeable draft documents later.
- **Command lifecycle**: envelope (world/command IDs, expected revisions) → gateway limits → single world owner assigns order/tick boundary (never client timestamps) → durable idempotency receipts scoped by world+actor → atomic validation and application (first valid placement wins) → single persistence commit (state + tick + receipts) → only then publish results. Receipts survive reconnect/restart; expiry needs an explicit epoch contract, not silent forgetting.
- **Storage choice**: typed dense domain stores first; standalone `bevy_ecs` documented as usable if heterogeneous components/cross-cutting queries dominate — table storage iterates fast but inserts/removes slower, sparse sets invert that [M1]. No full engine application on the server.
- **Backpressure is application-level**: Tokio bounded channels bound message count, not payload bytes [M4]; a socket accepting writes proves nothing about browser application. Require processed-sequence acks, bounded in-flight windows, resync on delta gaps; never drop a middle delta; presence/cursor packets may drop independently.
- **Persistence**: SQLite WAL allows concurrent readers, one writer; not for network filesystems; `synchronous=FULL` when "committed" means durable; use online backup APIs, never copy a live main file alone [M6, M7]. Persist changed domain records + tick + receipts transactionally; a command-only log does not reconstruct autonomous simulation state unless determinism is actually proven.
- **Simulation time**: integer ticks independent of frame/transmission cadence (10 Hz starting hypothesis); multi-rate needs via due-time queues; stagger deterministically; never skip consequential thresholds; overload slows/pauses explicitly rather than skipping economically meaningful ticks; camera/viewer presence never changes outcomes.
- **Routing**: bounded A* on the actual graph, cached routes, topology-version invalidation; HPA* (clustered abstraction, cached crossings, local repair) is the grounded later optimization for measured long-route cost — it does not solve congestion [M10]. Parallel route proposals commit only if person/intent/topology versions still match.
- **Determinism honesty**: Rust + fixed timestep do not guarantee replay; floating point, iteration order, parallel reductions, RNG call order, and content changes break it. Promise at most a tested, version-scoped replay contract.
- **Protocol**: Rust DTOs via Serde JSON, TypeScript generated with ts-rs [M8, M9]; wire/save/content versions separate; runtime validation on both sides; decimal-string fields for 64-bit IDs/sequences because JSON numbers cannot carry arbitrary u64 and a `bigint` annotation alone does not transform wire values.

## 4. Integrated assets: buildings and terrain

- **Shape grammar precedent**: CityEngine's CGA models buildings by iterative rule refinement — extrude footprint, component-split facades, split floors, repeat tiles, insert window/door assets, project textures with `setupProjection`/`projectUV`; attributes exposed for editor editing without code changes [A1]. This validates declarative bounded recipes (not arbitrary scripts) as a proven authoring model for our first compiler.
- **Interchange**: glTF 2.0 is the ISO-standard runtime delivery format (single-file GLB, PBR materials, instancing support) [A2]; KTX 2.0 + Basis Universal supercompression transcodes to GPU formats at runtime, cutting download and GPU memory, at the cost of a transcode/toolchain step [A3]. Use for interchange and authored imports; the editable recipe remains source of truth.
- **Terrain**: geometry clipmaps treat terrain as nested regular grids with incremental updates — a useful precedent for chunked heightfields; full GPU clipmaps are not a first-slice requirement [A4]. Initial model: bounded heightfield chunks, surface materials, authoritative buildability; height edits overlapping occupied footprints are rejected.
- **Pathfinding basis**: A*/BFS/Dijkstra family semantics (graph abstraction, costs, heuristics, early exit, distance fields) [A5]; HPA* for hierarchical caching when long-route search is measured as a bottleneck [M10].

## 5. Sources

Game mechanics:

- [G1] SimCity manual — "Inside the Simulator" (Don Hopkins Unix archive): https://www.donhopkins.com/home/catalog/simcity/manual/inside.html — Classic-era edition; numbers are edition-specific.
- [G2] Will Wright UI talk summary (Don Hopkins): https://www.donhopkins.com/home/catalog/simcity/WillWright.html — historical summary with porter's elaborations; not a verbatim transcript.
- [G3] Polygon, "SimCity pedestrians 'teleport' to keep the game fun" (2013, Moskowitz GDC report): https://www.polygon.com/2013/3/29/4161422/simcity-pedestrians-teleport/ — sole secondary source in the game set; used for attributed 2013 statements only.
- [G4] Cities: Skylines I official manual (Steam): https://store.steampowered.com/manual/255710 — base-game era.
- [G5] CS2 Feature Highlight #11, Citizen Simulation & Lifepath (2023-08-28): https://www.paradoxinteractive.com/games/cities-skylines-ii/features/citizen-simulation-lifepath — pre-release design description.
- [G6] CS2 Dev Diary: Economy 2.0 Part 1 (2024-06-03): https://www.paradoxinteractive.com/games/cities-skylines-ii/news/dev-diary-economy-part-one — documents that redesign, not latest balance.
- [G7] CS2 "Economy 2.0 is here!" release notice: https://www.paradoxinteractive.com/games/cities-skylines-ii/news/economy-patch-is-here — full forum patch notes not inspected (challenge wall); no build-number claims.
- [G8] Dwarf Fortress official features: https://www.bay12games.com/dwarves/features.html — unversioned page; broad concepts only; roadmap ≠ shipped.
- [G9] Dwarf Fortress Talk #25 transcript: https://www.bay12games.com/media/df_talk_25_transcript.html — historical, pre-Steam; community-produced transcript.
- [G10] Multi Player SimCity for X11 announcement (1993): https://www.donhopkins.com/home/catalog/simcity/simcity-announcement.html — same-city editing evidence; no modern usability claims.
- [G11] GlassBox agent overview (Wikipedia/GDC-derived summary): https://en.wikipedia.org/wiki/SimCity_(2013_video_game) — corroborates [G3]'s engine context.
- [G12] Dwarf Fortress Wiki "Need": http://dwarffortresswiki.org/index.php/Need — wiki; focus mechanics description, edition-dependent.

Rendering:

- [R1] Babylon.js Thin Instances (primary docs): https://doc.babylonjs.com/features/featuresDeepDive/mesh/copies/thinInstances/
- [R2] Babylon.js Optimizing Your Scene: https://doc.babylonjs.com/features/featuresDeepDive/scene/optimize_your_scene/
- [R3] Three.js BatchedMesh (primary docs): https://threejs.org/docs/pages/BatchedMesh.html ; InstancedMesh: https://threejs.org/docs/pages/InstancedMesh.html ; WebGPURenderer: https://threejs.org/docs/pages/WebGPURenderer.html
- [R4] wgpu DownlevelCapabilities (`is_webgpu_compliant`): https://docs.rs/wgpu/latest/wgpu/struct.DownlevelCapabilities.html
- [R5] MDN WebGPU API: https://developer.mozilla.org/en-US/docs/Web/API/WebGPU_API
- [R6] MDN WebAssembly concepts: https://developer.mozilla.org/en-US/docs/WebAssembly/Guides/Concepts
- [R7] MDN GPUSupportedLimits: https://developer.mozilla.org/en-US/docs/Web/API/GPUSupportedLimits
- [R8] MDN browser-compat-data GPU.json: https://github.com/mdn/browser-compat-data/blob/main/api/GPU.json

Simulation / multiplayer:

- [M1] bevy_ecs docs (standalone use; table vs sparse-set storage): https://docs.rs/bevy_ecs/latest/bevy_ecs/
- [M2] Gaffer On Games, Deterministic Lockstep: https://gafferongames.com/post/deterministic_lockstep/
- [M3] Shapiro et al., CRDTs (Inria repository): https://inria.hal.science/inria-00555588
- [M4] Tokio `sync::mpsc` (bounded backpressure): https://docs.rs/tokio/latest/tokio/sync/mpsc/
- [M5] Axum WebSocket extract: https://docs.rs/axum/latest/axum/extract/ws/index.html
- [M6] SQLite WAL: https://sqlite.org/wal.html — one writer; NFS caution; durability modes; WAL-reset fix in 3.51.3+ (backports 3.44.6/3.50.7) — verify the linked version.
- [M7] rusqlite (connections, transactions, online backup): https://docs.rs/rusqlite/latest/rusqlite/
- [M8] ts-rs (Rust→TypeScript; tagged enums; default `bigint` mapping for large integers): https://docs.rs/ts-rs/latest/ts_rs/
- [M9] serde_json: https://docs.rs/serde_json/latest/serde_json/
- [M10] Botea, Müller, Schaeffer — "Near Optimal Hierarchical Path-Finding (HPA*)": https://webdocs.cs.ualberta.ca/~mmueller/ps/2004/hpastar.pdf

Assets / terrain / routing:

- [A1] ArcGIS CityEngine Tutorial 6 — Basic shape grammar: https://doc.arcgis.com/en/cityengine/2021.0/tutorials/tutorial-6-basic-shape-grammar.htm — pinned 2021.0 version; live docs mutate.
- [A2] glTF 2.0 (Khronos; ISO/IEC 12113:2022): https://www.khronos.org/gltf/
- [A3] KTX / Basis Universal (Khronos): https://www.khronos.org/ktx/
- [A4] GPU Gems 2 ch. 2 — Geometry Clipmaps (Asirvatham & Hoppe): https://developer.nvidia.com/gpugems/gpugems2/part-i-geometric-complexity/chapter-2-terrain-rendering-using-gpu-based-geometry
- [A5] Red Blob Games — Introduction to the A* Algorithm: https://www.redblobgames.com/pathfinding/a-star/introduction.html

## 6. Open uncertainties

- Player count, target population, map size, tick cadence, durability latency, and offline-time policy are **product decisions not yet made**; they bound all budgets. Capacity claims are impossible before they exist.
- No benchmark has been run: all frame-time, element-count, and server-latency gates are proposals (architecture §8).
- Library documentation was read at current published versions; implementation must pin and re-verify compatible versions (notably SQLite's bundled vs system version).
- SimCity 2013's stable personal identity, modern CS2 patched behavior, and exact DF need/stress formulas were **not** established and are not asserted anywhere in this repository's documents.
