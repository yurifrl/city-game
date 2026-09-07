# m1-02 — Authoritative server

Own: `crates/server/**` only. Never modify `crates/simulation` or `crates/protocol` — compile against the scaffold signatures. If a signature blocks you, code against the contract as written and record a deviation.

Read first: `docs/contracts/milestone-1.md` (§Server endpoints, §Persistence schema, §Asset derivation, §Merge order), `AGENTS.md`.

## Change

Implement `crates/server/src/main.rs` (split into modules freely, e.g. `worlds/`, `protocol_adapter/`, `persistence/`, keep them under `crates/server/src/`):

- Tokio runtime; axum on `127.0.0.1:8080` run from repo root.
- `GET /` serves `./apps/web/dist` with SPA fallback (`tower-http` `ServeDir` + fallback `index.html`).
- `GET /api/assets` → `{"definitions":[...]}`; `POST /api/assets` body Recipe JSON → validate via simulation, derive stats server-side, assign id, persist, broadcast `definition_published`; invalid → `400 {"errors":[...]}`.
- `GET /ws`: on connect send `hello` (actor "owner", full world DTO incl. definitions). Client messages: parse → enqueue into a **bounded** channel (drop/disconnect policy on overflow is fine for M1, but bound it). One world-runner thread/task owns `WorldState` + the single rusqlite connection; network tasks never touch them.
- Command lifecycle: order = server arrival order (never client timestamps); receipt check (same command_id + identical payload ⇒ replay stored outcome; same id different payload ⇒ `duplicate_identity_conflict`); validate → apply → **one transaction** commits mutation + meta counters + receipt → only then broadcast event(s) to all sockets and `command_result` to the issuer. Bad JSON/unknown shape ⇒ `bad_request` result to that socket; socket stays open; nothing broadcast.
- Persistence per contracts SQL (WAL, `synchronous=FULL`, schema_version 1). Boot: load tables → `simulation::save::from_save` → serve state. Seed the three stock definitions on an empty database (exact recipes in contracts) before serving.
- Convert domain ↔ wire at the adapter boundary (`WireId` decimal strings, `Cell`, codes: overlap/out_of_bounds/unknown_asset/unknown_building/duplicate_identity_conflict/bad_request).

## Tests (required)

- Receipt idempotency: replay returns the stored outcome; changed payload rejected.
- Ordered application of concurrent enqueued commands.
- Persistence roundtrip with a tempdir database (place → reopen → same buildings).
- Malformed WS/HTTP payloads produce `bad_request` and leave state untouched.

## Acceptance

`cargo check -p city-server` and `cargo test -p city-server` green. No `todo!()` left. Manual boot/browser pass happens at integration (m1-05), not yours.

## Constraints

Targeted cargo with `-p` only. `rusqlite`/`axum` version bumps allowed in your Cargo.toml if resolution fails — record deviations.
