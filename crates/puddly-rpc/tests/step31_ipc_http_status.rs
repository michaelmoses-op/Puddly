//! Step 31: map `IpcErrorCode` → RPC HTTP status (ADR-049).

use puddly_actions::{ActionError, IpcErrorCode, NativeActionRunner};
use puddly_auth::{MemorySessionStore, SessionStore};
use puddly_check::check;
use puddly_core::{CSRF_COOKIE_NAME, CSRF_HEADER_NAME, SESSION_COOKIE_NAME};
use puddly_data::statement_book_from_app;
use puddly_db::{DbRow, RecordingExecutor, SqlValue};
use puddly_rpc::{RpcError, RpcRequest, RpcRuntime};
use serde_json::json;
use std::path::PathBuf;

fn todo_slice() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/todo-slice")
}

fn archive_row(owner: &str) -> DbRow {
    let mut row = DbRow::new();
    row.insert("id", SqlValue::Uuid("t1".into()));
    row.insert("owner", SqlValue::Uuid(owner.into()));
    row.insert("title", SqlValue::Text("x".into()));
    row.insert("done", SqlValue::Bool(false));
    row.insert("project", SqlValue::Uuid("p1".into()));
    row
}

#[test]
fn ipc_error_code_http_status_table() {
    assert_eq!(IpcErrorCode::BadRequest.http_status(), 400);
    assert_eq!(IpcErrorCode::UnknownOp.http_status(), 400);
    assert_eq!(IpcErrorCode::Unauthorized.http_status(), 401);
    assert_eq!(IpcErrorCode::Forbidden.http_status(), 403);
    assert_eq!(IpcErrorCode::NotFound.http_status(), 404);
    assert_eq!(IpcErrorCode::RunFailed.http_status(), 500);
    assert_eq!(IpcErrorCode::DbFailed.http_status(), 500);
    assert_eq!(IpcErrorCode::ImportFailed.http_status(), 500);
    assert_eq!(IpcErrorCode::SidecarNotConfigured.http_status(), 500);
    assert_eq!(IpcErrorCode::ProtocolMismatch.http_status(), 500);
    assert_eq!(IpcErrorCode::NoRunExport.http_status(), 500);
}

#[test]
fn rpc_status_hint_maps_action_ipc_codes_and_fallback() {
    let mapped = [
        (IpcErrorCode::BadRequest, 400),
        (IpcErrorCode::Unauthorized, 401),
        (IpcErrorCode::Forbidden, 403),
        (IpcErrorCode::NotFound, 404),
        (IpcErrorCode::RunFailed, 500),
    ];
    for (code, want) in mapped {
        let err = RpcError::from(ActionError::db(code, "x"));
        assert_eq!(err.status_hint(), want, "{code}");
    }
    // Uncoded action errors stay 500.
    let fallback = RpcError::from(ActionError::msg("boom"));
    assert_eq!(fallback.status_hint(), 500);
    let stub = RpcError::from(ActionError::NotExecutable {
        resource: "todo".into(),
        action: "archive".into(),
        path: "/x".into(),
    });
    assert_eq!(stub.status_hint(), 500);
}

#[tokio::test]
async fn action_dispatch_surfaces_mapped_status_and_code() {
    let root = todo_slice();
    let ir = check(&root).ir.unwrap();
    let book = statement_book_from_app(&ir);
    let actions = puddly_actions::catalog_from_app(&root, &ir).unwrap();
    let mut sessions = MemorySessionStore::new();
    let mut runner = NativeActionRunner::new();
    runner.register("todo", "archive", |_| {
        Err(ActionError::db(IpcErrorCode::NotFound, "todo gone"))
    });

    let user = puddly_auth::AuthUser::new("me", "me@x.co");
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
    exec.push_canned(vec![archive_row("me")]);

    let req = RpcRequest::new("todo.archive", json!({"id": "t1"}))
        .with_header(CSRF_HEADER_NAME, "1")
        .with_cookie(format!("{SESSION_COOKIE_NAME}={token}; {CSRF_COOKIE_NAME}=1"));
    let resp = rt.handle(&req, &mut exec).await;
    assert!(!resp.ok, "{resp:?}");
    let err = resp.error.as_ref().unwrap();
    assert_eq!(err.code, "not_found");
    assert_eq!(err.status, 404);
    assert!(err.message.contains("todo gone"), "{}", err.message);
}

#[tokio::test]
async fn action_dispatch_unknown_fallback_is_500_action() {
    let root = todo_slice();
    let ir = check(&root).ir.unwrap();
    let book = statement_book_from_app(&ir);
    let actions = puddly_actions::catalog_from_app(&root, &ir).unwrap();
    let mut sessions = MemorySessionStore::new();
    let mut runner = NativeActionRunner::new();
    runner.register("todo", "archive", |_| Err(ActionError::msg("unexpected")));

    let user = puddly_auth::AuthUser::new("me", "me@x.co");
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
    exec.push_canned(vec![archive_row("me")]);

    let req = RpcRequest::new("todo.archive", json!({"id": "t1"}))
        .with_header(CSRF_HEADER_NAME, "1")
        .with_cookie(format!("{SESSION_COOKIE_NAME}={token}; {CSRF_COOKIE_NAME}=1"));
    let resp = rt.handle(&req, &mut exec).await;
    assert!(!resp.ok, "{resp:?}");
    let err = resp.error.as_ref().unwrap();
    assert_eq!(err.code, "action");
    assert_eq!(err.status, 500);
}
