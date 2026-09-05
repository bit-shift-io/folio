//! Integration tests: boot the real folio router on an ephemeral port and
//! exercise HTTP + WebSocket hint channels against a throwaway root dir.

use std::time::Duration;

use folio::server::{self, AppState};
use futures_util::StreamExt;
use tokio_tungstenite::tungstenite::Message;

async fn setup() -> (tempfile::TempDir, String, tokio::task::JoinHandle<Result<(), std::io::Error>>) {
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

async fn http_get(addr: &str, path: &str) -> (u16, String) {
    let res = reqwest::Client::new()
        .get(format!("http://{addr}{path}"))
        .send()
        .await
        .unwrap();
    (res.status().as_u16(), res.text().await.unwrap())
}

async fn http_post(addr: &str, path: &str, body: &str) -> (u16, String) {
    let res = reqwest::Client::new()
        .post(format!("http://{addr}{path}"))
        .header("content-type", "application/json")
        .body(body.to_string())
        .send()
        .await
        .unwrap();
    (res.status().as_u16(), res.text().await.unwrap())
}

#[tokio::test]
async fn info_and_filetree_serve_the_root() {
    let (_dir, addr, _handle) = setup().await;

    let (status, body) = http_get(&addr, "/info").await;
    assert_eq!(status, 200);
    assert!(body.contains("root"));

    let (status, body) = http_get(&addr, "/filetree?path=").await;
    assert_eq!(status, 200);
    let entries: Vec<serde_json::Value> = serde_json::from_str(&body).unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0]["is_dir"], true); // inner/ sorted before hello.txt
    assert_eq!(entries[0]["name"], "inner");
}

#[tokio::test]
async fn filecontent_and_search_work() {
    let (_dir, addr, _handle) = setup().await;

    let (status, body) = http_get(&addr, "/filecontent?path=hello.txt").await;
    assert_eq!(status, 200);
    assert!(body.contains("\"content\":\"hi\""));

    let (status, _) = http_get(&addr, "/filecontent?path=../../etc/passwd").await;
    assert_eq!(status, 400);

    let (status, body) = http_get(&addr, "/filesearch?q=deep").await;
    assert_eq!(status, 200);
    assert!(body.contains("inner/deep.md"));
}

#[tokio::test]
async fn rename_and_delete_mutate_and_broadcast_hints() {
    let (_dir, addr, _handle) = setup().await;

    let (status, body) = http_post(&addr, "/rename", r#"{"path":"hello.txt","to":"renamed.txt"}"#).await;
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
async fn websocket_pushes_change_hints() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("inner")).unwrap();
    std::fs::write(dir.path().join("inner").join("seed.txt"), "x").unwrap();
    let root = dir.path().to_string_lossy().into_owned();

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

    std::fs::write(dir.path().join("inner").join("created.txt"), "new").unwrap();

    let mut saw = false;
    for _ in 0..20 {
        match tokio::time::timeout(Duration::from_millis(500), ws.next()).await {
Ok(Some(Ok(Message::Text(text)))) => {
                    if text.to_string().contains(r#""path":"inner""#) {
                    saw = true;
                    break;
                }
            }
            Ok(Some(Ok(Message::Ping(_)))) => continue,
            _ => break,
        }
    }
    assert!(saw, "expected a change hint for path \"inner\"");
}