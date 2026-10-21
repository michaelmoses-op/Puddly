//! Step 26: soft-plug IR row decode into CRUD list/get (ADR-044).

use puddly_auth::{AuthUser, SessionStore};
use puddly_check::check;
use puddly_core::SESSION_COOKIE_NAME;
use puddly_db::{DbRow, RecordingExecutor, SqlValue};
use puddly_rpc::{decode_row_to_json, RpcRequest, RpcServer};
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

fn canned_todo_row(done: SqlValue) -> DbRow {
    let mut row = DbRow::new();
    row.insert(
        "id",
        SqlValue::Uuid("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee".into()),
    );
    row.insert("title", SqlValue::Text("Hi".into()));
    row.insert("done", done);
    row.insert("dueDate", SqlValue::Null);
    row.insert(
        "project",
        SqlValue::Uuid("11111111-1111-1111-1111-111111111111".into()),
    );
    row.insert(
        "owner",
        SqlValue::Uuid("22222222-2222-2222-2222-222222222222".into()),
    );
    row.insert("createdAt", SqlValue::Text("2026-01-01T00:00:00Z".into()));
    row
}

#[test]
fn decode_matches_todo_slice_ir() {
    let ir = check(&todo_slice()).ir.expect("ir");
    let todo = ir.resources.iter().find(|r| r.name == "todo").unwrap();
    let v = decode_row_to_json(todo, &canned_todo_row(SqlValue::Bool(false))).expect("decode");
    assert_eq!(v["title"], "Hi");
    assert_eq!(v["done"], false);
    assert!(v["dueDate"].is_null());
    assert!(v.get("id").is_some());
}

#[tokio::test]
async fn list_uses_typed_decode() {
    let root = todo_slice();
    let ir = check(&root).ir.unwrap();
    let mut server = RpcServer::from_app(&root, ir).unwrap();
    let user = AuthUser::new(
        "22222222-2222-2222-2222-222222222222",
        "a@b.co",
    );
    let token = server.sessions.create(&user, 3600).await.unwrap();

    let mut exec = RecordingExecutor::new();
    exec.push_canned(vec![canned_todo_row(SqlValue::Bool(true))]);

    let mut req = RpcRequest::new("todo.list", json!({}));
    req.cookie_header = Some(format!("{SESSION_COOKIE_NAME}={token}"));
    let resp = server.handle(&req, &mut exec).await;
    assert!(resp.ok, "{:?}", resp.error);
    let items = resp.data.unwrap()["items"].as_array().unwrap().clone();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["title"], "Hi");
    assert_eq!(items[0]["done"], true);
}

#[tokio::test]
async fn list_decode_type_error_surfaces() {
    let root = todo_slice();
    let ir = check(&root).ir.unwrap();
    let mut server = RpcServer::from_app(&root, ir).unwrap();
    let user = AuthUser::new(
        "22222222-2222-2222-2222-222222222222",
        "a@b.co",
    );
    let token = server.sessions.create(&user, 3600).await.unwrap();

    let mut exec = RecordingExecutor::new();
    exec.push_canned(vec![canned_todo_row(SqlValue::Text("not-a-bool".into()))]);

    let mut req = RpcRequest::new("todo.list", json!({}));
    req.cookie_header = Some(format!("{SESSION_COOKIE_NAME}={token}"));
    let resp = server.handle(&req, &mut exec).await;
    assert!(!resp.ok, "expected decode failure");
    let err = format!("{:?}", resp.error);
    assert!(
        err.contains("row decode") || err.contains("wrong type") || err.contains("done"),
        "{err}"
    );
}
