//! Step 11 (adjusted Step 14): GET SSR needs load+auth; islands JS still sync.

use puddly_auth::{AuthUser, SessionStore};
use puddly_check::check;
use puddly_core::SESSION_COOKIE_NAME;
use puddly_db::{DbRow, RecordingExecutor, SqlValue};
use puddly_rpc::RpcServer;
use puddly_server::{dispatch_http, try_dispatch_get, HttpRequest};
use puddly_views::ViewBundle;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use tokio::sync::Mutex;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

fn example_root() -> PathBuf {
    workspace_root().join("examples/todo-slice")
}

fn load_rpc() -> RpcServer {
    let root = example_root();
    let ir = check(&root).ir.expect("ir");
    RpcServer::from_app(&root, ir).expect("rpc")
}

#[test]
fn get_islands_runtime_js() {
    let req = HttpRequest {
        method: "GET".into(),
        path: "/puddly-islands.js".into(),
        headers: BTreeMap::new(),
        body: Vec::new(),
    };
    let resp = try_dispatch_get(&req, None).unwrap().unwrap();
    assert_eq!(resp.status, 200);
    let body = String::from_utf8_lossy(&resp.body);
    assert!(body.contains("data-puddly-island"));
    // Step 16: mutation helper ships in the same runtime.
    assert!(body.contains("X-Puddly-CSRF"));
    assert!(body.contains("credentials"));
}

#[tokio::test]
async fn dispatch_get_v_todo_with_session_and_canned_rows() {
    let mut rpc = load_rpc();
    let user = AuthUser::new("owner-1", "o@x.co");
    let token = rpc.sessions.create(&user, 3600).await.unwrap();
    let rpc = Mutex::new(rpc);

    let mut exec = RecordingExecutor::new();
    let mut row = DbRow::new();
    row.insert("id", SqlValue::Uuid("t1".into()));
    row.insert("title", SqlValue::Text("Step11 milk".into()));
    row.insert("done", SqlValue::Bool(false));
    row.insert("owner", SqlValue::Uuid("owner-1".into()));
    exec.push_canned(vec![row]);

    let views = ViewBundle::load(&example_root()).unwrap();
    let mut headers = BTreeMap::new();
    headers.insert(
        "cookie".into(),
        format!("{SESSION_COOKIE_NAME}={token}"),
    );
    let req = HttpRequest {
        method: "GET".into(),
        path: "/v/todo".into(),
        headers,
        body: Vec::new(),
    };
    let resp = dispatch_http(&req, &rpc, &mut exec, Some(&views), None)
        .await
        .unwrap();
    assert_eq!(resp.status, 200);
    let body = String::from_utf8_lossy(&resp.body);
    assert!(body.contains("Todos"));
    assert!(body.contains("Step11 milk"), "{body}");
    assert!(!exec.log.is_empty(), "SSR load hits DB");
}

#[tokio::test]
async fn anonymous_root_is_401_not_fixture() {
    let rpc = Mutex::new(load_rpc());
    let mut exec = RecordingExecutor::new();
    let views = ViewBundle::load(&example_root()).unwrap();
    let req = HttpRequest {
        method: "GET".into(),
        path: "/".into(),
        headers: BTreeMap::new(),
        body: Vec::new(),
    };
    let resp = dispatch_http(&req, &rpc, &mut exec, Some(&views), None)
        .await
        .unwrap();
    assert_eq!(resp.status, 401);
    assert!(!String::from_utf8_lossy(&resp.body).contains("Buy milk"));
}
