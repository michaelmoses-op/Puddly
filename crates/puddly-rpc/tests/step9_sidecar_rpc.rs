//! Step 9: RPC dispatch → IpcActionRunner → real Node sidecar + user wiring.

use puddly_actions::IpcActionRunner;
use puddly_auth::SessionStore;
use puddly_check::check;
use puddly_core::{CSRF_HEADER_NAME, SESSION_COOKIE_NAME};
use puddly_data::statement_book_from_app;
use puddly_db::{DbRow, RecordingExecutor, SqlValue};
use puddly_rpc::{RpcRequest, RpcRuntime};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

fn node_available() -> bool {
    Command::new("node")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[tokio::test]
async fn rpc_archive_via_sidecar_receives_user() {
    if !node_available() {
        eprintln!("skip: node not on PATH");
        return;
    }
    let root = workspace_root().join("examples/todo-slice");
    let ir = check(&root).ir.expect("ir");
    let book = statement_book_from_app(&ir);
    let actions = puddly_actions::catalog_from_app(&root, &ir).unwrap();
    let mut sessions = puddly_auth::MemorySessionStore::new();
    let mut runner = IpcActionRunner::node_sidecar(&workspace_root());

    let user = puddly_auth::AuthUser::new("owner-me", "me@x.co").with_roles(vec!["admin".into()]);
    let token = sessions.create(&user, 3600).await.unwrap();

    let mut rt = RpcRuntime {
        ir: &ir,
        book: &book,
        actions: &actions,
        sessions: &mut sessions,
        runner: &mut runner,
        secure_cookies: false,
    };
    let mut exec = RecordingExecutor::new();
    let mut row = DbRow::new();
    row.insert("id", SqlValue::Uuid("arch-1".into()));
    row.insert("owner", SqlValue::Uuid("owner-me".into()));
    exec.push_canned(vec![row]);

    let req = RpcRequest::new("todo.archive", json!({"id": "arch-1"}))
        .with_header(CSRF_HEADER_NAME, "1")
        .with_cookie(format!("{SESSION_COOKIE_NAME}={token}"));
    let resp = rt.handle(&req, &mut exec).await;
    assert!(resp.ok, "{resp:?}");
    let data = resp.data.unwrap();
    assert_eq!(data["archived"], "arch-1");
    assert_eq!(data["userId"], "owner-me");
}
