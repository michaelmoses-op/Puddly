//! Step 28: action `ctx.db` helpers via nested IPC + host CRUD (ADR-046).

use puddly_actions::IpcActionRunner;
use puddly_auth::SessionStore;
use puddly_check::check;
use puddly_core::{CSRF_COOKIE_NAME, CSRF_HEADER_NAME, SESSION_COOKIE_NAME};
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

fn todo_row(id: &str, owner: &str, title: &str) -> DbRow {
    let mut row = DbRow::new();
    row.insert("id", SqlValue::Uuid(id.into()));
    row.insert("title", SqlValue::Text(title.into()));
    row.insert("done", SqlValue::Bool(false));
    row.insert("project", SqlValue::Uuid("proj-1".into()));
    row.insert("owner", SqlValue::Uuid(owner.into()));
    row
}

#[tokio::test]
async fn archive_action_loads_row_via_ctx_db_get() {
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

    let user = puddly_auth::AuthUser::new("owner-me", "me@x.co");
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
    // 1) enforce_action_access FindById
    exec.push_canned(vec![todo_row("arch-1", "owner-me", "Milk")]);
    // 2) ctx.db.get FindById
    exec.push_canned(vec![todo_row("arch-1", "owner-me", "Milk")]);

    let req = RpcRequest::new("todo.archive", json!({"id": "arch-1"}))
        .with_header(CSRF_HEADER_NAME, "1")
        .with_cookie(format!("{SESSION_COOKIE_NAME}={token}; {CSRF_COOKIE_NAME}=1"));
    let resp = rt.handle(&req, &mut exec).await;
    assert!(resp.ok, "{resp:?}");
    let data = resp.data.unwrap();
    assert_eq!(data["archived"], "arch-1");
    assert_eq!(data["userId"], "owner-me");
    assert_eq!(data["title"], "Milk");

    // ADR-036 host delete after action
    assert!(
        exec.log.iter().any(|q| q.kind == "execute"),
        "expected DELETE execute after archive: {:?}",
        exec.log
    );
}

#[tokio::test]
async fn ctx_db_get_forbidden_for_other_owner() {
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

    let user = puddly_auth::AuthUser::new("owner-me", "me@x.co");
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
    // Action access uses owner rule on the *action* resource row — must pass first.
    // Put the caller's id so archive access allows; then ctx.db.get loads a foreign row.
    exec.push_canned(vec![todo_row("arch-1", "owner-me", "Mine")]);
    // ctx.db.get returns someone else's row → forbid
    exec.push_canned(vec![todo_row("arch-1", "other-owner", "Secret")]);

    let req = RpcRequest::new("todo.archive", json!({"id": "arch-1"}))
        .with_header(CSRF_HEADER_NAME, "1")
        .with_cookie(format!("{SESSION_COOKIE_NAME}={token}; {CSRF_COOKIE_NAME}=1"));
    let resp = rt.handle(&req, &mut exec).await;
    assert!(!resp.ok, "expected action failure, got {resp:?}");
    let err = resp.error.as_ref().expect("error body");
    // Nested ctx.db forbidden re-emitted as wire code → HTTP 403 (ADR-049).
    assert_eq!(err.code, "forbidden", "{err:?}");
    assert_eq!(err.status, 403, "{err:?}");
    assert!(
        err.message.contains("forbidden"),
        "message={}",
        err.message
    );
}

#[tokio::test]
async fn ctx_db_get_unauthorized_without_session() {
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

    let mut rt = RpcRuntime {
        ir: &ir,
        book: &book,
        actions: &actions,
        sessions: &mut sessions,
        runner: &mut runner,
        secure_cookies: false,
    };
    let mut exec = RecordingExecutor::new();

    // No cookie → enforce_action_access fails before sidecar (owner rule).
    let req = RpcRequest::new("todo.archive", json!({"id": "arch-1"}))
        .with_header(CSRF_HEADER_NAME, "1")
        .with_cookie(format!("{CSRF_COOKIE_NAME}=1"));
    let resp = rt.handle(&req, &mut exec).await;
    assert!(!resp.ok, "{resp:?}");
    assert_eq!(resp.error.as_ref().unwrap().code, "unauthorized");
}
