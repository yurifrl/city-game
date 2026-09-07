# Engineering constitution

## Primary objective

Deliver the best-organized, clearest, most maintainable code possible—not the largest amount of product functionality. Correctness and explicit ownership come first. Performance is a design constraint, not permission for opaque code. If a feature requires architectural shortcuts, reduce the slice rather than weaken the architecture.

Early playability still matters: build small, complete vertical slices that exercise real systems. Do not spend months building a speculative general-purpose engine. Well-organized means easy to understand and change, not many layers, directories, interfaces, or dependencies.

## Project identity

A browser-based cooperative city simulation. Multiple people edit the same authoritative map. Its differentiators are deep, interacting simulation systems and persistent individuals with lives and needs. Buildings, terrain, and their integrated designers are part of the engine, not a disconnected art pipeline.

Read [GENESIS.md](GENESIS.md) for product intent, [docs/architecture.md](docs/architecture.md) for technical decisions, [docs/research.md](docs/research.md) for evidence, and the current plan in `.agents/plans/` before changing architecture. Proposed paths in these documents are not claims that implementation exists.

## Organization rules

- Organize by responsibility and domain: world, construction, utilities, citizens, economy, transport, assets. Give each concept one owner and one source of truth.
- Keep the simulation independent of HTTP, sockets, databases, UI, graphics, and wall-clock time. Infrastructure depends on the domain; the domain never imports infrastructure.
- Separate authoritative state, network representations, client projections, authoring documents, and GPU resources. They have different owners and lifetimes.
- Start with cohesive modules. Extract a crate or package only for an enforced dependency boundary, independent build target, or demonstrated reuse. Do not create empty packages for future features.
- Prefer direct functions, explicit data, composition, and narrow APIs. Avoid inheritance trees, service locators, global mutable state, generic manager classes, and catch-all `utils`, `common`, or `shared` dumping grounds.
- Introduce an abstraction for a current boundary or repeated behavior, not an imagined future backend. Do not wrap libraries merely to rename their methods.
- Keep public surfaces small; private by default. Name types by domain meaning. Document units, coordinate conventions, ownership, ordering, and invalid states.
- Model states and failures explicitly. Do not hide failures behind defaults, successful-looking fallbacks, ignored results, or broad exception catches.
- Use one convention per problem. Migrate all callers when replacing a contract; remove obsolete paths rather than leaving compatibility shims without a real compatibility requirement.
- Keep changes focused. Update the relevant design decision when changing a boundary. Never reorganize unrelated code just to make a patch look cleaner.

## Simulation rules

- Rust owns authoritative game rules. Clients submit intent; clients cannot assign money, occupancy, population, utility availability, or simulation results.
- One logical writer owns each world. Explicit command ordering and tick phases govern all mutations, including solo play and administrative tools.
- Use stable domain identifiers. Array offsets, ECS entities, network connection IDs, and renderer instance indices are not durable citizen/building IDs.
- Persist individual identity and consequential history. Rendering a crowd marker must not turn the underlying people into anonymous population counters.
- Model only detail that creates a player-visible choice, consequence, explanation, or compelling story. Dwarf Fortress is a depth direction, not an immediate feature-parity requirement.
- Separate simulation time from frame time and network delivery time. Use bounded multi-rate updates; rendering fewer people must not change their needs or outcomes.
- Inject time and seeded randomness. Define deterministic ordering where replay requires it; do not claim cross-version or cross-platform bitwise determinism without proof.
- Make conservation, capacity, affordability, occupancy, connectivity, and lifecycle invariants explicit. A utility connection is not the same as sufficient utility supply.
- Expose causes, not just scores: a player must be able to inspect why a household cannot move in or why a building has no water.

## Performance rules

- Measure CPU time, GPU time where available, memory, draw calls, submitted geometry, upload volume, server tick latency, and network bandwidth separately. Record hardware, browser, viewport, build, workload, and percentiles.
- Do not claim an engine, language, WebGPU, or WebAssembly guarantees a particular object count. Distinguish visible objects, submitted instances, polygons, and simulated individuals.
- Use dense/data-oriented storage on hot paths, spatial partitioning, batching, instancing, shared materials, screen-size LOD, cached topology, and incremental updates where justified.
- Avoid avoidable per-frame/per-tick allocations, full-world scans, whole-world copies, per-agent tasks, repeated pathfinding, and per-object network messages. Profile before adding complicated optimizations.
- Keep the UI framework out of the per-entity render loop. A citizen or building is not a DOM element or UI component.
- Compile assets on authoring changes, not every frame. Treat draw calls, materials, textures, LODs, and bounds as part of asset quality.
- Preserve authoritative semantics when lowering visual quality. Do not silently stop simulation to maintain FPS.

## Assets and multiplayer rules

- Building and terrain designers use the same asset contracts and rendering pipeline as the game. No editor-only asset format that needs manual reconstruction to become playable.
- Keep editable source recipes distinct from compiled visual artifacts and server-validated gameplay definitions. Pin immutable published revisions in saved worlds.
- Share schemas/code-generated wire types rather than hand-maintaining Rust and TypeScript copies. Validate untrusted values at runtime; static types are not validation.
- Validate permissions, command preconditions, resource costs, asset bounds, and quotas on the server. Never execute uploaded author scripts or trust a client-defined building's capacity or cost.
- Handle concurrent placement, duplicate requests, reconnect, stale edits, and slow consumers explicitly. No silent last-writer-wins for destructive world edits.
- Solo and multiplayer use the same command path. Do not retrofit multiplayer onto a separate client-owned simulation.

## Verification and completion

- Define observable acceptance before implementation. Finish one usable vertical slice before starting another subsystem.
- Test behavior and domain invariants, not private wiring or incidental text. Prefer real domain/integration scenarios; mock only unavoidable external boundaries.
- Keep focused regressions for plausible bugs: overlapping edits, duplicate spending, disconnected utilities, insufficient supply, stale asset revisions, orphaned residents, and save/reload inconsistencies.
- Run the actual affected workflow. UI changes need browser evidence; multiplayer needs at least two clients; performance claims need reproducible measurements, not screenshots of a counter.
- Use the repository's formatter, linting, type checks, and focused tests when configured. Do not invent passing commands before tooling exists.
- Leave no stubs, fake data paths presented as real systems, dead code, abandoned experiments, or speculative scaffolding. Temporary benchmark probes may inform a decision; retain only useful reproducible workloads.
- Keep documentation synchronized with changed contracts. Report precisely what was verified and what remains an unmeasured target.
