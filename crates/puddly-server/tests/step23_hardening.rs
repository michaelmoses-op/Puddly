//! Step 23: production hardening — SSE drain, health/ready stubs, metrics, env (ADR-041).

use puddly_check::check;
use puddly_db::RecordingExecutor;
use puddly_rpc::RpcServer;
use puddly_server::{
    dispatch_http, resolve_serve_env, stream_sse, validate_database_url, HttpRequest, Metrics,
    SseHub, SSE_SHUTDOWN_FRAME,
};
use serde_json::Value;
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

#[test]
fn env_validation_rejects_bad_urls() {
    assert!(validate_database_url("postgres://localhost/db").is_ok());
    assert!(validate_database_url("http://localhost/db").is_err());
    assert!(resolve_serve_env(None, false).is_err());
    let env = resolve_serve_env(
        Some("postgres://puddly:puddly@127.0.0.1:5432/puddly".into()),
        false,
    )
    .expect("ok");
    assert!(!env.secure_cookies);
}

#[tokio::test]
async fn health_payload_includes_service() {
    let rpc = Mutex::new(load_rpc());
    let mut exec = RecordingExecutor::new();
    let req = HttpRequest {
        method: "GET".into(),
        path: "/health".into(),
        headers: BTreeMap::new(),
        body: Vec::new(),
    };
    let resp = dispatch_http(&req, &rpc, &mut exec, None, None)
        .await
        .unwrap();
    assert_eq!(resp.status, 200);
    let v: Value = serde_json::from_slice(&resp.body).unwrap();
    assert_eq!(v["ok"], true);
    assert_eq!(v["live"], true);
    assert!(v["service"].as_str().is_some());
}

#[tokio::test]
async fn ready_via_dispatch_without_pool_is_503() {
    let rpc = Mutex::new(load_rpc());
    let mut exec = RecordingExecutor::new();
    let req = HttpRequest {
        method: "GET".into(),
        path: "/ready".into(),
        headers: BTreeMap::new(),
        body: Vec::new(),
    };
    let resp = dispatch_http(&req, &rpc, &mut exec, None, None)
        .await
        .unwrap();
    assert_eq!(resp.status, 503);
    let v: Value = serde_json::from_slice(&resp.body).unwrap();
    assert_eq!(v["ready"], false);
}

#[tokio::test]
async fn metrics_snapshot_shape() {
    let metrics = Metrics::shared();
    metrics.inc_http();
    metrics.inc_rpc_ok();
    let hub = SseHub::new();
    let snap = metrics.snapshot(Some(&hub), None);
    assert_eq!(snap["http_requests"], 1);
    assert_eq!(snap["rpc_ok"], 1);
    assert_eq!(snap["sse_active"], 0);
    assert_eq!(snap["ok"], true);
}

#[tokio::test]
async fn sse_drain_ends_stream() {
    let hub = SseHub::shared();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let hub_accept = Arc::clone(&hub);

    tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut buf = vec![0u8; 4096];
        let _ = stream.read(&mut buf).await;
        stream_sse(&mut stream, &hub_accept, None).await.unwrap();
    });

    let mut client = TcpStream::connect(addr).await.unwrap();
    client
        .write_all(b"GET /events HTTP/1.1\r\nHost: localhost\r\n\r\n")
        .await
        .unwrap();

    // Wait until the server has acquired a connection guard.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    while hub.active_connections() == 0 && tokio::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(hub.active_connections(), 1);

    hub.begin_shutdown();
    assert!(hub.wait_drained(Duration::from_secs(2)).await);

    let mut got = Vec::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    while tokio::time::Instant::now() < deadline {
        let mut chunk = [0u8; 512];
        match tokio::time::timeout(Duration::from_millis(200), client.read(&mut chunk)).await {
            Ok(Ok(0)) => break,
            Ok(Ok(n)) => {
                got.extend_from_slice(&chunk[..n]);
                let s = String::from_utf8_lossy(&got);
                if s.contains("puddly-shutdown") || s.contains(SSE_SHUTDOWN_FRAME.trim()) {
                    break;
                }
            }
            _ => break,
        }
    }
    let text = String::from_utf8_lossy(&got);
    assert!(
        text.contains("puddly-shutdown") || text.contains(": puddly-shutdown"),
        "expected shutdown frame, got: {text}"
    );
    assert!(hub.try_acquire().is_none());
}
