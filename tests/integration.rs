//! Integration tests: boot the real folio router on an ephemeral port and
//! exercise HTTP + WebSocket hint channels against a throwaway root dir.

use std::time::Duration;

use folio::server::{self, AppState};
use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::Message;

async fn setup() -> (
    tempfile::TempDir,
    String,
    tokio::task::JoinHandle<Result<(), std::io::Error>>,
) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("hello.txt"), "hi").unwrap();
    std::fs::create_dir(dir.path().join("inner")).unwrap();
    std::fs::write(dir.path().join("inner").join("deep.md"), "# deep").unwrap();

    let root = dir.path().to_string_lossy().into_owned();
    let state = AppState::new(std::path::PathBuf::from(&root));
    state.spawn_watcher();
    let app = server::build_router(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let handle = tokio::spawn(async move { axum::serve(listener, app).await });
    (dir, format!("127.0.0.1:{port}"), handle)
}

/// Sends a minimal HTTP/1.1 request over a raw `TcpStream` and reads the
/// response to EOF. Returns `(status_code, body)`. Deliberately hand-rolled:
/// the integration tests only ever talk to folio on loopback, and an HTTP
/// client crate would dwarf the thing under test.
async fn http_request(addr: &str, method: &str, path: &str, body: Option<&str>) -> (u16, String) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    let mut req = format!("{method} {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n");
    match body {
        Some(b) => {
            req.push_str("Content-Type: application/json\r\n");
            req.push_str(&format!("Content-Length: {}\r\n", b.len()));
        }
        None => req.push_str("Content-Length: 0\r\n"),
    }
    req.push_str("\r\n");
    if let Some(b) = body {
        req.push_str(b);
    }
    stream.write_all(req.as_bytes()).await.unwrap();
    stream.flush().await.unwrap();

    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).await.unwrap();
    let text = String::from_utf8_lossy(&raw).into_owned();

    // Split the status line off the body: "HTTP/1.1 200 OK" → 200.
    let (head, body) = text
        .split_once("\r\n\r\n")
        .unwrap_or_else(|| panic!("malformed response for {path}: {text:?}"));
    let status = head
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse().ok())
        .unwrap_or_else(|| panic!("no status line for {path}: {head:?}"));
    (status, body.to_string())
}

async fn http_get(addr: &str, path: &str) -> (u16, String) {
    http_request(addr, "GET", path, None).await
}

async fn http_post(addr: &str, path: &str, body: &str) -> (u16, String) {
    http_request(addr, "POST", path, Some(body)).await
}

#[tokio::test]
async fn info_and_filetree_serve_the_root_absolutely() {
    let (dir, addr, _handle) = setup().await;
    let root_s = dir.path().display().to_string();

    let (status, body) = http_get(&addr, "/info").await;
    assert_eq!(status, 200);
    assert!(body.contains(&format!("\"root\":\"{root_s}\"")));

    let (status, body) = http_get(&addr, "/filetree?path=").await;
    assert_eq!(status, 200);
    let entries: Vec<serde_json::Value> = serde_json::from_str(&body).unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0]["is_dir"], true); // inner/ sorted before hello.txt
    assert_eq!(entries[0]["name"], "inner");
    assert_eq!(entries[0]["path"], format!("{root_s}/inner"));

    // A relative path resolves against the initial root.
    let (status, body) = http_get(&addr, "/filetree?path=inner").await;
    assert_eq!(status, 200);
    let entries: Vec<serde_json::Value> = serde_json::from_str(&body).unwrap();
    assert_eq!(entries[0]["path"], format!("{root_s}/inner/deep.md"));
}

#[tokio::test]
async fn filecontent_and_search_work() {
    let (dir, addr, _handle) = setup().await;
    let root_s = dir.path().display().to_string();

    let (status, body) = http_get(&addr, "/filecontent?path=hello.txt").await;
    assert_eq!(status, 200);
    assert!(body.contains("\"content\":\"hi\""));

    // Missing files: raw reads 404, folded reads report the error inline.
    let (status, _) = http_get(
        &addr,
        &format!("/filecontent?path={root_s}/nope.txt&raw=true"),
    )
    .await;
    assert_eq!(status, 404);
    let (status, body) = http_get(&addr, "/filecontent?path=nope.txt").await;
    assert_eq!(status, 200);
    assert!(body.contains("error"));

    let (status, body) = http_get(&addr, "/filesearch?q=deep").await;
    assert_eq!(status, 200);
    assert!(body.contains(&format!("{root_s}/inner/deep.md")));
}

