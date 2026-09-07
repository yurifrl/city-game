# Milestone 1 contracts — normative

Milestone 1 = plan step 1 ("Author a building and place it"): one client, integrated building designer, asset catalogue, placement with the authoritative command path, automatic persistence, reload.

Precedence: this file > task briefs (`tk show <id>` plan) > implementer preference. AGENTS.md engineering rules always apply.

## Owned paths (edit ONLY these)

| Task | Owns |
|---|---|
| m1-01 simulation | `crates/simulation/**`, `crates/protocol/**` (may adjust ts-rs attributes; wire JSON shape below is fixed) |
| m1-02 server | `crates/server/**` |
| m1-03 web core | `apps/web/**` EXCEPT `src/editors/**` and `src/assets/**` |
| m1-04 designer | `apps/web/src/editors/**`, `apps/web/src/assets/**` (replace the stub files entirely) |
| m1-05 integration | everything, on the integration branch, after merges |

Never edit another task's paths. If a contract is wrong or blocked: implement the closest sane version, record the deviation in your final report. Never leave `todo!()`/`unimplemented!()` in files you own when you finish.

## Shared conventions

- Grid: `GRID_WIDTH = GRID_HEIGHT = 96` cells, origin top-left, `x` east (right), `y` south (down), cell = 1 m.
- Domain IDs are `u64`, allocated by the server only. **Wire IDs are decimal strings** (`WireId = String`) because JS numbers are unsafe above 2^53.
- Rotation: `u8` 0..=3, quarter turns clockwise. Odd rotations swap footprint width/height.
- Footprint: `w × h` cells with `1 ≤ w,h ≤ 8`, anchored at the minimum cell.
- Tick: persisted counter; M1 advances nothing autonomously (no scheduled systems yet).
- One writer per world; every mutation goes through the command path (solo = single client, same path).

## crates/protocol — shared pure-data crate (normative)

`crates/protocol/src/lib.rs` in the scaffold is the complete type contract; implementers may not rename/restructure the JSON shape. It must stay a pure data crate: serde + ts-rs only, no tokio/axum/sqlite. `simulation` may use these types (shared data, not infrastructure); server adapters convert `WireId` strings to `u64` at the boundary.

Wire JSON (text frames over WS, JSON bodies over HTTP), `snake_case`, tagged with `"type"` (command result uses `"result"`):

```json
// client → server
{"type":"command","command":{"type":"place_building","command_id":"c1","def_id":"1","revision":1,"cell":{"x":10,"y":12},"rotation":0}}
{"type":"command","command":{"type":"demolish_building","command_id":"c2","building_id":"7"}}

// server → client (on connect)
{"type":"hello","actor":"owner","world":{"grid":{"width":96,"height":96},"tick":0,
 "buildings":[{"id":"1","def_id":"1","revision":1,"cell":{"x":10,"y":12},"rotation":0,"derived":{"capacity":2,"cost_credits":2160}}],
 "definitions":[{"id":"1","revision":1,"recipe":{...},"derived":{"capacity":2,"cost_credits":2160}}]}}

// server → client (after each committed command, broadcast to ALL connections)
{"type":"event","event":{"type":"building_placed","building":{...as above...}}}
{"type":"event","event":{"type":"building_demolished","building_id":"7"}}
{"type":"event","event":{"type":"definition_published","definition":{...}}}

// server → client (reply to the issuing connection only)
{"type":"command_result","result":{"result":"accepted","command_id":"c1","building_id":"7"}}
{"type":"command_result","result":{"result":"rejected","command_id":"c1","code":"overlap","message":"footprint overlaps building 3","conflicting_building_id":"3"}}
```

Reject codes: `overlap`, `out_of_bounds`, `unknown_asset`, `unknown_building`, `duplicate_identity_conflict`, `bad_request`.

Recipe JSON (`city_protocol::Recipe`), fields fixed:

```json
{"name":"Cedar Walk-up","archetype":"house","footprint_w":3,"footprint_h":2,"floors":3,
 "base_color":"#b5654f","roof_color":"#3a3a3a","window_style":"grid","seed":11}
```

`archetype ∈ house|shop|office|tower`, `window_style ∈ grid|stripes`. Validation: name 1..=64 chars; footprint 1..=8; floors 1..=40; colors match `^#[0-9a-fA-F]{6}$`.

## Asset derivation (server-authoritative, m1-01 implements, m1-02 uses)

