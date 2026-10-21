//! Step 6 RPC skeleton tests — RecordingExecutor, no live Postgres / HTTP.

use puddly_actions::{catalog_from_app, NativeActionRunner};
use puddly_auth::{hash_password, MemorySessionStore, SessionStore};
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
    let result = check(&root);
    assert!(result.ok, "todo-slice must check clean");
    RpcServer::from_app(&root, result.ir.unwrap()).expect("load actions")
}

#[test]
fn loads_archive_action_source() {
    let root = todo_slice();
    let ir = check(&root).ir.unwrap();
    let catalog = catalog_from_app(&root, &ir).unwrap();
    let action = catalog.get("todo", "archive").expect("archive loaded");
    assert!(action.source.contains("export async function run"));
    assert!(action.path.ends_with("archive.action.ts"));
}

#[tokio::test]
async fn csrf_required_on_create_not_list() {
    let mut server = load_server();
    let mut exec = RecordingExecutor::new();

    let list = RpcRequest::new("todo.list", json!({}));
    let resp = server.handle(&list, &mut exec).await;
    // unauthorized (no session) — but not CSRF
    assert!(!resp.ok);
    assert_eq!(resp.error.as_ref().unwrap().code, "unauthorized");

    let create = RpcRequest::new("todo.create", json!({"title": "x"}));
    let resp = server.handle(&create, &mut exec).await;
    assert!(!resp.ok);
    assert_eq!(resp.error.as_ref().unwrap().code, "csrf");
}

#[tokio::test]
async fn create_enforces_authenticated() {
    let mut server = load_server();
    let mut exec = RecordingExecutor::new();
    let req = RpcRequest::new("todo.create", json!({"title": "hi"}))
        .with_header(CSRF_HEADER_NAME, "1");
    let resp = server.handle(&req, &mut exec).await;
    assert!(!resp.ok);
    assert_eq!(resp.error.as_ref().unwrap().code, "unauthorized");
}

#[tokio::test]
async fn get_enforces_owner_against_row() {
    let mut server = load_server();
    let mut exec = RecordingExecutor::new();

    // find_by_id returns a row owned by someone else
    let mut row = DbRow::new();
    row.insert("id", SqlValue::Uuid("t1".into()));
    row.insert("owner", SqlValue::Uuid("other-user".into()));
    row.insert("title", SqlValue::Text("secret".into()));
    exec.push_canned(vec![row]);

    // Establish a session as user "me"
    let user = puddly_auth::AuthUser::new("me", "me@x.co");
    let token = server.sessions.create(&user, 3600).await.unwrap();

    let req = RpcRequest::new("todo.get", json!({"id": "t1"}))
        .with_cookie(format!("{SESSION_COOKIE_NAME}={token}"));
    let resp = server.handle(&req, &mut exec).await;
    assert!(!resp.ok);
    assert_eq!(resp.error.as_ref().unwrap().code, "forbidden");
}

#[tokio::test]
async fn get_allows_owner() {
    let mut server = load_server();
    let mut exec = RecordingExecutor::new();

    let mut row = DbRow::new();
    row.insert("id", SqlValue::Uuid("t1".into()));
    row.insert("owner", SqlValue::Uuid("me".into()));
    row.insert("title", SqlValue::Text("ok".into()));
    row.insert("done", SqlValue::Bool(false));
    row.insert("dueDate", SqlValue::Null);
    row.insert("project", SqlValue::Uuid("p1".into()));
    row.insert("createdAt", SqlValue::Text("2026-01-01T00:00:00Z".into()));
    exec.push_canned(vec![row]);

    let user = puddly_auth::AuthUser::new("me", "me@x.co");
    let token = server.sessions.create(&user, 3600).await.unwrap();

    let req = RpcRequest::new("todo.get", json!({"id": "t1"}))
        .with_cookie(format!("{SESSION_COOKIE_NAME}={token}"));
    let resp = server.handle(&req, &mut exec).await;
    assert!(resp.ok, "{resp:?}");
    assert_eq!(resp.data.as_ref().unwrap()["title"], "ok");
}

