# MCP-driven Building Studio + pixel-art engine direction (Milestone 2)

Agents author buildings by calling MCP tools; humans sculpt the same blueprint in a 3D builder panel; the server validates both through one command path; the whole game renders in a mid-2000s Habbo-style pixel-art isometric look (reference: the pixel-city stills + Habbo Hotel).

## Context

- M1 shipped the full vertical slice: Rust server (receipted single-writer commands, WS replication, SQLite), Babylon client, asset pipeline where a `Recipe` compiles deterministically into one box-mass prototype.
- The current "designer" is a form with 7 procedural fields. This milestone replaces that authoring model with an explicit **Blueprint** (a parts list), adds an **MCP server** so agents are first-class builders, and re-skins rendering to the target art style.
- MCP: official `rust-mcp-sdk` (RMCP) implements the 2026-07-28 spec with axum integration and Streamable HTTP; stdio available for local agents. Both Rust and TS SDKs are spec-complete; we stay in Rust so validation and id allocation live with the domain (no second authority, no shim client).

## Art direction (binding for all rendering)

- Isometric 2:1 dimetric, chunky proportions, crisp dark outlines (~2px at native res).
- Flat 3-tone shading per face (light/mid/dark from one hue) — no gradients, no smooth specular.
- Warm mid-2000s palette: terracotta, brick red, tan, cream; muted teal/blue glass; deep blue-grey shadows (not pure black). Fixed palette defined once, owned by the domain (`PALETTE` in `crates/simulation::assets::style`), referenced by atlas generation, preview renderer, and scene clear/fog colors.
- Windows/doors are pixel patterns painted into the facade atlas, not geometry. Rooftop clutter (AC, antenna, water tank) for silhouettes.
- Implementation: render scene to a low-res target (÷2–÷3) with nearest-neighbor upscale; inverted-hull outlines on prototypes; atlas pixels clamped to palette.

## Engine: Blueprint asset pipeline

- `Blueprint` = `{ name, archetype, footprint_w ≤ 8, footprint_h ≤ 8, parts: Vec<Part>, seed }`.
- `Part` = `{ kind, x, y, z, w, h, d, color/pattern, facing }`; kinds v1: `mass`, `window_band`, `window`, `door`, `roof_flat` (parapet), `roof_gable`, `awning`, `sign`, `ac_unit`, `antenna`, `planter`.
- Budgets (server-enforced): ≤ 64 parts, ≤ 300 compiled triangles, height ≤ 120 m, colors from palette (with bounded jitter allowed at compile time).
- Validation rules (domain, pure): parts inside footprint bounds and height; masses must not interpenetrate (touching/stacking OK); `door` on ground level facing outward; `roof_*` only as topmost layer; `awning/sign/window` must sit on a mass face; `sign` ≤ 2 per building.
- Derived stats: keep the M1 formula (capacity from enclosed cell-floors × density, cost = cell-floors × cost-per-cell). Part-based cost surcharge is deferred (recorded in deviations when relevant).
- Clean cutover: `Recipe` (7 procedural fields) is **replaced** by `Blueprint`; the compiler lowers a Blueprint equivalently for the 3 seed recipes; save schema_version 1 → 2 with a one-shot migration; no dual-format shims.

## MCP studio (new endpoint in `city-server`)

- `POST|GET /mcp` Streamable HTTP (RMCP + axum), plus `city-server --mcp-stdio` for local agent subprocesses.
- Tools: `studio_schema()` (part vocabulary, budgets, rules — the agent's manual), `blueprint_create`, `blueprint_get`, `blueprint_list`, `part_add`, `part_update`, `part_remove`, `blueprint_preview(id) -> PNG image`, `blueprint_publish(id) -> AssetDefinition`.
- Authority: every mutation is a receipted command on the existing single-writer path (same ordering/durability as WS clients); server validates parts before storing; publish assigns the durable `AssetId` and revision.
- Every committed blueprint change broadcasts `blueprint_updated` to WS clients — the human studio panel live-mirrors agent edits and vice versa.
- `blueprint_preview` returns a PNG rendered by the new preview renderer, so any MCP client (not just browser-driving agents) can see its work.

## Preview renderer: `crates/pixel`

- Deterministic software isometric rasterizer: Blueprint → PNG at 2:1 dimetric, fixed palette, 3-tone face shading, dark outlines, rooftop clutter glyphs. One focused module + unit tests with golden-image hashes (locks the style in CI).
- Reused by: MCP `blueprint_preview`, studio panel (fallback thumbnail), and as the style reference the Babylon atlas must match.

## Studio UI (web panel, no React)

- Studio panel replaces the M1 form: blueprint part list, add/edit/remove forms per part kind, live iso preview canvas rendering the in-progress blueprint through the same compiled prototype path, publish button.
- Human edits go through the same WS command path as MCP — agent and human edits interleave by server arrival order, both receipted.
- Pixel-art scene pass applies to game + studio: low-res render target + nearest upscale, palette atlas, outline hull.

## Steps

1. **Domain**: `crates/simulation` gains `blueprint.rs` (types, validation, budgets, derivation); `Recipe` → `Blueprint` cutover; compiler lowers seeds identically; save v1→v2 migration; targeted tests.
2. **Server**: blueprint CRUD as receipted commands (WS + HTTP), `blueprints` table (schema v2), `blueprint_updated` broadcast; publish → definition revision; tests (receipts, budget reject, concurrent human+agent edits ordered).
3. **Pixel renderer**: `crates/pixel` rasterizer + golden-image tests; palette module lives in domain (`assets::style`).
4. **MCP endpoint**: RMCP server on `/mcp` + stdio flag; tools wired to the command path; conformance smoke + end-to-end proof: I author "Corner Shop" purely via MCP tools, publish, place it in-game, screenshot.
5. **Studio panel**: web UI for part editing with live preview (WS-driven), publish into catalogue; manual browser pass.
6. **Pixel render pass in-game**: low-res target + upscale + outlines + palette atlas on prototypes; verify game view and preview PNG are recognizably the same building.
7. **Content + style proof**: rebuild 3 seeds, author 3–4 new buildings via MCP (corner shop, office block, apartment, tower with setbacks), place a small skyline, screenshot vs the reference images.

## Verification

- Domain: unit tests for every validation rule and the derivation formula (as M1).
- Renderer: golden-image hash tests (deterministic, no GPU).
- Server: receipt/replay tests for new commands; budget and rule rejection tests; interleaved human+agent edit ordering test.
- MCP: RMCP conformance smoke; end-to-end agent-authored building placed in-game (screenshots).
- Two-client proof: agent adds a part while the studio panel is open — panel updates live; simultaneous edits never lose a part (receipted order).
- Full DoD: `cargo build/test --workspace`, `npm run build`, browser pass with screenshots, restart persistence of blueprints.

## Out of scope

- Terrain sculpting studio (later; pipeline is designed to extend to it).
- Multi-user permissions, presence/cursors, concurrent-edit merge UI (single shared world ordering suffices for now).
- Citizens/animation visuals; 1 km² scale targets (reference images inspire style, not a perf gate); mobile.
