//! Step 14: view `load` via CRUD + session cookie; no demo fixture on happy path.

use puddly_auth::{AuthUser, SessionStore};
use puddly_check::check;
use puddly_core::SESSION_COOKIE_NAME;
use puddly_db::{DbRow, RecordingExecutor, SqlValue};
use puddly_rpc::RpcServer;
use puddly_server::{dispatch_http, HttpRequest};
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

fn canned_todo_row(id: &str, title: &str) -> DbRow {
    let mut row = DbRow::new();
    row.insert("id", SqlValue::Uuid(id.into()));
    row.insert("title", SqlValue::Text(title.into()));
    row.insert("done", SqlValue::Bool(false));
    row.insert("owner", SqlValue::Uuid("owner-1".into()));
    row
}

#[tokio::test]
async fn authenticated_view_load_renders_canned_title() {
    let mut rpc = load_rpc();
    let user = AuthUser::new("owner-1", "o@x.co");
    let token = rpc.sessions.create(&user, 3600).await.unwrap();
    let rpc = Mutex::new(rpc);

    let mut exec = RecordingExecutor::new();
    exec.push_canned(vec![
        canned_todo_row("t-from-db", "From DB"),
        canned_todo_row("t-two", "Second row"),
    ]);

    let views = ViewBundle::load(&example_root()).expect("views");
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
    assert_eq!(resp.status, 200, "{}", String::from_utf8_lossy(&resp.body));
    let body = String::from_utf8_lossy(&resp.body);
    assert!(body.contains("From DB"), "expected canned title:\n{body}");
    assert!(body.contains("Second row"), "expected second item:\n{body}");
    assert!(!body.contains("Buy milk"), "fixture title must not appear:\n{body}");
    assert!(body.contains("Todos"), "{body}");
    assert_eq!(
        body.matches("data-puddly-island=\"TodoRow\"").count(),
        2,
        "list dialect should render one island per item:\n{body}"
    );
    assert!(body.contains("data-puddly-key=\"t-from-db\""), "{body}");
    assert!(
        exec.log.iter().any(|q| q.sql.contains("SELECT")),
        "load must hit list SQL: {:?}",
        exec.log
    );
}

#[tokio::test]
async fn anonymous_view_load_returns_401_html() {
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
    let body = String::from_utf8_lossy(&resp.body);
    assert!(body.contains("Sign in required"), "{body}");
    assert!(body.contains("href=\"/login\""), "401 should link to login:\n{body}");
    assert!(!body.contains("Buy milk"), "{body}");
    assert!(exec.log.is_empty(), "unauthorized before query: {:?}", exec.log);
}
