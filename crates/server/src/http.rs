//! HTTP surface: static SPA hosting, asset endpoints, WS upgrade.

use axum::extract::ws::WebSocketUpgrade;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{any, delete, get};
use axum::{Json, Router};
use city_protocol::Recipe;
use serde::Serialize;
use std::sync::atomic::AtomicU64;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot};
use tower_http::services::{ServeDir, ServeFile};

use crate::hub::HubHandle;
use crate::runner::WorldRequest;
use crate::ws;

/// State shared by all handlers: the ingress queue into the world runner
/// and the connection id allocator. Network tasks never touch world state.
#[derive(Clone)]
pub(crate) struct App {
    pub(crate) world_tx: mpsc::Sender<WorldRequest>,
    pub(crate) hub: HubHandle,
    pub(crate) next_conn: Arc<AtomicU64>,
}

#[derive(Serialize)]
struct DefinitionsResponse {
    definitions: Vec<city_protocol::AssetDefinition>,
}

pub(crate) fn router(app: App, dist_dir: &str) -> Router {
    let index = std::path::Path::new(dist_dir).join("index.html");
    let static_service = ServeDir::new(dist_dir).fallback(ServeFile::new(index));
    Router::new()
        .route("/api/assets", get(assets_get).post(assets_post))
        .route("/api/assets/{id}", delete(assets_delete))
        .route("/ws", get(ws_upgrade))
        .route("/api/{*rest}", any(api_not_found))
        .fallback_service(static_service)
        .with_state(app)
}

async fn api_not_found() -> Response {
    (StatusCode::NOT_FOUND, "not found").into_response()
}

/// `GET /api/assets` → `{"definitions":[...]}` from the world owner, so the
/// catalogue has exactly one source of truth.
async fn assets_get(State(app): State<App>) -> Response {
    let (tx, rx) = oneshot::channel();
    if app
        .world_tx
        .send(WorldRequest::Snapshot { reply: tx })
        .await
        .is_err()
    {
        return service_unavailable();
    }
    let Ok(dto) = rx.await else {
        return service_unavailable();
    };
    (
        StatusCode::OK,
        Json(DefinitionsResponse {
            definitions: dto.definitions,
        }),
    )
        .into_response()
}

/// `POST /api/assets` body = Recipe JSON. 200 = full published definition
/// (validated, derived, persisted, broadcast); 400 = `{"errors":[...]}`.
async fn assets_post(State(app): State<App>, body: String) -> Response {
    let recipe: Recipe = match serde_json::from_str(&body) {
        Ok(recipe) => recipe,
        Err(err) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "errors": [format!("invalid recipe JSON: {err}")] })),
            )
                .into_response()
        }
    };
    let (tx, rx) = oneshot::channel();
    if app
        .world_tx
        .send(WorldRequest::PublishAsset { recipe, reply: tx })
        .await
        .is_err()
    {
        return service_unavailable();
    }
    match rx.await {
        Ok(Ok(definition)) => (StatusCode::OK, Json(definition)).into_response(),
        Ok(Err(errors)) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "errors": errors })),
        )
            .into_response(),
        Err(_) => service_unavailable(),
    }
}

/// `DELETE /api/assets/{id}`. 200 = removed (persisted, broadcast);
/// 409 = refused (`{"errors":[...]}` — unknown id or placed buildings).
async fn assets_delete(State(app): State<App>, Path(id): Path<String>) -> Response {
    let Ok(def_id) = id.parse::<u64>() else {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "errors": [format!("def id {id:?} is not a number")] })),
        )
            .into_response();
    };
    let (tx, rx) = oneshot::channel();
    if app
        .world_tx
        .send(WorldRequest::RemoveDefinition { def_id, reply: tx })
        .await
        .is_err()
    {
        return service_unavailable();
    }
    match rx.await {
        Ok(Ok(())) => {
            (StatusCode::OK, Json(serde_json::json!({ "removed": true }))).into_response()
        }
        Ok(Err(message)) => (
            StatusCode::CONFLICT,
            Json(serde_json::json!({ "errors": [message] })),
        )
            .into_response(),
        Err(_) => service_unavailable(),
    }
}

async fn ws_upgrade(State(app): State<App>, upgrade: WebSocketUpgrade) -> Response {
    upgrade.on_upgrade(move |socket| ws::run_connection(app, socket))
}

fn service_unavailable() -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(serde_json::json!({ "errors": ["world runner unavailable"] })),
    )
        .into_response()
}