| Archetype | capacity density (persons / cell·floor) | cost per cell·floor (credits) |
|---|---|---|
| house | 0.15 | 120 |
| shop | 0.10 | 180 |
| office | 0.30 | 260 |
| tower | 0.40 | 340 |

`capacity = floor(w·h·floors·density)`, `cost_credits = w·h·floors·cost_per_cell` (integer math). The client never supplies capacity or cost. `validate_recipe` returns all validation errors (not just the first).

Seed catalogue (m1-02 inserts exactly these on an empty database, ids 1,2,3):

1. "Cedar Walk-up" — house, 3×2, 3 floors, base `#b5654f`, roof `#3a3a3a`, grid, seed 11
2. "Harbor Point Office" — office, 3×3, 8 floors, base `#7f9bb3`, roof `#2f2f38`, stripes, seed 22
3. "Meridian Tower" — tower, 2×2, 24 floors, base `#9fb8c8`, roof `#22242c`, grid, seed 33

## Server endpoints (m1-02)

Run from repo root: binds `127.0.0.1:8080`, persists to `./data/city.db`, serves `./apps/web/dist` (fallback to `index.html`).

- `GET /` — static files + SPA fallback
- `GET /api/assets` → `{"definitions":[...]}` (Definition JSON as above)
- `POST /api/assets` body = Recipe JSON → `200` full AssetDefinition, or `400` `{"errors":["..."]}`; also persists, assigns id, and broadcasts `definition_published`
- `GET /ws` — WebSocket upgrade

Command lifecycle: bounded ingress queue → single world owner thread assigns order (never client timestamps) → receipt lookup (same `command_id` + identical canonical payload ⇒ replay stored result; same id different payload ⇒ `duplicate_identity_conflict`) → validate+apply → single SQLite transaction (mutation + meta + receipt) → only then broadcast events and reply `command_result`. Bad JSON/unknown shape ⇒ `bad_request` result to that socket, socket stays open. Malformed commands are never broadcast as events.

## Persistence schema (m1-02; m1-01 provides `save::WorldSave` mapping)

`PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;` one owned connection on the world-runner thread.

```sql
CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
-- meta rows: schema_version=1, tick, next_building_id, next_asset_id
CREATE TABLE IF NOT EXISTS assets (
  id INTEGER PRIMARY KEY, recipe_json TEXT NOT NULL,
  capacity INTEGER NOT NULL, cost_credits INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS buildings (
  id INTEGER PRIMARY KEY, def_id INTEGER NOT NULL, revision INTEGER NOT NULL,
  cell_x INTEGER NOT NULL, cell_y INTEGER NOT NULL, rotation INTEGER NOT NULL,
  command_id TEXT NOT NULL UNIQUE);
CREATE TABLE IF NOT EXISTS receipts (
  command_id TEXT PRIMARY KEY, payload TEXT NOT NULL, outcome_json TEXT NOT NULL);
```

Boot: load tables → `simulation::save::from_save` → serve `hello` from that state. Every accepted command commits in one transaction before any broadcast.

## crates/simulation public API

`crates/simulation/src/lib.rs` in the scaffold is the normative public surface (module layout, type names, function signatures). m1-01 fills bodies, keeps the public API stable, and owns the internals (may add private helpers/types). If a signature must change, note it as a deviation — m1-02 compiles against the scaffold signatures.

Required domain tests (m1-01): overlap rejected with the correct `conflicting` id; rotation swaps footprint dims; out-of-bounds rejected; `validate_recipe` covers every rule and derives the table values exactly (use the seed recipes as fixtures); save→load roundtrip preserves tick, id counters, catalogue, buildings; occupant index agrees with buildings at all times (property: no two buildings share a cell).

## Web app (m1-03 core, m1-04 designer)

Plain TypeScript + DOM (no framework this milestone; React deferred — record in report). Vite dev proxy already targets `127.0.0.1:8080`.

Module layout (m1-03 creates; m1-04 only owns `editors/` and `assets/`):

