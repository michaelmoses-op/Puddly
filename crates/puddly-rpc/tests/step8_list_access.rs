//! Step 8: list filters AND owner; per-action access; IPC protocol seam.

use puddly_actions::{catalog_from_app, IpcActionRunner, IpcRequest, IpcResponse, IPC_PROTOCOL_VERSION};
use puddly_auth::SessionStore;
use puddly_check::check;
use puddly_core::{CSRF_HEADER_NAME, SESSION_COOKIE_NAME};
use puddly_data::statement_book_from_app;
use puddly_db::{DbRow, RecordingExecutor, SqlValue};
use puddly_rpc::{RpcRequest, RpcRuntime, RpcServer};
use serde_json::json;
use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

fn todo_slice() -> PathBuf {
    workspace_root().join("examples/todo-slice")
}

fn load_server() -> RpcServer {
    let root = todo_slice();
    RpcServer::from_app(&root, check(&root).ir.unwrap()).unwrap()
}

#[test]
fn list_ands_body_filter_with_owner_predicate() {
    let mut server = load_server();
    let user = puddly_auth::AuthUser::new("owner-1", "o@x.co");
    let token = server.sessions.create(&user, 3600).unwrap();
    let mut exec = RecordingExecutor::new();
    exec.push_canned(vec![]);

    let req = RpcRequest::new(
        "todo.list",
        json!({
            "where": { "done": false },
            "limit": 25
        }),
    )
    .with_cookie(format!("{SESSION_COOKIE_NAME}={token}"));
    let resp = server.handle(&req, &mut exec);
    assert!(resp.ok, "{resp:?}");

    let q = exec.log.iter().find(|q| q.sql.contains("SELECT")).unwrap();
    assert!(q.sql.contains("\"owner\""));
    assert!(q.sql.contains("\"done\""));
    assert!(q.sql.contains(" AND "));
    // owner first ($1), done ($2), limit ($3)
    assert_eq!(q.params[0], SqlValue::Uuid("owner-1".into()));
    assert_eq!(q.params[1], SqlValue::Bool(false));
    assert_eq!(q.params[2], SqlValue::Int(25));
}

#[test]
fn action_access_forbids_non_owner() {
    let mut server = load_server();
    let user = puddly_auth::AuthUser::new("me", "me@x.co");
    let token = server.sessions.create(&user, 3600).unwrap();
    let mut exec = RecordingExecutor::new();
    let mut row = DbRow::new();
    row.insert("id", SqlValue::Uuid("t1".into()));
    row.insert("owner", SqlValue::Uuid("other".into()));
    exec.push_canned(vec![row]);

    let req = RpcRequest::new("todo.archive", json!({"id": "t1"}))
        .with_header(CSRF_HEADER_NAME, "1")
        .with_cookie(format!("{SESSION_COOKIE_NAME}={token}"));
    let resp = server.handle(&req, &mut exec);
    assert!(!resp.ok);
    assert_eq!(resp.error.as_ref().unwrap().code, "forbidden");
}

#[test]
fn archive_action_has_owner_access_in_ir() {
    let ir = check(&todo_slice()).ir.unwrap();
    let todo = ir.resources.iter().find(|r| r.name == "todo").unwrap();
    let archive = todo.actions.iter().find(|a| a.name == "archive").unwrap();
    match &archive.access {
        Some(puddly_ir::RuleIr::Owner { field, .. }) => assert_eq!(field, "owner"),
        other => panic!("expected owner access, got {other:?}"),
    }
}

#[test]
fn ipc_protocol_stub_runner_frames_call() {
    let root = todo_slice();
    let ir = check(&root).ir.unwrap();
    let book = statement_book_from_app(&ir);
    let actions = catalog_from_app(&root, &ir).unwrap();
    let mut sessions = puddly_auth::MemorySessionStore::new();
    let mut runner = IpcActionRunner::stub();
    let user = puddly_auth::AuthUser::new("me", "me@x.co");
    let token = sessions.create(&user, 3600).unwrap();

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
    row.insert("id", SqlValue::Uuid("t1".into()));
    row.insert("owner", SqlValue::Uuid("me".into()));
    exec.push_canned(vec![row]);

    let req = RpcRequest::new("todo.archive", json!({"id": "t1"}))
        .with_header(CSRF_HEADER_NAME, "1")
        .with_cookie(format!("{SESSION_COOKIE_NAME}={token}"));
    let resp = rt.handle(&req, &mut exec);
    assert!(!resp.ok);
    assert_eq!(resp.error.as_ref().unwrap().code, "action");

    // Protocol types round-trip
    assert_eq!(IPC_PROTOCOL_VERSION, 1);
    let loaded = actions.get("todo", "archive").unwrap();
    let call = puddly_actions::ActionCall {
        resource: "todo",
        action: "archive",
        input: &json!({"id": "t1"}),
        source_path: &loaded.path,
        source: &loaded.source,
    };
    let line = IpcRequest::run(&call, None).encode_line().unwrap();
    assert!(line.contains("\"op\":\"run\""));
    let ok = IpcResponse::ok_data(json!({"ok": true})).encode_line().unwrap();
    assert!(IpcResponse::decode_line(&ok).unwrap().ok);
}
