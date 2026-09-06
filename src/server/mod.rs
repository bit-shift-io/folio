//! Embedded Axum server: file browser HTTP endpoints + change-hint
//! WebSocket channel. Handlers operate on absolute paths; the CLI `--root`
//! only sets the initial directory, browsing is unrestricted.

pub mod static_files;

use rust_embed::RustEmbed;

use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use tower_http::cors::CorsLayer;
use notify::Watcher;
use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, mpsc};

use crate::apps;
use crate::config;
use crate::fs;
use crate::media;

/// Debounce window for the filesystem watcher: events are batched until this
/// much quiet time elapses, then one hint is broadcast per affected directory.
const WATCH_DEBOUNCE_MS: u64 = 200;
/// Search result cap, matching the web UI's single-fetch expectation.
const SEARCH_LIMIT: usize = 200;

/// Icon themes embedded into the binary so they load in release builds
/// regardless of the working directory. Served as a fallback when no
/// matching file is found on disk (themes_dir).
#[derive(RustEmbed)]
#[folder = "res/icons"]
struct IconAssets;

/// A server-pushed change hint. HTTP remains the source of truth; the
/// WebSocket only tells clients which directory to refetch.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeHint {
    #[serde(rename = "type")]
    pub kind: String,
    /// Absolute directory path to refetch.
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

/// Shared server state: initial dir, home dir (for `~` in the path bar), the
/// change-hint broadcaster, and a channel that tells the watcher which
/// directory to follow.
#[derive(Clone)]
pub struct AppState {
    root: String,
    root_marker: PathBuf,
    home: String,
    tx: broadcast::Sender<ChangeHint>,
    watch_tx: mpsc::UnboundedSender<PathBuf>,
    watch_rx: Arc<std::sync::Mutex<Option<mpsc::UnboundedReceiver<PathBuf>>>>,
    themes_dir: PathBuf,
    config_path: PathBuf,
}

impl AppState {
    pub fn new(root: PathBuf) -> Self {
        let (tx, _) = broadcast::channel(256);
        let (watch_tx, watch_rx) = mpsc::unbounded_channel();
        let home = std::env::var("HOME").unwrap_or_default();
        let themes_dir = std::env::current_dir()
            .unwrap_or_default()
            .join("res")
            .join("icons");
        Self {
            root_marker: root.clone(),
            root: root.display().to_string(),
            home,
            tx,
            watch_tx,
            watch_rx: Arc::new(std::sync::Mutex::new(Some(watch_rx))),
            themes_dir,
            config_path: config::config_path().unwrap_or_default(),
        }
    }

    pub fn spawn_watcher(&self) {
        let rx = self
            .watch_rx
            .lock()
            .ok()
            .and_then(|mut guard| guard.take());
        let Some(rx) = rx else {
            tracing::warn!("file watcher already spawned once");
            return;
        };
        spawn_watcher(self.root_marker.clone(), rx, self.tx.clone());
    }
}

#[derive(Serialize, Deserialize)]
pub struct Info {
    pub root: String,
    pub home: String,
}

/// Reports the initial directory and the user's home (for `~` expansion).
pub async fn info_json(State(state): State<AppState>) -> Json<Info> {
    Json(Info {
        root: state.root.clone(),
        home: state.home.clone(),
    })
}

