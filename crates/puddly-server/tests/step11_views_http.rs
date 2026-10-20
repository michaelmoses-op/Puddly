//! Step 11: GET SSR HTML + island runtime without Postgres.

use puddly_check::check;
use puddly_db::RecordingExecutor;
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
fn get_root_ssr_without_executor() {
    let views = ViewBundle::load(&example_root()).expect("views");
    let req = HttpRequest {
        method: "GET".into(),
        path: "/".into(),
        headers: BTreeMap::new(),
        body: Vec::new(),
    };
    let resp = try_dispatch_get(&req, Some(&views)).unwrap().unwrap();
    assert_eq!(resp.status, 200);
    let body = String::from_utf8_lossy(&resp.body);
    assert!(body.contains("Buy milk"), "{body}");
    assert!(body.contains("data-puddly-island=\"TodoRow\""), "{body}");
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
}

#[tokio::test]
async fn dispatch_get_v_todo() {
    let rpc = Mutex::new(load_rpc());
    let mut exec = RecordingExecutor::new();
    let views = ViewBundle::load(&example_root()).unwrap();
    let req = HttpRequest {
        method: "GET".into(),
        path: "/v/todo".into(),
        headers: BTreeMap::new(),
        body: Vec::new(),
    };
    let resp = dispatch_http(&req, &rpc, &mut exec, Some(&views))
        .await
        .unwrap();
    assert_eq!(resp.status, 200);
    let body = String::from_utf8_lossy(&resp.body);
    assert!(body.contains("Todos"));
    assert!(exec.log.is_empty(), "SSR must not hit the DB");
}
