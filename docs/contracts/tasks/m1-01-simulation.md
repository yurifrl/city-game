# m1-01 — Simulation domain core + protocol types

Own: `crates/simulation/**`, `crates/protocol/**`.

Read first: `docs/contracts/milestone-1.md` (normative), `AGENTS.md`, and the scaffold signatures in `crates/simulation/src/lib.rs` (keep that public surface stable — the server compiles against it).

## Change

- Fill every `todo!("m1-01")` in `crates/simulation/src/*` per the contracts:
  - `world.rs`: dense storage + cell→building index that always agrees with `buildings()`.
  - `assets.rs`: catalogue; `validate_recipe` enforces every rule (name length, dims 1..=8, floors 1..=40, hex colors, known archetype/style) and returns ALL violations; derives capacity/cost from the contracts table (integer math, exact).
  - `construction.rs`: `footprint` (odd rotations swap w/h), pure `validate_placement` (bounds, occupancy with conflicting id, definition exists), `apply_placement` (stores + updates index), `apply_demolition`.
  - `save.rs`: `to_save`/`from_save`; `from_save` rejects malformed records (unknown asset id, duplicate cells, rotation > 3, duplicate ids) with `Err(String)` — never silent defaults.
- `crates/protocol`: make it compile and keep the documented JSON shape exactly. `cargo test -p city-protocol` must export ts-rs bindings into `apps/web/src/protocol/generated/`. If the pinned `ts-rs` feature names or attribute spellings need adjusting, adjust them — the JSON shape (tagged enums, snake_case, `WireId` strings) is the contract, not the attribute syntax. Remove any stray attributes (e.g. `optional_fields` where nothing is optional).

## Tests (required)

Inline `#[cfg(test)]` or `tests/`: overlap rejection carries the correct conflicting id; rotation swaps footprint dims; out-of-bounds rejected; `validate_recipe` matrix incl. the three seed recipes from the contracts (exact derived values); save→load roundtrip preserves tick, both id counters, catalogue, buildings; occupant index consistency (no two buildings share any cell after a sequence of placements/demolitions).

## Acceptance

`cargo check -p city-simulation -p city-protocol` green; `cargo test -p city-simulation -p city-protocol` green. No `todo!()` left in owned files. Do NOT touch `crates/server` or `apps/web`.

## Constraints

Targeted cargo commands only (always with `-p`), never workspace-wide build/test. Version-pin fixes allowed in your own Cargo.toml; record every deviation from the contract in your final report.
