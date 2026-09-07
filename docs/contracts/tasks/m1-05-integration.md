# m1-05 — Integration and vertical-slice proof

Own: everything, on `main`, after m1-01..04 merged. Blocked by all four.

Read first: `docs/contracts/milestone-1.md` (§Merge order and integration, §Definition of done), `AGENTS.md` verification rules.

## Change

1. Resolve merge fallout (imports, signatures, types) — minimal diffs, keep each module's ownership.
2. Protocol bindings: try swapping `apps/web/src/protocol/types.ts` for ts-rs generated bindings; keep `types.ts` if not strictly drop-in better, and record the decision + reason.
3. Full verification from repo root: `cargo build --workspace && cargo test --workspace`; `cd apps/web && npm install && npm run build`.
4. Boot the server (`cargo run -p city-server` from repo root), then browser-verify the contracts' Definition of done: grid renders; catalogue lists 3 seed definitions; designer publishes a new recipe with derived stats; place ≥3 buildings; overlapping placement visibly rejected with the server message; demolish works; backend chip shows webgpu/webgl2; kill + restart server → identical building set after reload.
5. Fix what's broken within milestone scope; anything architectural → note it, don't redesign.
6. Record the React deferral and any other deviations in `docs/architecture.md` (small edit, one place).

## Acceptance

All builds/tests green on `main`; the browser pass above actually performed with results reported (screenshots where possible); server restart persistence proven.