/// Serves an icon theme asset from disk: `/icons/<theme>/<subdir>/<file>`,
/// rooted at the themes directory (`res/icons`). The theme name is the first
/// path segment and everything after it must stay inside that theme's folder.
pub async fn icon_handler(
    State(state): State<AppState>,
    axum::extract::Path((theme, path)): axum::extract::Path<(String, String)>,
) -> Response {
    let theme_p = Path::new(&theme);
    let theme_ok = !theme.is_empty()
        && theme_p.components().count() == 1
        && theme_p
            .components()
            .next()
            .is_some_and(|c| matches!(c, Component::Normal(_)));
    if !theme_ok {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let rel = Path::new(&path);
    let rel_ok = !rel
        .components()
        .any(|c| matches!(c, Component::ParentDir | Component::RootDir | Component::Prefix(_)));
    if !rel_ok {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let file = state.themes_dir.join(&theme).join(rel);
    match std::fs::read(&file) {
        Ok(bytes) => {
            let mime = if file.extension().is_some_and(|e| e == "svg") {
                "image/svg+xml"
            } else {
                fs::mime_for_path(&file.display().to_string())
            };
            ([(header::CONTENT_TYPE, mime)], bytes).into_response()
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // Fall back to the copy embedded in the binary, so release
            // builds work even when launched outside the project dir where
            // res/icons doesn't exist on disk.
            match IconAssets::get(&format!("{theme}/{}", rel.display())) {
                Some(content) => {
                    let mime = fs::mime_for_path(&format!("{theme}/{}", rel.display()));
                    (
                        [(header::CONTENT_TYPE, mime)],
                        content.data.into_owned(),
                    )
                        .into_response()
                }
                None => StatusCode::NOT_FOUND.into_response(),
            }
        }
        Err(e) => {
            tracing::warn!("failed to read {}: {e}", file.display());
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// Resolves a client-supplied path: empty → the initial root; absolute →
/// used as-is; otherwise joined onto the initial root. There is no
/// confinement — `..` is resolved by the OS, exactly as a shell would.
fn resolve_path(raw: &str, base: &Path) -> PathBuf {
    if raw.is_empty() {
        return base.to_path_buf();
    }
    let p = Path::new(raw);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        base.join(p)
    }
}

/// Parent directory of an absolute path. `/etc/x` → `/etc`; `/etc/` → `/etc`;
/// `/` → `/`.
fn parent_of_abs(p: &str) -> String {
    let p = p.trim_end_matches('/');
    if p.is_empty() {
        return "/".to_string();
    }
    match p.rfind('/') {
        Some(0) => "/".to_string(),
        Some(i) => p[..i].to_string(),
        None => "/".to_string(),
    }
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
    let base = resolve_path(&query.path, &state.root_marker);
    match tokio::task::spawn_blocking(move || fs::list_dir(&base)).await {
        Ok(Ok(entries)) => (StatusCode::OK, Json(entries)),
        Ok(Err(fs::ListError::NotFound)) => (StatusCode::NOT_FOUND, Json(Vec::new())),
        Ok(Err(fs::ListError::NotADirectory)) => (StatusCode::BAD_REQUEST, Json(Vec::new())),
        Ok(Err(fs::ListError::Io(e))) => {
            tracing::warn!("list_dir failed: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, Json(Vec::new()))
        }
        Err(e) => {
            tracing::warn!("filetree task panicked: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, Json(Vec::new()))
        }
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
    let base = resolve_path(&query.path, &state.root_marker);
    let path_str = base.display().to_string();
    if query.raw {
        match tokio::task::spawn_blocking(move || std::fs::read(&base)).await {
            Ok(Ok(bytes)) => {
                let mime = fs::mime_for_path(&path_str);
                ([(header::CONTENT_TYPE, mime)], bytes).into_response()
            }
            Ok(Err(e)) if e.kind() == std::io::ErrorKind::NotFound => {
                (StatusCode::NOT_FOUND, format!("no such file: {path_str}")).into_response()
            }
            Ok(Err(e)) => {
                tracing::warn!("failed to read {}: {e}", path_str);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("failed to read {path_str}"),
                )
                    .into_response()
            }
            Err(e) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("read task panicked: {e}"),
            )
                .into_response(),
        }
    } else {
        match tokio::task::spawn_blocking(move || fs::get_file_content(&base)).await {
            Ok(content) => Json(content).into_response(),
            Err(e) => Json(fs::FileContent {
                path: path_str.clone(),
                size: 0,
                is_binary: false,
                is_image: false,
                mime: String::new(),
                content: String::new(),
                error: format!("task panicked: {e}"),
            })
            .into_response(),
        }
    }
}

#[derive(Deserialize)]
pub struct FileInfoQuery {
    path: String,
}

/// Metadata for the file/folder shown in the preview's properties panel,
/// plus media headers when the file type is one we can parse.
#[derive(Serialize)]
pub struct FileInfo {
    pub name: String,
    pub path: String,
    pub size: u64,
    pub is_dir: bool,
    /// Modification time in unix seconds.
    pub modified: u64,
    /// Unix permission bits from `MetadataExt::mode`.
    pub mode: u32,
    pub mime: String,
    pub media: Option<media::MediaInfo>,
}

pub async fn fileinfo_handler(
    State(state): State<AppState>,
    axum::extract::Query(query): axum::extract::Query<FileInfoQuery>,
) -> Response {
    let base = resolve_path(&query.path, &state.root_marker);
    let path_str = base.display().to_string();
    let result = tokio::task::spawn_blocking(move || -> Option<FileInfo> {
        use std::os::unix::fs::MetadataExt;
        let meta = std::fs::metadata(&base).ok()?;
        let is_dir = meta.is_dir();
        let name = base
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| path_str.clone());
        let mime = if is_dir {
            "inode/directory".to_string()
        } else {
            fs::mime_for_entry(&base, &name).to_string()
        };
        let media = if is_dir { None } else { media::probe(&base, &mime) };
        Some(FileInfo {
            name,
            path: path_str,
            size: meta.len(),
            is_dir,
            modified: meta.mtime().max(0) as u64,
            mode: meta.mode(),
            mime,
            media,
        })
    })
    .await;
    match result {
        Ok(Some(info)) => Json(info).into_response(),
        Ok(None) => (StatusCode::NOT_FOUND, Json(MutateResponse { ok: false })).into_response(),
        Err(e) => {
            tracing::warn!("fileinfo task panicked: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, Json(MutateResponse { ok: false })).into_response()
        }
    }
}

#[derive(Deserialize)]
pub struct FileSearchQuery {
    q: String,
    /// Search base directory; defaults to the initial root.
    #[serde(default)]
    path: String,
}

pub async fn filesearch_handler(
    State(state): State<AppState>,
    axum::extract::Query(query): axum::extract::Query<FileSearchQuery>,
) -> (StatusCode, Json<Vec<fs::FileTreeEntry>>) {
    let base = resolve_path(&query.path, &state.root_marker);
    let q = query.q;
    match tokio::task::spawn_blocking(move || fs::search_files(&base, &q, SEARCH_LIMIT)).await {
        Ok(entries) => (StatusCode::OK, Json(entries)),
        Err(e) => {
            tracing::warn!("search task panicked: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, Json(Vec::new()))
        }
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

pub async fn rename_handler(
    State(state): State<AppState>,
    Json(body): Json<MutateRequest>,
) -> Response {
    if body.to.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(MutateResponse { ok: false }),
        )
            .into_response();
    }
    let from = resolve_path(&body.path, &state.root_marker);
    let to = resolve_path(&body.to, &state.root_marker);
    let from_s = from.display().to_string();
    let to_s = to.display().to_string();
    let affected = {
        let mut affected = vec![parent_of_abs(&from_s)];
        let to_parent = parent_of_abs(&to_s);
        if to_parent != affected[0] {
            affected.push(to_parent);
        }
        affected
    };
    let result = tokio::task::spawn_blocking(move || fs::rename_path(&from, &to)).await;
    match result {
        Ok(Ok(())) => {
            broadcast_after_mutation(&state, affected);
            (StatusCode::OK, Json(MutateResponse { ok: true })).into_response()
        }
        Ok(Err(fs::FsError::InvalidInput(msg))) => {
            tracing::warn!("rename rejected: {msg}");
            (StatusCode::BAD_REQUEST, Json(MutateResponse { ok: false })).into_response()
        }
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
    let path = resolve_path(&body.path, &state.root_marker);
    let path_s = path.display().to_string();
    let affected_parent = parent_of_abs(&path_s);
    let result = tokio::task::spawn_blocking(move || fs::delete_path(&path)).await;
    match result {
        Ok(Ok(())) => {
            broadcast_after_mutation(&state, vec![affected_parent]);
            (StatusCode::OK, Json(MutateResponse { ok: true })).into_response()
        }
        Ok(Err(fs::FsError::InvalidInput(msg))) => {
            tracing::warn!("delete rejected: {msg}");
            (StatusCode::BAD_REQUEST, Json(MutateResponse { ok: false })).into_response()
        }
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

/// Lists installed desktop applications for the preview pane's "edit"
/// dropdown. Cellar of truth for `/open`: only apps returned here can be
/// launched.
pub async fn apps_handler() -> impl IntoResponse {
    match tokio::task::spawn_blocking(apps::list_apps).await {
        Ok(apps) => Json(apps).into_response(),
        Err(e) => {
            tracing::warn!("apps discovery panicked: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(Vec::<apps::AppEntry>::new()),
            )
                .into_response()
        }
    }
}

#[derive(Deserialize)]
pub struct OpenRequest {
    /// id of an app returned by `/apps` (the `.desktop` file path).
    id: String,
    /// Absolute path of the file/folder to open.
    path: String,
}

/// Launches an enumerated application with the given path as its argument.
/// Only ids the server itself discovered are accepted.
pub async fn open_handler(
    State(state): State<AppState>,
    Json(body): Json<OpenRequest>,
) -> Response {
    if body.id.trim().is_empty() || body.path.trim().is_empty() {
        return (StatusCode::BAD_REQUEST, Json(MutateResponse { ok: false })).into_response();
    }
    let target = resolve_path(&body.path, &state.root_marker);
    let path_str = target.display().to_string();
    let mime = fs::mime_for_path(&path_str).to_string();
    let id = body.id;
    let config_path = state.config_path.clone();
    let result = tokio::task::spawn_blocking(move || {
        apps::open_with(&id, &target)?;
        if !config_path.as_os_str().is_empty() {
            let mut cfg = config::load_from(&config_path);
            cfg.last_app.insert(mime, id);
            config::save_to(&config_path, &cfg);
        }
        Ok(())
    })
    .await;
    match result {
        Ok(Ok(())) => (StatusCode::OK, Json(MutateResponse { ok: true })).into_response(),
        Ok(Err(apps::LaunchError::NotListed(_))) => {
            (StatusCode::BAD_REQUEST, Json(MutateResponse { ok: false })).into_response()
        }
        Ok(Err(apps::LaunchError::MissingTarget(_))) => {
            (StatusCode::NOT_FOUND, Json(MutateResponse { ok: false })).into_response()
        }
        Ok(Err(apps::LaunchError::NoExec(_))) => {
            (StatusCode::BAD_REQUEST, Json(MutateResponse { ok: false })).into_response()
        }
        Ok(Err(apps::LaunchError::Spawn(e))) => {
            tracing::warn!("open with failed: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, Json(MutateResponse { ok: false })).into_response()
        }
        Err(e) => {
            tracing::warn!("open task panicked: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, Json(MutateResponse { ok: false })).into_response()
        }
    }
}

#[derive(Deserialize)]
pub struct DefaultAppQuery {
    /// MIME type to look up; unknown types get no default.
    #[serde(default)]
    mime: String,
}

#[derive(Serialize)]
pub struct DefaultAppResponse {
    /// `.desktop` id last used for this MIME type, if any.
    id: Option<String>,
}

/// Reports the app chosen most recently for a file type, so the preview's
/// edit button can launch it directly.
pub async fn default_app_handler(
    axum::extract::Query(query): axum::extract::Query<DefaultAppQuery>,
) -> impl IntoResponse {
    let id = config::load().last_app.get(&query.mime).cloned();
    Json(DefaultAppResponse { id })
}

/// Upgrades a connection to the change-hint channel. Besides relaying server
/// hints it accepts `{type:"watch", path}` messages that tell the watcher
/// which directory to follow (the currently viewed folder).
pub async fn update_hint_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> impl IntoResponse {
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
                    Some(Ok(Message::Text(text))) => {
                        let s: &str = &text;
                        if let Ok(value) = serde_json::from_str::<serde_json::Value>(s) {
                            if value.get("type").and_then(|t| t.as_str()) == Some("watch") {
                                if let Some(p) = value.get("path").and_then(|p| p.as_str()) {
                                    let _ = state.watch_tx.send(PathBuf::from(p));
                                }
                            }
                        }
                    }
                    Some(Ok(_)) => {}
                    _ => break,
                }
            }
        }
    }
}

/// Watches whichever directory the frontend is viewing and broadcasts change
/// hints, batching events through a short debounce so directory churn
/// collapses into a single hint per affected directory. The frontend swaps the
/// watched directory over the WebSocket; when nothing tells it otherwise, the
/// initial root is watched.
fn spawn_watcher(
    initial: PathBuf,
    mut watch_rx: mpsc::UnboundedReceiver<PathBuf>,
    tx: broadcast::Sender<ChangeHint>,
) {
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
        let mut watched: Option<PathBuf> = None;
        set_watch(&mut watcher, &mut watched, initial);

        let mut pending: BTreeSet<String> = BTreeSet::new();
        loop {
            tokio::select! {
                ev = event_rx.recv() => {
                    match ev {
                        Some(ev) => absorb(&ev, &mut pending, watched.as_deref()),
                        None => return,
                    }
                    loop {
                        let deadline = tokio::time::sleep(std::time::Duration::from_millis(WATCH_DEBOUNCE_MS));
                        tokio::pin!(deadline);
                        tokio::select! {
                            ev = event_rx.recv() => {
                                match ev {
                                    Some(ev) => absorb(&ev, &mut pending, watched.as_deref()),
                                    None => return,
                                }
                            }
                            _ = &mut deadline => {
                                flush(&mut pending, &tx);
                                break;
                            }
                        }
                    }
                }
                w = watch_rx.recv() => {
                    match w {
                        Some(dir) => set_watch(&mut watcher, &mut watched, dir),
                        None => return,
                    }
                }
            }
        }
    });
}

fn set_watch(
    watcher: &mut notify::RecommendedWatcher,
    watched: &mut Option<PathBuf>,
    dir: PathBuf,
) {
    let dir = normalize_dir(dir);
    if watched.as_deref() == Some(dir.as_path()) {
        return;
    }
    if let Some(prev) = watched.take() {
        let _ = watcher.unwatch(&prev);
    }
    let dir_s = dir.display().to_string();
    if let Some(mode) = watch_mode_for(&dir) {
        match watcher.watch(&dir, mode) {
            Ok(()) => {
                *watched = Some(dir);
                tracing::info!("watching {dir_s} for changes");
            }
            Err(e) => {
                tracing::warn!("failed to watch {dir_s}: {e}");
            }
        }
    } else {
        *watched = Some(dir);
        tracing::info!("not watching pseudo-filesystem {dir_s}");
    }
}

fn normalize_dir(dir: PathBuf) -> PathBuf {
    let mut s = dir.display().to_string();
    while s.len() > 1 && s.ends_with('/') {
        s.pop();
    }
    PathBuf::from(s)
}

fn watch_mode_for(dir: &Path) -> Option<notify::RecursiveMode> {
    if dir == Path::new("/") {
        return Some(notify::RecursiveMode::NonRecursive);
    }
    let name = dir.file_name().and_then(|n| n.to_str());
    let is_system_pseudo = dir.parent() == Some(Path::new("/"))
        && matches!(name, Some("proc") | Some("sys") | Some("dev"));
    if is_system_pseudo {
        return None;
    }
    Some(notify::RecursiveMode::Recursive)
}

fn flush(pending: &mut BTreeSet<String>, tx: &broadcast::Sender<ChangeHint>) {
    for dir in pending.iter() {
        let _ = tx.send(ChangeHint::changed(dir.clone()));
    }
    pending.clear();
}

fn absorb(ev: &notify::Event, pending: &mut BTreeSet<String>, watched: Option<&Path>) {
    use notify::EventKind;
    match ev.kind {
        EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_) | EventKind::Any => {}
        _ => return,
    }
    let Some(watched) = watched else {
        return;
    };
    let watched_s = watched.to_string_lossy().replace('\\', "/");
    for p in &ev.paths {
        let s = p.to_string_lossy().replace('\\', "/");
        if parent_of_abs(&s) == watched_s {
            pending.insert(watched_s.clone());
        }
    }
}

/// Builds the router for the given state.
pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/info", get(info_json))
        .route("/icons/{theme}/{*path}", get(icon_handler))
        .route("/filetree", get(filetree_handler))
        .route("/filecontent", get(filecontent_handler))
        .route("/fileinfo", get(fileinfo_handler))
        .route("/filesearch", get(filesearch_handler))
        .route("/apps", get(apps_handler))
        .route("/open", post(open_handler))
        .route("/defaultapp", get(default_app_handler))
        .route("/ws", get(update_hint_handler))
        .route("/rename", post(rename_handler))
        .route("/delete", post(delete_handler))
        .route("/", get(static_files::serve_static))
        .route("/{*path}", get(static_files::serve_static))
        // Permissive CORS so embedded hosts (e.g. grit's localhost:5000 UI)
        // can probe, fetch, and WebSocket-upgrade cross-origin.
        .layer(CorsLayer::permissive())
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
    async fn info_reports_root_and_home() {
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
        assert_eq!(info.home, std::env::var("HOME").unwrap_or_default());
    }

    #[tokio::test]
    async fn filetree_lists_root_and_subdir() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src/sub")).unwrap();
        std::fs::write(dir.path().join("src/main.rs"), "fn main() {}\n").unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "[package]\n").unwrap();
        let root_s = dir.path().display().to_string();

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
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let entries: Vec<fs::FileTreeEntry> = serde_json::from_slice(&bytes).unwrap();
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert!(names.contains(&"src"), "got: {names:?}");
        assert!(names.contains(&"Cargo.toml"), "got: {names:?}");
        assert_eq!(names.len(), 2);
        assert!(entries.iter().any(|e| e.is_dir && e.path == format!("{root_s}/src")));
        assert!(entries
            .iter()
            .any(|e| !e.is_dir && e.path == format!("{root_s}/Cargo.toml")));

        let response = router
            .oneshot(
                Request::builder()
                    .uri(format!("/filetree?path={root_s}/src"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let entries: Vec<fs::FileTreeEntry> = serde_json::from_slice(&bytes).unwrap();
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert!(
            names.contains(&"sub") && names.contains(&"main.rs"),
            "got: {names:?}"
        );
        assert!(entries[0].is_dir, "dirs must come first: {names:?}");
    }

    #[tokio::test]
    async fn filetree_404_for_missing_path_and_400_for_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("f.txt"), "x").unwrap();
        let root_s = dir.path().display().to_string();
        let router = app_for(dir.path());

        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/filetree?path={root_s}/nope"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        let response = router
            .oneshot(
                Request::builder()
                    .uri(format!("/filetree?path={root_s}/f.txt"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

#[tokio::test]
    async fn icon_serves_theme_files_and_blocks_traversal() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let theme_root = dir.path().join("res").join("icons");
        std::fs::create_dir_all(theme_root.join("test-theme/mimetypes/96")).unwrap();
        let mut file = std::fs::File::create(theme_root.join("test-theme/mimetypes/96/x.svg")).unwrap();
        file.write_all(b"<svg></svg>").unwrap();

        let state = AppState::new(dir.path().to_path_buf());
        let mut state = state;
        // point themes_dir at the temp tree
        state.themes_dir = theme_root;
        let router = build_router(state);

        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/icons/test-theme/mimetypes/96/x.svg")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(response.into_body(), 1024)
            .await
            .unwrap();
        assert_eq!(&bytes[..], b"<svg></svg>");

        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/icons/test-theme/../../../../etc/passwd")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/icons/../other/thing.svg")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        let response = router
            .oneshot(
                Request::builder()
                    .uri("/icons/test-theme/mimetypes/96/missing.svg")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

#[tokio::test]
    async fn icon_falls_back_to_embedded_theme() {
        // Simulates a release build launched from a directory with no
        // res/icons on disk: themes_dir points at an empty tree, so the
        // bundled breeze-dark theme must serve from the embedded copy.
        let dir = tempfile::tempdir().unwrap();
        let theme_root = dir.path().join("res").join("icons");
        std::fs::create_dir_all(&theme_root).unwrap();

        let mut state = AppState::new(dir.path().to_path_buf());
        state.themes_dir = theme_root;
        let router = build_router(state);

        let response = router
            .oneshot(
                Request::builder()
                    .uri("/icons/breeze-dark/places/96/folder.svg")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let header = response.headers().get(header::CONTENT_TYPE).unwrap();
        assert_eq!(header, "image/svg+xml");
        let bytes = axum::body::to_bytes(response.into_body(), 1024 * 64)
            .await
            .unwrap();
        assert!(!bytes.is_empty());
        assert!(String::from_utf8_lossy(&bytes).contains("<svg"));
    }

#[test]
    fn parent_of_abs_reduces_to_parent_dir() {
        assert_eq!(parent_of_abs("/etc/x"), "/etc");
        assert_eq!(parent_of_abs("/etc/"), "/");
        assert_eq!(parent_of_abs("/etc"), "/");
        assert_eq!(parent_of_abs("/"), "/");
        assert_eq!(parent_of_abs("/a/b/c"), "/a/b");
    }

    #[test]
    fn absorb_only_keeps_events_in_watched_dir() {
        fn ev(path: &str) -> notify::Event {
            notify::Event::new(notify::EventKind::Create(notify::event::CreateKind::File))
                .add_path(std::path::PathBuf::from(path))
        }
        let root = Path::new("/home/u/watchdir");
        let mut pending = BTreeSet::new();

        // direct child change -> hint for the watched dir
        absorb(&ev("/home/u/watchdir/a.txt"), &mut pending, Some(root));
        assert_eq!(pending.len(), 1);
        assert!(pending.contains("/home/u/watchdir"));

        // deep change under a subdir -> ignored (parent != watched dir)
        absorb(
            &ev("/home/u/watchdir/sub/b.txt"),
            &mut pending,
            Some(root),
        );
        assert_eq!(pending.len(), 1, "deep events must not surface");

        // change outside the watched dir -> ignored
        absorb(&ev("/home/u/other/c.txt"), &mut pending, Some(root));
        assert_eq!(pending.len(), 1);
    }

    #[test]
    fn watch_mode_skips_noise_dirs() {
        assert_eq!(
            watch_mode_for(Path::new("/")),
            Some(notify::RecursiveMode::NonRecursive)
        );
        assert_eq!(watch_mode_for(Path::new("/proc")), None);
        assert_eq!(watch_mode_for(Path::new("/sys")), None);
        assert_eq!(watch_mode_for(Path::new("/dev")), None);
        assert_eq!(
            watch_mode_for(Path::new("/home/u")),
            Some(notify::RecursiveMode::Recursive)
        );
        // a regular "proc" folder elsewhere still gets watched
        assert_eq!(
            watch_mode_for(Path::new("/home/u/proc")),
            Some(notify::RecursiveMode::Recursive)
        );
    }
}