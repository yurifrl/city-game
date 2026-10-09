---
name: city-asset-studio
description: >
  This skill should be used when creating, editing, or publishing game assets
  (buildings, props, structures — anything placed in the city) for the
  city-game project through the Asset Studio MCP server. Covers authoring
  parts via MCP tools, previewing, and publishing into the city catalogue.
---

# City Asset Studio

Author game assets by calling MCP tools on the Asset Studio; publish finished
scenes into the city game as placeable assets. Assets are not limited to
buildings — any composed object (shops, towers, props, lamp posts, kiosks,
trees) follows the same part vocabulary.

## Connection

- MCP endpoint: `http://localhost:8200/mcp` (Streamable HTTP).
- Both `city-game/.mcp.json` and the studio's own `.mcp.json` register it as
  `asset-studio`. If the tools are missing, check the studio process is
  running (`cd ../building-studio && bun run server.ts`) and that port 8200
  answers (`curl -s http://localhost:8200/api/scenes`).
- Start with the `studio_help` tool — it always reflects the live vocabulary.

## Workflow

1. `scene_new { name }` — one scene per asset draft (kebab-case name).
2. Read `studio_help` for the part kinds, grid, and palette.
3. Compose with `part_add { scene, kind, x, y, z, w, h, d, color, ... }`:
   - Coordinates are cells (1 cell = 1 m): x = east, z = south, y = up from
     the ground. Parts anchor at their min corner.
   - Build from big to small: main masses first, then roof, then details
     (doors, awnings, signs, rooftop clutter).
4. Inspect: `scene_snapshot { scene }` (exact JSON) and `preview_svg { scene }`
   (quick iso render). For the real look, open
   `http://localhost:8200/?scene=<name>` in the browser.
5. Iterate: `part_update { scene, id, ... }`, `part_remove { scene, id }`.
6. Publish into the city game with
   `city_publish { scene, name, archetype }`:
   - `archetype`: house | shop | office | tower (drives capacity and cost).
   - The city auto-derives footprint (ceil of part bounds, max 8×8), floors
     (ceil of max top / 3 m), colors (most common = walls, topmost = roof).
   - The city server validates everything; on rejection it returns the exact
     rule violations — fix and re-publish.
7. Verify in the city app (`http://localhost:8080/`): the asset appears in the
   catalogue with an iso icon. Clicking the card selects it for placement
   (then click the grid to place; R rotates, Esc cancels). Editing lives in
   the detail panel — open it via the card's ✎ button or double-click.

## Design recipes that read well

- **Setbacks**: stack successively smaller masses (e.g. 8×6 → 6×6 → 4×4).
- **Corner shop**: 6×4×3 mass + door + cream awning + gold sign + roof slab +
  AC unit + planters flanking the door.
- **Apartments**: brick mass + `roof_gable` cap + chimney + door + planters.
- **Towers**: slim mass stack + antenna (tall thin part) + water tank + crown
  tier in a contrasting color.
- Contrast facade color vs sign color; keep signs ≤ 2 per asset; put rooftop
  clutter inside the roof line so the silhouette stays clean.

## Rules

- The city catalogue is immutable-by-revision: publishing always creates a new
  definition id. Tweaks published again are new entries; remove stale ones via
  the city catalogue UI (Remove is refused while placed buildings exist).
- Never bypass the MCP tools by writing scene JSON directly — the tools are
  the validation and broadcast path.
- Grid limits: x/z within ±12 in the studio, ≤ 64 parts, y ≤ 48.