#[tokio::test]
async fn rename_and_delete_mutate_and_broadcast_hints() {
    let (_dir, addr, _handle) = setup().await;

    let (status, body) = http_post(
        &addr,
        "/rename",
        r#"{"path":"hello.txt","to":"renamed.txt"}"#,
    )
    .await;
    assert_eq!(status, 200, "{body}");

    let (status, body) = http_get(&addr, "/filetree?path=").await;
    assert_eq!(status, 200);
    assert!(body.contains("renamed.txt"));
    assert!(!body.contains("hello.txt"));

    let (status, _) = http_post(&addr, "/delete", r#"{"path":"renamed.txt"}"#).await;
    assert_eq!(status, 200);

    let (status, body) = http_get(&addr, "/filetree?path=").await;
    assert_eq!(status, 200);
    assert!(!body.contains("renamed.txt"));
}

#[tokio::test]
async fn websocket_pushes_change_hints_with_absolute_paths() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("inner")).unwrap();
    std::fs::write(dir.path().join("inner").join("seed.txt"), "x").unwrap();
    let root = dir.path().to_string_lossy().into_owned();
    let root_s = dir.path().display().to_string();

    let state = AppState::new(std::path::PathBuf::from(&root));
    state.spawn_watcher();
    let app = server::build_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let _server = tokio::spawn(async move { axum::serve(listener, app).await });

    let url = format!("ws://{addr}/ws");
    let (mut ws, _) = tokio_tungstenite::connect_async(&url).await.unwrap();

    while let Ok(msg) = tokio::time::timeout(Duration::from_millis(50), ws.next()).await {
        match msg {
            Some(Ok(Message::Text(_))) => continue,
            Some(Ok(Message::Ping(_))) => continue,
            _ => break,
        }
    }

    ws.send(Message::Text(
        format!(r#"{{"type":"watch","path":"{root_s}/inner"}}"#).into(),
    ))
    .await
    .unwrap();
    tokio::time::sleep(Duration::from_millis(150)).await;

    std::fs::write(dir.path().join("inner").join("created.txt"), "new").unwrap();

    let expected = format!(r#""path":"{root_s}/inner""#);
    let mut saw = false;
    for _ in 0..20 {
        match tokio::time::timeout(Duration::from_millis(500), ws.next()).await {
            Ok(Some(Ok(Message::Text(text)))) => {
                if text.to_string().contains(&expected) {
                    saw = true;
                    break;
                }
            }
            Ok(Some(Ok(Message::Ping(_)))) => continue,
            _ => break,
        }
    }
    assert!(saw, "expected a change hint for path {expected:?}");
}

#[tokio::test]
async fn websocket_watch_message_retargets_the_watcher() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("dirA")).unwrap();
    std::fs::create_dir(dir.path().join("dirB")).unwrap();
    let root = dir.path().to_string_lossy().into_owned();
    let root_s = dir.path().display().to_string();

    let state = AppState::new(std::path::PathBuf::from(&root));
    state.spawn_watcher();
    let app = server::build_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let _server = tokio::spawn(async move { axum::serve(listener, app).await });

    let url = format!("ws://{addr}/ws");
    let (mut ws, _) = tokio_tungstenite::connect_async(&url).await.unwrap();

    while let Ok(msg) = tokio::time::timeout(Duration::from_millis(50), ws.next()).await {
        match msg {
            Some(Ok(Message::Text(_))) => continue,
            Some(Ok(Message::Ping(_))) => continue,
            _ => break,
        }
    }

    ws.send(Message::Text(
        format!(r#"{{"type":"watch","path":"{root_s}/dirB"}}"#).into(),
    ))
    .await
    .unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;

    std::fs::write(dir.path().join("dirB").join("created.txt"), "new").unwrap();

    let expected = format!(r#""path":"{root_s}/dirB""#);
    let mut saw = false;
    for _ in 0..20 {
        match tokio::time::timeout(Duration::from_millis(500), ws.next()).await {
            Ok(Some(Ok(Message::Text(text)))) => {
                if text.to_string().contains(&expected) {
                    saw = true;
                    break;
                }
            }
            Ok(Some(Ok(Message::Ping(_)))) => continue,
            _ => break,
        }
    }
    assert!(saw, "expected a change hint for watched dirB");

    std::fs::write(dir.path().join("dirA").join("untracked.txt"), "x").unwrap();
    if let Ok(Some(Ok(Message::Text(text)))) =
        tokio::time::timeout(Duration::from_millis(500), ws.next()).await
    {
        assert!(
            !text.to_string().contains("dirA"),
            "dirA was unwatched, got hint: {text}"
        );
    }
}