```
src/main.ts              composition root: builds every piece, wires deps below
src/styles.css           dark UI, right panel 320px, canvas fullscreen, backend chip
src/protocol/types.ts    TS mirrors of every wire type in this file (field names exact)
src/connection/client.ts WS connect/reconnect(1s→10s backoff), pending{command_id→resolve},
                         send(command), onMessage dispatch, snapshot application
src/world/replica.ts     definitions map, buildings map, cell→buildingId index,
                         footprintCells(defId,cell,rotation): {x,y}[], nextCommandId()
src/renderer/engine.ts   Babylon init: WebGPU when navigator.gpu and engine creation
                         succeeds, else WebGL2; backend chip in DOM
src/renderer/camera.ts   orthographic oblique (alpha -45°, elevation ~35.264°),
                         wheel zoom (ortho scale), drag pan, keys optional
src/renderer/grid.ts     96×96 line grid, hover cell highlight
src/renderer/buildings.ts thin-instance groups keyed `${defId}@${revision}`;
                         registerPrototype(key, mesh), setInstances(key, entries[]),
                         removeEntity(id), idToEntity map for picking
src/renderer/ghost.ts    semi-transparent preview mesh, green/red validity tint
src/tools/placement.ts   catalogue click arms tool; ghost snaps to hovered cell;
                         R rotates; ESC cancels; click sends place command; pending
                         until result (rejection → visible toast with message);
                         client-side overlap → red ghost (server still decides)
src/inspectors/building.ts click building → panel: name, id, cell, rotation,
                         capacity, cost, archetype
```

Boundary files m1-04 implements (scaffold stubs exist; m1-03 calls them via `main.ts`):

```ts
// src/protocol/types.ts (m1-03) — shapes m1-03 and m1-04 share:
export interface CellTS { x: number; y: number }
export interface RecipeTS { name: string; archetype: "house"|"shop"|"office"|"tower";
  footprint_w: number; footprint_h: number; floors: number;
  base_color: string; roof_color: string; window_style: "grid"|"stripes"; seed: number }
export interface DerivedStatsTS { capacity: number; cost_credits: number }
export interface AssetDefinitionTS { id: string; revision: number; recipe: RecipeTS; derived: DerivedStatsTS }
export interface BuildingTS { id: string; def_id: string; revision: number; cell: CellTS; rotation: number; derived: DerivedStatsTS }

// src/assets/compiler.ts (m1-04; scaffold stub has the same signature):
export interface CompiledPrototype { mesh: import("@babylonjs/core").Mesh;
  triangleCount: number; materialCount: number }
export function compilePrototype(def: AssetDefinitionTS, scene: import("@babylonjs/core").Scene): CompiledPrototype
// Deterministic from (recipe, seed): box mass floors×3m tall, window band texture
// drawn on a canvas DynamicTexture (grid or stripes per window_style, base_color
// walls, roof_color flat slab + parapet, seed-driven small rooftop box + hue jitter),
// merged into ONE mesh per definition. Budget: ≤ ~600 triangles/prototype. No
// per-window meshes.

// src/editors/designer.ts (m1-04; scaffold stubs have these signatures):
export interface DesignerDeps {
  publishRecipe(recipe: RecipeTS): Promise<{ ok: boolean; definition?: AssetDefinitionTS; errors?: string[] }>;
  armPlacement(def: AssetDefinitionTS): void;
  refreshCatalogue(def: AssetDefinitionTS): void;
}
export function mountDesigner(container: HTMLElement, deps: DesignerDeps): void
export function mountCatalogue(container: HTMLElement, defs: AssetDefinitionTS[], onArm: (def: AssetDefinitionTS) => void): void
export function upsertCatalogueEntry(def: AssetDefinitionTS): void
// Designer form: name, archetype select, footprint w/h, floors, base/roof color,
// window style, seed (number). Shows derived capacity/cost after publish, or the
// server's error list. Publish = POST /api/assets.
```

`main.ts` flow: engine → camera → grid → buildings view → replica ← client.hello; on `definition_published`/hello.definitions: compile prototype + register + refresh catalogue; replica changes → recompute instances per key → `setInstances`; placement tool gets `armPlacement` from catalogue/designer.

## Merge order and integration (m1-05)

`m1-01 → m1-02 → m1-03 → m1-04`, then m1-05 on `main`: swap `src/protocol/types.ts` for ts-rs generated bindings if (and only if) they are drop-in and strictly better; otherwise keep `types.ts` and record why. Full `cargo build && cargo test` workspace, `npm run build`, boot server, full acceptance pass, fix what's broken within milestone scope.

## Definition of done (whole milestone, verified on main)

1. `cargo test` workspace green; `npm run build` green.
2. Browser: grid renders; catalogue lists the 3 seed definitions; designer publishes a new recipe with derived stats shown; place ≥3 buildings; overlapping placement visibly rejected with a message; demolish works; backend chip shows webgpu or webgl2.
3. Kill server, restart, reload page: identical building set (persistence proof).
4. TwoBrowser sanity (single connection is the M1 scope; second-client ordering lands in M2).
