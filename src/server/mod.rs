//! Embedded Axum server: file browser HTTP endpoints + change-hint
//! WebSocket channel.

pub mod static_files;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use notify::Watcher;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

use crate::fs;

/// Debounce window for the filesystem watcher: events are batched until this
/// much quiet time elapses, then one hint is broadcast per affected directory.
const WATCH_DEBOUNCE_MS: u64 = 200;
/// Search result cap, matching the web UI's single-fetch expectation.
const SEARCH_LIMIT: usize = 200;

/// A server-pushed change hint. HTTP remains the source of truth; the
/// WebSocket only tells clients which directory to refetch.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeHint {
    #[serde(rename = "type")]
    pub kind: String,
    /// Root-relative directory path to refetch; `""` means the root.
    pub path: String,
}

impl ChangeHint {
    fn changed(path: String) -> Self {
        Self {
            kind: "changed".to_string(),
            path,
        }
    }
}

/// Shared server state: the browsable root and the change-hint broadcaster.
#[derive(Clone)]
pub struct AppState {
    root: String,
    root_marker: PathBuf,
    tx: broadcast::Sender<ChangeHint>,
}

impl AppState {
    pub fn new(root: PathBuf) -> Self {
        let (tx, _) = broadcast::channel(256);
        Self {
            root_marker: root.clone(),
            root: root.display().to_string(),
            tx,
        }
    }

    pub fn spawn_watcher(&self) {
        spawn_watcher(self.root_marker.clone(), self.tx.clone());
    }
}

#[derive(Serialize, Deserialize)]
pub struct Info {
    pub root: String,
}

/// Reports the browsable root (with `$HOME` abbreviated to `~`).
pub async fn info_json(State(state): State<AppState>) -> Json<Info> {
    Json(Info {
        root: state.root.clone(),
    })
}

#[derive(Deserialize)]
pub struct FileTreeQuery {
    #[serde(default)]
    path: String,
}

pub async fn filetree_handler(
    State(state): State<AppState>,
    axum::extract::Query(query): axum::extract::Query<FileTreeQuery>,
) -> (StatusCode, Json<Vec<fs::FileTreeEntry>>) {
    let root = PathBuf::from(&state.root_marker);
    match tokio::task::spawn_blocking(move || fs::list_dir(&root, &query.path)).await {
        Ok(entries) => (StatusCode::OK, Json(entries)),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(vec![fs::FileTreeEntry {
                name: format!("listing task panicked: {e}"),
                path: String::new(),
                is_dir: false,
                depth: 0,
            }]),
        ),
    }
}

#[derive(Deserialize)]
pub struct FileContentQuery {
    path: String,
    #[serde(default)]
    raw: bool,
}

pub async fn filecontent_handler(
    State(state): State<AppState>,
    axum::extract::Query(query): axum::extract::Query<FileContentQuery>,
) -> Response {
    if query.raw {
        let root = state.root_marker.clone();
        let Some(full) = fs::safe_join(&root, &query.path) else {
            return (
                StatusCode::BAD_REQUEST,
                "path escapes the root directory".to_string(),
            )
                .into_response();
        };
        let file_path = query.path.clone();
        match tokio::task::spawn_blocking(move || std::fs::read(&full)).await {
            Ok(Ok(bytes)) => {
                let mime = fs::mime_for_path(&file_path);
                ([(header::CONTENT_TYPE, mime)], bytes).into_response()
            }
            Ok(Err(e)) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("failed to read {}: {e}", file_path),
            )
                .into_response(),
            Err(e) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("read task panicked: {e}"),
            )
                .into_response(),
        }
    } else {
        let Some(_full) = fs::safe_join(&state.root_marker, &query.path) else {
            return (
                StatusCode::BAD_REQUEST,
                "path escapes the root directory".to_string(),
            )
                .into_response();
        };
        let root = state.root_marker.clone();
        let path = query.path.clone();
        match tokio::task::spawn_blocking(move || fs::get_file_content(&root, &path)).await {
            Ok(content) => Json(content).into_response(),
            Err(e) => Json(fs::FileContent {
                path: query.path,
                size: 0,
                is_binary: false,
                is_image: false,
                content: String::new(),
                error: format!("task panicked: {e}"),
            })
            .into_response(),
        }
    }
}

#[derive(Deserialize)]
pub struct FileSearchQuery {
    q: String,
}

