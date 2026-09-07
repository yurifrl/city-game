# m1-03 — Web core (engine, replica, tools)

Own: `apps/web/**` EXCEPT `src/editors/**` and `src/assets/**` (stub signatures there must keep compiling — you may import and call them; their bodies arrive via m1-04).

Read first: `docs/contracts/milestone-1.md` (§Web app — module layout and data flow), `AGENTS.md`.

## Change

Implement all modules listed in the contracts §Web app and `src/main.ts` as the composition root:

- `src/protocol/types.ts` mirroring every wire type exactly (field names from the contracts).
- `connection/client.ts`: WS to `/ws` (dev proxy exists), reconnect backoff 1s→10s, pending map by `command_id`, dispatch hello/event/command_result.
- `world/replica.ts`: definitions + buildings + cell→building index, `footprintCells(defId, cell, rotation)` (odd rotation swaps dims), `nextCommandId()`.
- `renderer/engine.ts`: Babylon engine — WebGPU when `navigator.gpu` exists and engine creation succeeds, else WebGL2; show backend in `#backend-chip`.
- `renderer/camera.ts`: orthographic oblique (alpha -45°, ~35.264° elevation), wheel zoom via ortho scale, drag pan.
- `renderer/grid.ts`: 96×96 grid lines + hover cell highlight.
- `renderer/buildings.ts`: thin-instance groups keyed `${defId}@${revision}` with `registerPrototype` / `setInstances` / `removeEntity` / entity↔id map for picking.
- `renderer/ghost.ts`: preview mesh with green/red validity tint.
- `tools/placement.ts`: arm from catalogue, ghost snapping, R rotate, ESC cancel, click → command, pending until result; rejection → visible toast with the server message.
- `inspectors/building.ts`: click building → panel with name/id/cell/rotation/capacity/cost/archetype.
- `main.ts` wires everything per the contracts data-flow paragraph, including calling `mountCatalogue`/`mountDesigner` with the documented `DesignerDeps`.

UI copy/styling via existing `#panel`/`styles.css` structure; no new frameworks or UI deps.

## Acceptance

`cd apps/web && npm install && npm run build` green (tsc strict + vite build). Keep `src/editors/designer.ts` and `src/assets/compiler.ts` stub signatures intact. List in your report the exact manual browser steps the integrator should run (backend chip, seed catalogue, place, overlap rejection, inspect, reload).

## Constraints

Targeted checks only (`npm run build` inside `apps/web`). No workspace cargo. No React/framework additions (deferral is a recorded deviation from docs/architecture.md — note it in your report). Do not edit the two stub files' signatures.