#[tokio::test]
async fn custom_action_requires_auth_and_runner() {
    let mut server = load_server();
    let mut exec = RecordingExecutor::new();

    let req = RpcRequest::new("todo.archive", json!({"id": "t1"}))
        .with_header(CSRF_HEADER_NAME, "1");
    let resp = server.handle(&req, &mut exec).await;
    assert_eq!(resp.error.as_ref().unwrap().code, "unauthorized");

    let user = puddly_auth::AuthUser::new("me", "me@x.co");
    let token = server.sessions.create(&user, 3600).await.unwrap();
    // owner access loads the row first
    let mut row = DbRow::new();
    row.insert("id", SqlValue::Uuid("t1".into()));
    row.insert("owner", SqlValue::Uuid("me".into()));
    exec.push_canned(vec![row]);
    let req = RpcRequest::new("todo.archive", json!({"id": "t1"}))
        .with_header(CSRF_HEADER_NAME, "1")
        .with_cookie(format!("{SESSION_COOKIE_NAME}={token}"));
    let resp = server.handle(&req, &mut exec).await;
    // Stub runner → action error
    assert!(!resp.ok);
    assert_eq!(resp.error.as_ref().unwrap().code, "action");
}

#[tokio::test]
async fn native_action_runner_executes_archive() {
    let root = todo_slice();
    let ir = check(&root).ir.unwrap();
    let book = statement_book_from_app(&ir);
    let actions = catalog_from_app(&root, &ir).unwrap();
    let mut sessions = MemorySessionStore::new();
    let mut runner = NativeActionRunner::new();
    runner.register("todo", "archive", |input| {
        Ok(json!({ "archived": input["id"] }))
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
    let mut row = DbRow::new();
    row.insert("id", SqlValue::Uuid("abc".into()));
    row.insert("owner", SqlValue::Uuid("me".into()));
    exec.push_canned(vec![row]);
    let req = RpcRequest::new("todo.archive", json!({"id": "abc"}))
        .with_header(CSRF_HEADER_NAME, "1")
        .with_cookie(format!("{SESSION_COOKIE_NAME}={token}"));
    let resp = rt.handle(&req, &mut exec).await;
    assert!(resp.ok, "{resp:?}");
    assert_eq!(resp.data.unwrap()["archived"], "abc");
}

#[tokio::test]
async fn auth_login_sets_cookie() {
    let mut server = load_server();
    let mut exec = RecordingExecutor::new();

    let hash = hash_password("password123").unwrap();
    let mut row = DbRow::new();
    row.insert("user_id", SqlValue::Uuid("u1".into()));
    row.insert("email", SqlValue::Text("a@b.co".into()));
    row.insert("password_hash", SqlValue::Text(hash));
    row.insert("roles", SqlValue::Text("[]".into()));
    exec.push_canned(vec![row]);

    let req = RpcRequest::new(
        "auth.login",
        json!({"email": "a@b.co", "password": "password123"}),
    )
    .with_header(CSRF_HEADER_NAME, "1");
    let resp = server.handle(&req, &mut exec).await;
    assert!(resp.ok, "{resp:?}");
    assert!(!resp.set_cookies.is_empty());
    assert!(resp.set_cookies[0].contains(SESSION_COOKIE_NAME));
    assert_eq!(resp.data.as_ref().unwrap()["user"]["id"], "u1");
}

#[test]
fn rpc_target_parse() {
    let t = puddly_rpc::RpcTarget::parse("/rpc/todo.list").unwrap();
    assert_eq!(t.resource, "todo");
    assert_eq!(t.operation, "list");
    assert!(!t.is_mutation());
    assert!(puddly_rpc::RpcTarget::parse("auth.login").unwrap().is_mutation());
}