pub async fn filesearch_handler(
    State(state): State<AppState>,
    axum::extract::Query(query): axum::extract::Query<FileSearchQuery>,
) -> (StatusCode, Json<Vec<fs::FileTreeEntry>>) {
    let root = state.root_marker.clone();
    let q = query.q;
    match tokio::task::spawn_blocking(move || fs::search_files(&root, &q, SEARCH_LIMIT)).await {
        Ok(entries) => (StatusCode::OK, Json(entries)),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(vec![fs::FileTreeEntry {
                name: format!("search task panicked: {e}"),
                path: String::new(),
                is_dir: false,
                depth: 0,
            }]),
        ),
    }
}

#[derive(Deserialize)]
pub struct MutateRequest {
    path: String,
    #[serde(default)]
    to: String,
}

#[derive(Serialize)]
pub struct MutateResponse {
    ok: bool,
}

fn broadcast_after_mutation(state: &AppState, paths: Vec<String>) {
    for p in paths {
        let _ = state.tx.send(ChangeHint::changed(p));
    }
}

/// Parent directory of a root-relative path, also root-relative. `""` for
/// anything at or above the root.
pub fn parent_rel(rel: &str) -> String {
    match rel.rfind('/') {
        Some(idx) => rel[..idx].to_string(),
        None => String::new(),
    }
}

pub async fn rename_handler(
    State(state): State<AppState>,
    Json(body): Json<MutateRequest>,
) -> Response {
    let root = state.root_marker.clone();
    let from = body.path.clone();
    let to = body.to.clone();
    let affected = {
        let mut affected = vec![parent_rel(&from)];
        let to_parent = parent_rel(&to);
        if to_parent != affected[0] {
            affected.push(to_parent);
        }
        affected
    };
    let result =
        tokio::task::spawn_blocking(move || fs::rename_path(&root, &from, &to)).await;
    match result {
        Ok(Ok(())) => {
            broadcast_after_mutation(&state, affected);
            (StatusCode::OK, Json(MutateResponse { ok: true })).into_response()
        }
        Ok(Err(fs::FsError::EscapedRoot)) => (
            StatusCode::BAD_REQUEST,
            Json(MutateResponse { ok: false }),
        )
            .into_response(),
        Ok(Err(fs::FsError::Io(e))) => {
            tracing::warn!("rename failed: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(MutateResponse { ok: false }),
            )
                .into_response()
        }
        Err(e) => {
            tracing::warn!("rename task panicked: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(MutateResponse { ok: false }),
            )
                .into_response()
        }
    }
}

pub async fn delete_handler(
    State(state): State<AppState>,
    Json(body): Json<MutateRequest>,
) -> Response {
    let root = state.root_marker.clone();
    let path = body.path.clone();
    let affected_parent = parent_rel(&path);
    let result =
        tokio::task::spawn_blocking(move || fs::delete_path(&root, &path)).await;
    match result {
        Ok(Ok(())) => {
            broadcast_after_mutation(&state, vec![affected_parent]);
            (StatusCode::OK, Json(MutateResponse { ok: true })).into_response()
        }
        Ok(Err(fs::FsError::EscapedRoot)) => (
            StatusCode::BAD_REQUEST,
            Json(MutateResponse { ok: false }),
        )
            .into_response(),
        Ok(Err(fs::FsError::Io(e))) => {
            tracing::warn!("delete failed: {e}");
            (
                StatusCode::CONFLICT,
                Json(MutateResponse { ok: false }),
            )
                .into_response()
        }
        Err(e) => {
            tracing::warn!("delete task panicked: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(MutateResponse { ok: false }),
            )
                .into_response()
        }
    }
}

/// Upgrades a connection to the change-hint channel. The socket is
/// push-only: the server relays every `ChangeHint` broadcast as JSON.
pub async fn update_hint_handler(ws: WebSocketUpgrade, State(state): State<AppState>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| hint_socket(socket, state))
}

