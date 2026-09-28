//! Embedded static asset serving.
//!
//! Reads the `build.rs`-generated table in [`crate::assets`]. Content types
//! come from [`crate::fs::mime_for_path`], the same table the rest of the
//! server uses, so a new file under `web/dist/` is served with no code change
//! beyond its extension.

use axum::http::{header, StatusCode, Uri};
use axum::response::{IntoResponse, Response};

use crate::{assets, fs};

/// Serves an embedded asset by path; `index.html` is the default.
pub async fn serve_static(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() || path == "favicon.ico" {
        "index.html"
    } else {
        path
    };
    match assets::get(&format!("web/dist/{path}")) {
        Some(content) => (
            [
                (header::CONTENT_TYPE, fs::mime_for_path(path)),
                (header::CACHE_CONTROL, "no-store"),
            ],
            content,
        )
            .into_response(),
        None => (StatusCode::NOT_FOUND, "not found".to_string()).into_response(),
    }
}
