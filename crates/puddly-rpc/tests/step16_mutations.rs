//! Step 16: UI mutation loop — CSRF + session cookies for create/update/archive.
//! RecordingExecutor only (no live Postgres).

use puddly_actions::{catalog_from_app, NativeActionRunner};
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
    let result = check(&root);
    assert!(result.ok, "todo-slice must check clean");
    RpcServer::from_app(&root, result.ir.unwrap()).expect("load actions")
}

fn cookie_from_set(set_cookies: &[String]) -> String {
    let raw = set_cookies
        .first()
        .expect("expected Set-Cookie")
        .split(';')
        .next()
        .unwrap()
        .trim();
    raw.to_string()
}

#[tokio::test]
async fn mutation_without_csrf_fails() {
    let mut server = load_server();
    let mut exec = RecordingExecutor::new();
    exec.push_canned(vec![]); // register email check

    let reg = RpcRequest::new(
        "auth.register",
        json!({
            "email": "csrf@example.com",
            "password": "password123"
        }),
    )
    .with_header(CSRF_HEADER_NAME, "1");
    let reg_resp = server.handle(&reg, &mut exec).await;
    assert!(reg_resp.ok, "{reg_resp:?}");
    let cookie = cookie_from_set(&reg_resp.set_cookies);

    let create = RpcRequest::new(
        "todo.create",
        json!({
            "title": "no csrf",
            "project": "00000000-0000-4000-8000-000000000001"
        }),
    )
    .with_cookie(cookie);
    let resp = server.handle(&create, &mut exec).await;
    assert!(!resp.ok);
    assert_eq!(resp.error.as_ref().unwrap().code, "csrf");
}

