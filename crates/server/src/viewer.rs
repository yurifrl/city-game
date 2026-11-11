//! The built Svelte viewer, embedded in the binary at compile time so the
//! server is the only artifact. `build.rs` guarantees `apps/viewer/dist`
//! exists (and is current) before this derive reads it.

use axum::http::{header, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "../../apps/viewer/dist"]
struct Dist;

/// Serves the embedded file at the request path. Unknown non-asset paths
/// fall back to `index.html` (single-page app); a missing file under
/// `assets/` is a real 404 rather than HTML masquerading as a script.
pub(crate) async fn serve(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };
    let file = match Dist::get(path) {
        Some(file) => file,
        None if path.starts_with("assets/") => {
            return (StatusCode::NOT_FOUND, "not found").into_response()
        }
        None => match Dist::get("index.html") {
            Some(file) => file,
            None => return (StatusCode::NOT_FOUND, "not found").into_response(),
        },
    };
    ([(header::CONTENT_TYPE, file.metadata.mimetype().to_owned())], file.data).into_response()
}
