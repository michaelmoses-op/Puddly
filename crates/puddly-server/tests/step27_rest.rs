//! Step 27: opt-in REST + OpenAPI (ADR-045).

use puddly_auth::{AuthUser, SessionStore};
use puddly_check::check;
use puddly_core::{CSRF_HEADER_NAME, SESSION_COOKIE_NAME};
use puddly_db::{DbRow, RecordingExecutor, SqlValue};
use puddly_rpc::{generate_openapi, map_rest_to_rpc};
use puddly_server::{dispatch_http, dispatch_http_rest, HttpRequest};
use serde_json::json;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Mutex;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

fn load_rpc() -> puddly_rpc::RpcServer {
    let root = workspace_root().join("examples/todo-slice");
    let ir = check(&root).ir.expect("ir");
    puddly_rpc::RpcServer::from_app(&root, ir).expect("rpc")
}

fn canned_todo() -> DbRow {
    let mut row = DbRow::new();
    row.insert(
        "id",
        SqlValue::Uuid("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee".into()),
    );
    row.insert("title", SqlValue::Text("REST milk".into()));
    row.insert("done", SqlValue::Bool(false));
    row.insert("dueDate", SqlValue::Null);
    row.insert(
        "project",
        SqlValue::Uuid("11111111-1111-1111-1111-111111111111".into()),
    );
    row.insert(
        "owner",
        SqlValue::Uuid("22222222-2222-2222-2222-222222222222".into()),
    );
    row.insert("createdAt", SqlValue::Text("2026-01-01T00:00:00Z".into()));
    row
}

#[test]
fn openapi_contains_todo_paths() {
    let root = workspace_root().join("examples/todo-slice");
    let ir = check(&root).ir.unwrap();
    let doc = generate_openapi(&ir);
    assert_eq!(doc["openapi"], "3.0.3");
    let paths = doc["paths"].as_object().unwrap();
    assert!(paths.contains_key("/api/todo"));
    assert!(paths.contains_key("/api/todo/{id}"));
    assert!(paths.contains_key("/api/todo/archive"));
    assert!(paths.contains_key("/api/auth/login"));
    let desc = doc["info"]["description"].as_str().unwrap();
    assert!(desc.contains("X-Puddly-CSRF"));
}

#[test]
fn rest_mapping_helpers() {
    let m = map_rest_to_rpc("GET", "/api/todo", json!({})).unwrap();
    assert_eq!(m.target, "todo.list");
}

#[tokio::test]
async fn rest_list_requires_session_like_rpc() {
    let mut server = load_rpc();
    let user = AuthUser::new(
        "22222222-2222-2222-2222-222222222222",
        "r@x.co",
    );
    let token = server.sessions.create(&user, 3600).await.unwrap();
    let rpc = Arc::new(Mutex::new(server));

    let mut exec = RecordingExecutor::new();
    exec.push_canned(vec![canned_todo()]);

    let mut headers = BTreeMap::new();
    headers.insert("cookie".into(), format!("{SESSION_COOKIE_NAME}={token}"));
    let req = HttpRequest {
        method: "GET".into(),
        path: "/api/todo".into(),
        headers,
        body: Vec::new(),
    };
    let resp = dispatch_http_rest(&req, rpc.as_ref(), &mut exec, None, None)
        .await
        .unwrap();
    assert_eq!(resp.status, 200, "{}", String::from_utf8_lossy(&resp.body));
    let body: serde_json::Value = serde_json::from_slice(&resp.body).unwrap();
    assert!(body["ok"].as_bool().unwrap());
    assert_eq!(body["data"]["items"][0]["title"], "REST milk");
}

#[tokio::test]
async fn rest_create_requires_csrf() {
    let server = load_rpc();
    let rpc = Arc::new(Mutex::new(server));
    let mut exec = RecordingExecutor::new();
    let req = HttpRequest {
        method: "POST".into(),
        path: "/api/todo".into(),
        headers: BTreeMap::new(),
        body: serde_json::to_vec(&json!({"title": "x"})).unwrap(),
    };
    let resp = dispatch_http_rest(&req, rpc.as_ref(), &mut exec, None, None)
        .await
        .unwrap();
    assert_eq!(resp.status, 400);
    let body: serde_json::Value = serde_json::from_slice(&resp.body).unwrap();
    assert!(!body["ok"].as_bool().unwrap());
    assert!(body["error"]["message"]
        .as_str()
        .unwrap()
        .to_ascii_lowercase()
        .contains("csrf"));
}

#[tokio::test]
async fn rest_disabled_does_not_map_api() {
    let server = load_rpc();
    let rpc = Arc::new(Mutex::new(server));
    let mut exec = RecordingExecutor::new();
    let req = HttpRequest {
        method: "GET".into(),
        path: "/api/todo".into(),
        headers: BTreeMap::new(),
        body: Vec::new(),
    };
    // dispatch_http keeps rest=false → 404, not list
    let resp = dispatch_http(&req, rpc.as_ref(), &mut exec, None, None)
        .await
        .unwrap();
    assert_eq!(resp.status, 404);
    let _ = CSRF_HEADER_NAME;
}

#[tokio::test]
async fn openapi_via_dispatch_rest() {
    let server = load_rpc();
    let rpc = Arc::new(Mutex::new(server));
    let mut exec = RecordingExecutor::new();
    let req = HttpRequest {
        method: "GET".into(),
        path: "/openapi.json".into(),
        headers: BTreeMap::new(),
        body: Vec::new(),
    };
    let resp = dispatch_http_rest(&req, rpc.as_ref(), &mut exec, None, None)
        .await
        .unwrap();
    assert_eq!(resp.status, 200);
    let body: serde_json::Value = serde_json::from_slice(&resp.body).unwrap();
    assert_eq!(body["openapi"], "3.0.3");
}
