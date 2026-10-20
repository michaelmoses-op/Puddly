//! Step 18: vertical slice loop — register → create → list → update → archive → list.
//! RecordingExecutor only (no live Postgres).

use puddly_actions::{catalog_from_app, NativeActionRunner};
use puddly_check::check;
use puddly_core::{CSRF_HEADER_NAME, SESSION_COOKIE_NAME};
use puddly_data::statement_book_from_app;
use puddly_db::{DbRow, RecordingExecutor, SqlValue};
use puddly_rpc::{RpcRequest, RpcRuntime};
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
async fn register_create_list_update_archive_list() {
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

    // 1) Register
    exec.push_canned(vec![]); // email not taken
    let reg = RpcRequest::new(
        "auth.register",
        json!({
            "email": "slice18@example.com",
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

    // 2) Create todo without project — framework ensures a personal project
    let create = RpcRequest::new("todo.create", json!({ "title": "Ship Step 18" }))
        .with_header(CSRF_HEADER_NAME, "1")
        .with_cookie(cookie.clone());
    // project lookup returns empty → insert personal project (execute, no canned)
    let create_resp = rt.handle(&create, &mut exec).await;
    assert!(create_resp.ok, "{create_resp:?}");
    let todo_id = create_resp.data.as_ref().unwrap()["id"]
        .as_str()
        .expect("create returns id")
        .to_string();

    let user_insert = exec
        .log
        .iter()
        .find(|q| q.kind == "execute" && q.sql.contains("INSERT INTO \"user\""))
        .expect("app user row INSERT (auth id → user FK)");
    assert!(
        user_insert
            .params
            .iter()
            .any(|p| p == &SqlValue::Uuid(user_id.clone())),
        "user id missing: {:?}",
        user_insert.params
    );

    let project_insert = exec
        .log
        .iter()
        .find(|q| q.kind == "execute" && q.sql.contains("INSERT INTO \"project\""))
        .expect("personal project INSERT");
    assert!(
        project_insert
            .params
            .iter()
            .any(|p| p == &SqlValue::Uuid(user_id.clone())),
        "project owner missing: {:?}",
        project_insert.params
    );

    let todo_insert = exec
        .log
        .iter()
        .find(|q| q.kind == "execute" && q.sql.contains("INSERT INTO \"todo\""))
        .expect("todo INSERT");
    assert!(
        !todo_insert.sql.contains("createdAt"),
        "auto createdAt must be omitted so Postgres DEFAULT applies: {}",
        todo_insert.sql
    );
    assert!(
        todo_insert
            .params
            .iter()
            .any(|p| p == &SqlValue::Text("Ship Step 18".into())),
        "title missing: {:?}",
        todo_insert.params
    );
    assert!(
        todo_insert
            .params
            .iter()
            .any(|p| p == &SqlValue::Uuid(user_id.clone())),
        "owner missing: {:?}",
        todo_insert.params
    );

    // 3) List — one item
    let mut listed = DbRow::new();
    listed.insert("id", SqlValue::Uuid(todo_id.clone()));
    listed.insert("title", SqlValue::Text("Ship Step 18".into()));
    listed.insert("done", SqlValue::Bool(false));
    listed.insert("dueDate", SqlValue::Null);
    listed.insert(
        "project",
        SqlValue::Uuid("00000000-0000-4000-8000-000000000099".into()),
    );
    listed.insert("owner", SqlValue::Uuid(user_id.clone()));
    listed.insert("createdAt", SqlValue::Text("2026-10-20T00:00:00Z".into()));
    exec.push_canned(vec![listed]);

    let list1 = RpcRequest::new("todo.list", json!({}))
        .with_header(CSRF_HEADER_NAME, "1")
        .with_cookie(cookie.clone());
    let list1_resp = rt.handle(&list1, &mut exec).await;
    assert!(list1_resp.ok, "{list1_resp:?}");
    let items = list1_resp.data.as_ref().unwrap()["items"]
        .as_array()
        .unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["id"], todo_id);
    assert_eq!(items[0]["title"], "Ship Step 18");

    // 4) Update done
    let mut row = DbRow::new();
    row.insert("id", SqlValue::Uuid(todo_id.clone()));
    row.insert("title", SqlValue::Text("Ship Step 18".into()));
    row.insert("done", SqlValue::Bool(false));
    row.insert("dueDate", SqlValue::Null);
    row.insert(
        "project",
        SqlValue::Uuid("00000000-0000-4000-8000-000000000099".into()),
    );
    row.insert("owner", SqlValue::Uuid(user_id.clone()));
    row.insert("createdAt", SqlValue::Text("2026-10-20T00:00:00Z".into()));
    exec.push_canned(vec![row]);

    let update = RpcRequest::new("todo.update", json!({ "id": todo_id, "done": true }))
        .with_header(CSRF_HEADER_NAME, "1")
        .with_cookie(cookie.clone());
    let update_resp = rt.handle(&update, &mut exec).await;
    assert!(update_resp.ok, "{update_resp:?}");
    assert!(
        exec.log
            .iter()
            .any(|q| q.kind == "execute" && q.sql.contains("UPDATE \"todo\"")),
        "expected todo UPDATE"
    );

    // 5) Archive — owner check + action + DELETE
    let mut row = DbRow::new();
    row.insert("id", SqlValue::Uuid(todo_id.clone()));
    row.insert("owner", SqlValue::Uuid(user_id.clone()));
    exec.push_canned(vec![row]);

    let archive = RpcRequest::new("todo.archive", json!({ "id": todo_id }))
        .with_header(CSRF_HEADER_NAME, "1")
        .with_cookie(cookie.clone());
    let archive_resp = rt.handle(&archive, &mut exec).await;
    assert!(archive_resp.ok, "{archive_resp:?}");
    assert_eq!(archive_resp.data.unwrap()["archived"], todo_id);

    let delete = exec
        .log
        .iter()
        .rev()
        .find(|q| q.kind == "execute" && q.sql.contains("DELETE FROM \"todo\""))
        .expect("archive must DELETE the row");
    assert!(
        delete
            .params
            .iter()
            .any(|p| p == &SqlValue::Uuid(todo_id.clone())),
        "delete id missing: {:?}",
        delete.params
    );

    // 6) List again — empty
    exec.push_canned(vec![]);
    let list2 = RpcRequest::new("todo.list", json!({}))
        .with_header(CSRF_HEADER_NAME, "1")
        .with_cookie(cookie);
    let list2_resp = rt.handle(&list2, &mut exec).await;
    assert!(list2_resp.ok, "{list2_resp:?}");
    let items2 = list2_resp.data.as_ref().unwrap()["items"]
        .as_array()
        .unwrap();
    assert!(
        items2.is_empty(),
        "list after archive must be empty: {items2:?}"
    );
}

#[test]
fn example_views_have_nav_and_empty_state() {
    let todo = std::fs::read_to_string(todo_slice().join("features/todos/todo.view.tsx")).unwrap();
    assert!(todo.contains("href=\"/login\""), "todo nav Login");
    assert!(todo.contains("href=\"/register\""), "todo nav Register");
    assert!(todo.contains("href=\"/\""), "todo nav Todos");
    assert!(todo.contains("auth.logout"), "todo Logout");
    assert!(
        todo.contains("No todos yet"),
        "empty list CSS message"
    );
    assert!(
        !todo.contains("00000000-0000-4000-8000-000000000001"),
        "no magic project UUID in create form"
    );

    let login =
        std::fs::read_to_string(todo_slice().join("features/auth/login.view.tsx")).unwrap();
    assert!(login.contains("href=\"/\""), "login nav Todos");
    assert!(login.contains("href=\"/register\""), "login nav Register");

    let register =
        std::fs::read_to_string(todo_slice().join("features/auth/register.view.tsx")).unwrap();
    assert!(register.contains("href=\"/\""), "register nav Todos");
    assert!(register.contains("href=\"/login\""), "register nav Login");
}

#[test]
fn islands_runtime_surfaces_rpc_errors() {
    let js = include_str!("../../../tools/puddly-islands.js");
    assert!(
        js.contains("data-puddly-error") || js.contains("showRpcError"),
        "Step 18: inline RPC error host"
    );
}
