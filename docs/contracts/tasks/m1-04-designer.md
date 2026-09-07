# m1-04 — Building designer + asset compiler

Own: `apps/web/src/editors/**` and `apps/web/src/assets/**` only (replace both scaffold stubs entirely). You may read everything; edit nothing else.

Read first: `docs/contracts/milestone-1.md` (§Web app — exact TS signatures for `compiler.ts` and `designer.ts`), `AGENTS.md` (assets are part of the engine, compiled not hand-drawn).

## Change

- `src/assets/compiler.ts`: deterministic `compilePrototype(def, scene)` from `(recipe, seed)` — box mass `footprint × floors×3m` tall, window band facade drawn on a canvas `DynamicTexture` (grid or stripes per `window_style`, `base_color` walls, `roof_color` flat slab + parapet), seed-driven rooftop box + hue jitter; merge into ONE mesh per definition. Budget ≤ ~600 triangles/prototype, 1–2 materials. No per-window meshes. Facades must stay readable zoomed out (crisp window grid, dark edge definition — reference: dense illustrated skyline, muted reds/blues/tans).
- `src/editors/designer.ts`: designer form (name, archetype select, footprint w/h, floors, base/roof color, window style, seed) → preview recipe JSON → `deps.publishRecipe` (POST /api/assets) → show derived capacity/cost or the server's error list; `deps.armPlacement` on success. Catalogue: `mountCatalogue` lists definitions (click → `onArm`), `upsertCatalogueEntry` updates in place. Match the contracts' exported signatures exactly — m1-03's `main.ts` already calls them.

## Acceptance

`cd apps/web && npm install && npm run build` green. Determinism: compiling the same definition twice produces identical geometry (note how you verified, e.g. deterministic vertex hash in a scratch check you then removed). Report the manual browser steps the integrator should run (publish → appears in catalogue with derived stats → place → looks consistent).

## Constraints

Targeted checks only (`npm run build` inside `apps/web`). No new npm deps. You depend on `src/protocol/types.ts` (created by m1-03 on main) — in your worktree it does not exist yet, so create it locally ONLY to typecheck if missing, but DO NOT commit it (main owns it); note in your report that you did this.
