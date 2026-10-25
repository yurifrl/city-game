//! City server: authoritative world owner, HTTP API, WebSocket replication.
//! Contract: docs/contracts/multiplayer-stream.md. Run from the repo root:
//! binds 127.0.0.1:8090, persists ./data/city.db, serves ./apps/web/dist.

mod adapter;
mod http;
mod hub;
mod persistence;
mod runner;
mod ws;

use std::path::Path;

use city_simulation::{AssetCatalogue, WorldState};

/// Contract binding (docs/contracts/multiplayer-stream.md). `CITY_BIND` and
/// `CITY_DB` exist only so local smoke tests can avoid a port taken by an
/// unrelated service and keep the world database hermetic.
const BIND_ADDR: &str = "127.0.0.1:8090";
const DB_PATH: &str = "./data/city.db";
const DIST_DIR: &str = "./apps/web/dist";
const PRESENCE_TTL_SECS: u64 = 30;
const PLAYER_TTL_SECS: u64 = 1800;

/// Positive-integer env override, or the default when unset/invalid.
fn env_secs(key: &str, default: u64) -> u64 {
    match std::env::var(key) {
        Ok(value) => value.trim().parse::<u64>().unwrap_or(default),
        Err(_) => default,
    }
}

#[tokio::main]
async fn main() {
    if let Err(err) = run().await {
        eprintln!("city-server: fatal: {err}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), String> {
    if !Path::new(DIST_DIR).exists() {
        eprintln!("city-server: warning: {DIST_DIR} missing; static routes will 404 until the web app is built");
    }

    // Boot: open store → seed on empty DB → load tables → from_save → serve.
    let db_path = std::env::var("CITY_DB").unwrap_or_else(|_| DB_PATH.to_owned());
    let store = persistence::Store::open(Path::new(&db_path))?;
    if store.is_empty()? {
        eprintln!("city-server: empty database; seeding 3 stock definitions");
        store.seed()?;
    }
    let (world, catalogue): (WorldState, AssetCatalogue) = store.reload()?;
    eprintln!(
        "city-server: world loaded: tick {}, {} buildings, {} definitions",
        world.tick_count(),
        world.buildings().count(),
        catalogue.iter().count(),
    );

    let (world_tx, world_rx) = tokio::sync::mpsc::channel(runner::QUEUE_CAPACITY);
    let (hub_tx, hub_rx) = tokio::sync::mpsc::channel(64);
    tokio::spawn(hub::run(hub_rx));
    tokio::spawn(runner::run(
        world,
        catalogue,
        store,
        world_rx,
        hub_tx.clone(),
        runner::TtlConfig {
            presence_secs: env_secs("CITY_PRESENCE_TTL_SECS", PRESENCE_TTL_SECS),
            player_secs: env_secs("CITY_PLAYER_TTL_SECS", PLAYER_TTL_SECS),
        },
    ));

    let app = http::App {
        world_tx,
        hub: hub_tx,
        next_conn: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0)),
    };

    let bind_addr = std::env::var("CITY_BIND").unwrap_or_else(|_| BIND_ADDR.to_owned());
    let listener = tokio::net::TcpListener::bind(&bind_addr)
        .await
        .map_err(|err| format!("cannot bind {bind_addr}: {err}"))?;
    eprintln!("city-server: listening on http://{bind_addr}");
    axum::serve(listener, http::router(app, DIST_DIR))
        .await
        .map_err(|err| format!("server error: {err}"))
}
