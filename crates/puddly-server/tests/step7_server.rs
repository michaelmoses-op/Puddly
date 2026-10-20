//! Step 7: HTTP adapter + list owner WHERE injection (RecordingExecutor).

use puddly_auth::SessionStore;
use puddly_check::check;
use puddly_core::{CSRF_HEADER_NAME, SESSION_COOKIE_NAME};
use puddly_db::{Executor, RecordingExecutor, SqlValue};
use puddly_rpc::RpcServer;
use puddly_server::{dispatch_http, parse_http_request_bytes, HttpRequest};
use serde_json::json;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

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
fn parses_http_post_rpc() {
    let raw = b"\
POST /rpc/todo.list HTTP/1.1\r\n\
Host: localhost\r\n\
Content-Type: application/json\r\n\
Content-Length: 2\r\n\
\r\n\
{}";
    let req = parse_http_request_bytes(raw).unwrap();
    assert_eq!(req.method, "POST");
    assert_eq!(req.path, "/rpc/todo.list");
    assert_eq!(req.body, b"{}");
}

#[test]
fn health_endpoint() {
    let rpc = Mutex::new(load_rpc());
    let exec: Mutex<Box<dyn Executor + Send>> =
        Mutex::new(Box::new(RecordingExecutor::new()));
    let req = HttpRequest {
        method: "GET".into(),
        path: "/health".into(),
        headers: BTreeMap::new(),
        body: Vec::new(),
    };
    let resp = dispatch_http(&req, &rpc, &exec).unwrap();
    assert_eq!(resp.status, 200);
}

#[test]
fn http_list_injects_owner_where() {
    let mut server = load_rpc();
    let user = puddly_auth::AuthUser::new("owner-1", "o@x.co");
    let token = server.sessions.create(&user, 3600).unwrap();

    let rpc = Mutex::new(server);
    let mut recording = RecordingExecutor::new();
    recording.push_canned(vec![]);
    let exec: Mutex<Box<dyn Executor + Send>> = Mutex::new(Box::new(recording));

    let mut headers = BTreeMap::new();
    headers.insert(
        "cookie".into(),
        format!("{SESSION_COOKIE_NAME}={token}"),
    );
    let req = HttpRequest {
        method: "POST".into(),
        path: "/rpc/todo.list".into(),
        headers,
        body: b"{}".to_vec(),
    };
    let resp = dispatch_http(&req, &rpc, &exec).unwrap();
    assert_eq!(resp.status, 200, "{:?}", String::from_utf8_lossy(&resp.body));

    // Re-run via RpcServer to assert SQL (RecordingExecutor consumed inside mutex).
    let mut server = load_rpc();
    let token = {
        let user = puddly_auth::AuthUser::new("owner-1", "o@x.co");
        server.sessions.create(&user, 3600).unwrap()
    };
    let mut exec = RecordingExecutor::new();
    exec.push_canned(vec![]);
    let req = puddly_rpc::RpcRequest::new("todo.list", json!({}))
        .with_cookie(format!("{SESSION_COOKIE_NAME}={token}"));
    let resp = server.handle(&req, &mut exec);
    assert!(resp.ok, "{resp:?}");
    assert!(
        exec.log.iter().any(|q| {
            q.sql.contains("WHERE")
                && q.sql.contains("\"owner\"")
                && q.params == vec![SqlValue::Uuid("owner-1".into())]
        }),
        "expected owner WHERE predicate, got {:?}",
        exec.log
    );
}

#[test]
fn register_generates_user_id_when_omitted() {
    let mut server = load_rpc();
    let mut exec = RecordingExecutor::new();
    exec.push_canned(vec![]); // no existing email
    let req = puddly_rpc::RpcRequest::new(
        "auth.register",
        json!({
            "email": "new@example.com",
            "password": "password123"
        }),
    )
    .with_header(CSRF_HEADER_NAME, "1");
    let resp = server.handle(&req, &mut exec);
    assert!(resp.ok, "{resp:?}");
    let id = resp.data.as_ref().unwrap()["user"]["id"].as_str().unwrap();
    assert_eq!(id.len(), 36);
    assert!(id.contains('-'));
    assert!(!resp.set_cookies.is_empty());
}