#[tokio::test]
async fn register_create_update_archive_loop() {
    let root = todo_slice();
    let ir = check(&root).ir.unwrap();
    let book = statement_book_from_app(&ir);
    let actions = catalog_from_app(&root, &ir).unwrap();
    let mut sessions = puddly_auth::MemorySessionStore::new();
    let mut runner = NativeActionRunner::new();
    runner.register("todo", "archive", |input| {
        Ok(json!({ "archived": input["id"] }))
    });

    let mut rt = RpcRuntime {
        ir: &ir,
        book: &book,
        actions: &actions,
        sessions: &mut sessions,
        runner: &mut runner,
        secure_cookies: false,
    };
    let mut exec = RecordingExecutor::new();

    // 1) Register → session cookie
    exec.push_canned(vec![]); // email not taken
    let reg = RpcRequest::new(
        "auth.register",
        json!({
            "email": "mut@example.com",
            "password": "password123"
        }),
    )
    .with_header(CSRF_HEADER_NAME, "1");
    let reg_resp = rt.handle(&reg, &mut exec).await;
    assert!(reg_resp.ok, "{reg_resp:?}");
    let cookie = cookie_from_set(&reg_resp.set_cookies);
    assert!(cookie.starts_with(&format!("{SESSION_COOKIE_NAME}=")));
    let user_id = reg_resp.data.as_ref().unwrap()["user"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    // 2) Create todo (owner injected from session; title + project from body)
    let create = RpcRequest::new(
        "todo.create",
        json!({
            "title": "Buy milk",
            "project": "00000000-0000-4000-8000-000000000001"
        }),
    )
    .with_header(CSRF_HEADER_NAME, "1")
    .with_cookie(cookie.clone());
    let create_resp = rt.handle(&create, &mut exec).await;
    assert!(create_resp.ok, "{create_resp:?}");
    let todo_id = create_resp.data.as_ref().unwrap()["id"]
        .as_str()
        .expect("create returns id")
        .to_string();
    assert_eq!(todo_id.len(), 36);

    let insert = exec
        .log
        .iter()
        .find(|q| q.kind == "execute" && q.sql.contains("INSERT INTO \"todo\""))
        .expect("todo INSERT recorded");
    assert!(
        insert.params.iter().any(|p| p == &SqlValue::Uuid(user_id.clone())),
        "owner from session in INSERT params: {:?}",
        insert.params
    );
    assert!(
        insert
            .params
            .iter()
            .any(|p| p == &SqlValue::Text("Buy milk".into())),
        "title in INSERT params: {:?}",
        insert.params
    );

    // 3) Update done (partial body; find_by_id then UPDATE)
    let mut row = DbRow::new();
    row.insert("id", SqlValue::Uuid(todo_id.clone()));
    row.insert("title", SqlValue::Text("Buy milk".into()));
    row.insert("done", SqlValue::Bool(false));
    row.insert("dueDate", SqlValue::Null);
    row.insert("project", SqlValue::Uuid("00000000-0000-4000-8000-000000000001".into()));
    row.insert("owner", SqlValue::Uuid(user_id.clone()));
    row.insert("createdAt", SqlValue::Text("2026-01-01T00:00:00Z".into()));
    exec.push_canned(vec![row]);

    let update = RpcRequest::new(
        "todo.update",
        json!({ "id": todo_id, "done": true }),
    )
    .with_header(CSRF_HEADER_NAME, "1")
    .with_cookie(cookie.clone());
    let update_resp = rt.handle(&update, &mut exec).await;
    assert!(update_resp.ok, "{update_resp:?}");

    let upd = exec
        .log
        .iter()
        .rev()
        .find(|q| q.kind == "execute" && q.sql.contains("UPDATE \"todo\""))
        .expect("todo UPDATE recorded");
    assert!(
        upd.params.iter().any(|p| p == &SqlValue::Bool(true)),
        "done=true in UPDATE params: {:?}",
        upd.params
    );
    assert!(
        upd.params
            .iter()
            .any(|p| p == &SqlValue::Text("Buy milk".into())),
        "partial update must preserve title from existing row: {:?}",
        upd.params
    );

    // 4) Archive (owner check + native runner)
    let mut row = DbRow::new();
    row.insert("id", SqlValue::Uuid(todo_id.clone()));
    row.insert("owner", SqlValue::Uuid(user_id));
    exec.push_canned(vec![row]);

    let archive = RpcRequest::new("todo.archive", json!({ "id": todo_id }))
        .with_header(CSRF_HEADER_NAME, "1")
        .with_cookie(cookie);
    let archive_resp = rt.handle(&archive, &mut exec).await;
    assert!(archive_resp.ok, "{archive_resp:?}");
    assert_eq!(archive_resp.data.unwrap()["archived"], todo_id);
    assert!(
        exec.log
            .iter()
            .any(|q| q.kind == "execute" && q.sql.contains("DELETE FROM \"todo\"")),
        "Step 18: archive persists via DELETE"
    );
}

#[test]
fn islands_runtime_includes_csrf_and_credentials() {
    let js = include_str!("../../../tools/puddly-islands.js");
    assert!(
        js.contains("X-Puddly-CSRF"),
        "client must send CSRF header name used by puddly-core"
    );
    assert!(
        js.contains("credentials: \"include\"") || js.contains("credentials:\"include\""),
        "client must include session cookies"
    );
    assert!(js.contains("puddlyIslands.rpc") || js.contains("function rpc("));
    assert!(js.contains("data-puddly-rpc"));
    assert!(
        js.contains("data-puddly-redirect") || js.contains("redirectTarget"),
        "Step 17: auth forms redirect after success"
    );
}

#[tokio::test]
async fn login_then_create_injects_owner() {
    let mut server = load_server();
    let mut exec = RecordingExecutor::new();

    // Mint session without register SQL dance
    let user = puddly_auth::AuthUser::new("owner-16", "o16@x.co");
    let token = server.sessions.create(&user, 3600).await.unwrap();

    let create = RpcRequest::new(
        "todo.create",
        json!({
            "title": "from session",
            "project": "00000000-0000-4000-8000-000000000001"
        }),
    )
    .with_header(CSRF_HEADER_NAME, "1")
    .with_cookie(format!("{SESSION_COOKIE_NAME}={token}"));
    let resp = server.handle(&create, &mut exec).await;
    assert!(resp.ok, "{resp:?}");

    let insert = exec
        .log
        .iter()
        .find(|q| q.sql.contains("INSERT INTO \"todo\""))
        .expect("insert");
    // Non-UUID-shaped session ids land as Text via json_to_sql heuristic.
    assert!(
        insert.params.iter().any(|p| {
            matches!(p, SqlValue::Text(s) | SqlValue::Uuid(s) if s == "owner-16")
        }),
        "owner from session missing in {:?}",
        insert.params
    );
}
