//! Embedded static asset serving via rust-embed.

use axum::http::{header, Uri};
use axum::response::{IntoResponse, Response};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "web/dist"]
struct Assets;

/// Serves an embedded asset by path; `index.html` is the default.
pub async fn serve_static(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() || path == "favicon.ico" {
        "index.html"
    } else {
        path
    };
    match Assets::get(path) {
        Some(content) => {
            let mime = content.metadata.mimetype();
            ([(header::CONTENT_TYPE, mime)], content.data.into_owned()).into_response()
        }
        None => (
            axum::http::StatusCode::NOT_FOUND,
            "not found".to_string(),
        )
            .into_response(),
    }
}