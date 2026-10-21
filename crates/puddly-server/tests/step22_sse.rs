//! Step 22: SSE hub + GET /events + mutation publish (RecordingExecutor).

use puddly_auth::SessionStore;
use puddly_check::check;
use puddly_core::{CSRF_COOKIE_NAME, CSRF_HEADER_NAME, SESSION_COOKIE_NAME};
use puddly_db::RecordingExecutor;
use puddly_rpc::RpcServer;
use puddly_server::{
    dispatch_http, stream_sse, write_sse_headers, HttpRequest, SseEvent, SseHub, SSE_KEEPALIVE,
};
use puddly_views::ISLANDS_RUNTIME_JS;
use serde_json::json;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

fn load_rpc() -> RpcServer {
    let root = workspace_root().join("examples/todo-slice");
    let ir = check(&root).ir.expect("ir");
    RpcServer::from_app(&root, ir).expect("rpc")
}

#[tokio::test]
async fn hub_publish_subscribe_unit() {
    let hub = SseHub::new();
    let mut rx = hub.subscribe();
    let event = SseEvent::new("todo", "create", json!({ "id": "t1", "title": "Hi" }));
    assert_eq!(event.type_name, "todo.created");
    hub.publish(&event);
    let frame = rx.recv().await.unwrap();
    assert!(frame.contains("todo.created"));
    assert!(frame.starts_with("data: "));
}

#[tokio::test]
async fn events_endpoint_streams_comment_and_event() {
    let hub = Arc::new(SseHub::new());
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let hub_accept = Arc::clone(&hub);

    tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut buf = vec![0u8; 4096];
        let _ = stream.read(&mut buf).await;
        // Minimal: ignore request body; just stream SSE.
        stream_sse(&mut stream, &hub_accept, None).await.unwrap();
    });

    let mut client = TcpStream::connect(addr).await.unwrap();
    client
        .write_all(b"GET /events HTTP/1.1\r\nHost: localhost\r\n\r\n")
        .await
        .unwrap();

    // Read headers + first keep-alive comment.
    let mut got = Vec::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    while tokio::time::Instant::now() < deadline {
        let mut chunk = [0u8; 512];
        match tokio::time::timeout(Duration::from_millis(200), client.read(&mut chunk)).await {
            Ok(Ok(0)) => break,
            Ok(Ok(n)) => {
                got.extend_from_slice(&chunk[..n]);
                let s = String::from_utf8_lossy(&got);
                if s.contains("text/event-stream") && s.contains(": keep-alive") {
                    break;
                }
            }
            _ => break,
        }
    }
    let header_text = String::from_utf8_lossy(&got);
    assert!(
        header_text.contains("text/event-stream"),
        "headers: {header_text}"
    );
    assert!(
        header_text.contains(SSE_KEEPALIVE.trim_end()) || header_text.contains(": keep-alive"),
        "expected keep-alive comment, got: {header_text}"
    );

    hub.publish(&SseEvent::new("todo", "update", json!({ "id": "x", "done": true })));

    let mut event_buf = got.clone();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    while tokio::time::Instant::now() < deadline {
        let mut chunk = [0u8; 512];
        match tokio::time::timeout(Duration::from_millis(300), client.read(&mut chunk)).await {
            Ok(Ok(0)) => break,
            Ok(Ok(n)) => {
                event_buf.extend_from_slice(&chunk[..n]);
                let s = String::from_utf8_lossy(&event_buf);
                if s.contains("todo.updated") {
                    break;
                }
            }
            _ => break,
        }
    }
    let body = String::from_utf8_lossy(&event_buf);
    assert!(body.contains("todo.updated"), "stream: {body}");
    assert!(body.contains("data: "), "stream: {body}");
}

#[tokio::test]
async fn mutation_publishes_via_dispatch() {
    let mut server = load_rpc();
    let user = puddly_auth::AuthUser::new(
        "00000000-0000-4000-8000-000000000099",
        "live@example.com",
    );
    let token = server.sessions.create(&user, 3600).await.unwrap();

    let hub = SseHub::new();
    let mut rx = hub.subscribe();

    let rpc = Mutex::new(server);
    let mut exec = RecordingExecutor::new();

    let mut headers = BTreeMap::new();
    headers.insert(
        "cookie".into(),
        format!("{SESSION_COOKIE_NAME}={token}; {CSRF_COOKIE_NAME}=1"),
    );
    headers.insert(CSRF_HEADER_NAME.to_ascii_lowercase(), "1".into());
    let req = HttpRequest {
        method: "POST".into(),
        path: "/rpc/todo.create".into(),
        headers,
        body: serde_json::to_vec(&json!({
            "title": "Live todo",
            "project": "00000000-0000-4000-8000-000000000001"
        }))
        .unwrap(),
    };

    let resp = dispatch_http(&req, &rpc, &mut exec, None, Some(&hub))
        .await
        .unwrap();
    assert_eq!(resp.status, 200, "{:?}", String::from_utf8_lossy(&resp.body));

    let frame = tokio::time::timeout(Duration::from_secs(1), rx.recv())
        .await
        .expect("timeout waiting for SSE")
        .expect("recv");
    assert!(frame.contains("todo.created"), "frame: {frame}");
    assert!(frame.contains("\"op\":\"create\""), "frame: {frame}");
    assert!(frame.contains("\"id\""), "frame: {frame}");
}

#[test]
fn islands_js_contains_event_source_and_subscribe_live() {
    assert!(
        ISLANDS_RUNTIME_JS.contains("EventSource"),
        "islands runtime must reference EventSource"
    );
    assert!(
        ISLANDS_RUNTIME_JS.contains("subscribeLive"),
        "islands runtime must export subscribeLive"
    );
    assert!(ISLANDS_RUNTIME_JS.contains("/events"));
    assert!(ISLANDS_RUNTIME_JS.contains("data-puddly-live"));
}

#[tokio::test]
async fn write_sse_headers_content_type() {
    let mut buf: Vec<u8> = Vec::new();
    write_sse_headers(&mut buf).await.unwrap();
    let s = String::from_utf8(buf).unwrap();
    assert!(s.starts_with("HTTP/1.1 200 OK"));
    assert!(s.contains("text/event-stream"));
    assert!(!s.to_ascii_lowercase().contains("content-length"));
}
