//! City server: authoritative world owner, HTTP API, WebSocket replication.
//! Contract: docs/contracts/milestone-1.md. Run from the repo root:
//! binds 127.0.0.1:8080, persists ./data/city.db, serves ./apps/web/dist.

fn main() {
    todo!("m1-02: tokio runtime; axum router (GET /, GET/POST /api/assets, GET /ws); \
           world runner thread owning WorldState + rusqlite connection; bounded \
           command queue; receipt idempotency; transactional commit then broadcast")
}