async fn hint_socket(mut socket: WebSocket, state: AppState) {
    let mut rx = state.tx.subscribe();
    // Drop the backlog so a fresh connection starts from the present.
    while rx.try_recv().is_ok() {}
    loop {
        tokio::select! {
            msg = rx.recv() => {
                match msg {
                    Ok(hint) => {
                        let Ok(text) = serde_json::to_string(&hint) else { continue; };
                        if socket.send(Message::Text(text.into())).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
            incoming = socket.recv() => {
                match incoming {
                    Some(Ok(_)) => {}
                    _ => break,
                }
            }
        }
    }
}

/// Watches the root recursively and broadcasts change hints, batching events
/// through a short debounce so directory churn (builds, target/) collapses
/// into a single hint per affected directory.
fn spawn_watcher(root: PathBuf, tx: broadcast::Sender<ChangeHint>) {
    tokio::spawn(async move {
        let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel();
        let mut watcher = match notify::recommended_watcher(
            move |res: notify::Result<notify::Event>| {
                if let Ok(ev) = res {
                    let _ = event_tx.send(ev);
                }
            },
        ) {
            Ok(w) => w,
            Err(e) => {
                tracing::warn!("file watcher failed to start: {e}");
                return;
            }
        };
        if let Err(e) = watcher.watch(&root, notify::RecursiveMode::Recursive) {
            tracing::warn!("failed to watch root {}: {e}", root.display());
            return;
        }
        tracing::info!("watching {} for changes", root.display());

        loop {
            let Some(ev) = event_rx.recv().await else { break; };
            let mut pending: BTreeSet<String> = BTreeSet::new();
            absorb(&ev, &root, &mut pending);

            loop {
                let deadline = tokio::time::sleep(std::time::Duration::from_millis(
                    WATCH_DEBOUNCE_MS,
                ));
                tokio::pin!(deadline);
                tokio::select! {
                    ev = event_rx.recv() => {
                        match ev {
                            Some(ev) => absorb(&ev, &root, &mut pending),
                            None => break,
                        }
                    }
                    _ = &mut deadline => {
                        let flushed: Vec<String> = pending.iter().cloned().collect();
                        pending.clear();
                        for dir in flushed {
                            let _ = tx.send(ChangeHint::changed(dir));
                        }
                        break;
                    }
                }
            }
        }
    });
}

fn absorb(ev: &notify::Event, root: &Path, pending: &mut BTreeSet<String>) {
    use notify::EventKind;
    match ev.kind {
        EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_) | EventKind::Any => {}
        _ => return,
    }
    for p in &ev.paths {
        if let Ok(rel) = p.strip_prefix(root) {
            let rel = rel.to_string_lossy().replace('\\', "/");
            pending.insert(parent_rel(&rel));
        }
    }
}

/// Builds the router for the given state.
pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/info", get(info_json))
        .route("/filetree", get(filetree_handler))
        .route("/filecontent", get(filecontent_handler))
        .route("/filesearch", get(filesearch_handler))
        .route("/ws", get(update_hint_handler))
        .route("/rename", post(rename_handler))
        .route("/delete", post(delete_handler))
        .route("/", get(static_files::serve_static))
        .route("/{*path}", get(static_files::serve_static))
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    use super::*;

    fn app_for(root: &Path) -> Router {
        build_router(AppState::new(root.to_path_buf()))
    }

    #[tokio::test]
    async fn info_reports_root() {
        let dir = tempfile::tempdir().unwrap();
        let response = app_for(dir.path())
            .oneshot(Request::builder().uri("/info").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let info: Info = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(info.root, dir.path().display().to_string());
    }

    #[tokio::test]
    async fn filetree_lists_root_and_subdir() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src/sub")).unwrap();
        std::fs::write(dir.path().join("src/main.rs"), "fn main() {}\n").unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "[package]\n").unwrap();

        let router = app_for(dir.path());

        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/filetree?path=")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let entries: Vec<fs::FileTreeEntry> = serde_json::from_slice(&bytes).unwrap();
        let paths: Vec<(&str, bool)> =
            entries.iter().map(|e| (e.path.as_str(), e.is_dir)).collect();
        assert!(paths.contains(&("Cargo.toml", false)), "got: {paths:?}");
        assert!(paths.contains(&("src", true)), "got: {paths:?}");
        assert_eq!(paths.len(), 2);

        let response = router
            .oneshot(
                Request::builder()
                    .uri("/filetree?path=src")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let entries: Vec<fs::FileTreeEntry> = serde_json::from_slice(&bytes).unwrap();
        let paths: Vec<&str> = entries.iter().map(|e| e.path.as_str()).collect();
        assert!(
            paths.contains(&"src/sub") && paths.contains(&"src/main.rs"),
            "got: {paths:?}"
        );
        assert!(entries[0].is_dir, "dirs must come first: {paths:?}");
    }

    #[tokio::test]
    async fn filetree_rejects_traversal() {
        let dir = tempfile::tempdir().unwrap();
        let response = app_for(dir.path())
            .oneshot(
                Request::builder()
                    .uri("/filetree?path=../etc")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let entries: Vec<fs::FileTreeEntry> = serde_json::from_slice(&bytes).unwrap();
        assert!(entries.is_empty());
    }
}