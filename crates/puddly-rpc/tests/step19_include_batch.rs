//! Step 19: list `include` batches related rows (ADR-037) — RecordingExecutor only.

use puddly_auth::SessionStore;
use puddly_check::check;
use puddly_core::SESSION_COOKIE_NAME;
use puddly_db::{DbRow, RecordingExecutor, SqlValue};
use puddly_rpc::{RpcRequest, RpcServer};
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

fn todo_row(id: &str, project: &str, owner: &str) -> DbRow {
    let mut row = DbRow::new();
    row.insert("id", SqlValue::Uuid(id.into()));
    row.insert("title", SqlValue::Text(format!("Todo {id}")));
    row.insert("done", SqlValue::Bool(false));
    row.insert("dueDate", SqlValue::Null);
    row.insert("project", SqlValue::Uuid(project.into()));
    row.insert("owner", SqlValue::Uuid(owner.into()));
    row.insert("createdAt", SqlValue::Text("2026-01-01T00:00:00Z".into()));
    row
}

fn project_row(id: &str, title: &str, owner: &str) -> DbRow {
    let mut row = DbRow::new();
    row.insert("id", SqlValue::Uuid(id.into()));
    row.insert("title", SqlValue::Text(title.into()));
    row.insert("owner", SqlValue::Uuid(owner.into()));
    row.insert("createdAt", SqlValue::Text("2026-01-01T00:00:00Z".into()));
    row
}

#[tokio::test]
async fn list_include_project_batches_one_query_not_n_plus_one() {
    let mut server = load_server();
    let user = puddly_auth::AuthUser::new("owner-19", "o19@x.co");
    let token = server.sessions.create(&user, 3600).await.unwrap();

    let mut exec = RecordingExecutor::new();
    // Primary list: two todos sharing one project + one with another project.
    exec.push_canned(vec![
        todo_row("t1", "p1", "owner-19"),
        todo_row("t2", "p1", "owner-19"),
        todo_row("t3", "p2", "owner-19"),
    ]);
    // Batch include: one SELECT … IN (…) for unique project ids.
    exec.push_canned(vec![
        project_row("p1", "Personal", "owner-19"),
        project_row("p2", "Work", "owner-19"),
    ]);

    let req = RpcRequest::new(
        "todo.list",
        json!({
            "include": ["project"],
            "limit": 50
        }),
    )
    .with_cookie(format!("{SESSION_COOKIE_NAME}={token}"));
    let resp = server.handle(&req, &mut exec).await;
    assert!(resp.ok, "{resp:?}");

    let items = resp.data.as_ref().unwrap()["items"].as_array().unwrap();
    assert_eq!(items.len(), 3);
    assert_eq!(items[0]["project"]["title"], "Personal");
    assert_eq!(items[0]["project"]["id"], "p1");
    assert_eq!(items[1]["project"]["title"], "Personal");
    assert_eq!(items[2]["project"]["title"], "Work");

    let selects: Vec<_> = exec
        .log
        .iter()
        .filter(|q| q.kind == "query" && q.sql.contains("SELECT"))
        .collect();
    assert_eq!(
        selects.len(),
        2,
        "expected list + one batch include, got {}:\n{:#?}",
        selects.len(),
        selects
    );
    let batch = selects[1];
    assert!(
        batch.sql.contains("FROM \"project\""),
        "batch should hit project: {}",
        batch.sql
    );
    assert!(
        batch.sql.contains(" IN ("),
        "batch should use IN (…), not per-row find: {}",
        batch.sql
    );
    assert_eq!(batch.params.len(), 2, "unique FK ids only: {:?}", batch.params);
    assert!(batch.params.contains(&SqlValue::Uuid("p1".into())));
    assert!(batch.params.contains(&SqlValue::Uuid("p2".into())));
}

#[tokio::test]
async fn list_include_string_form_works() {
    let mut server = load_server();
    let user = puddly_auth::AuthUser::new("owner-19b", "o19b@x.co");
    let token = server.sessions.create(&user, 3600).await.unwrap();
    let mut exec = RecordingExecutor::new();
    exec.push_canned(vec![todo_row("t9", "p9", "owner-19b")]);
    exec.push_canned(vec![project_row("p9", "Solo", "owner-19b")]);

    let req = RpcRequest::new("todo.list", json!({ "include": "project" }))
        .with_cookie(format!("{SESSION_COOKIE_NAME}={token}"));
    let resp = server.handle(&req, &mut exec).await;
    assert!(resp.ok, "{resp:?}");
    assert_eq!(resp.data.as_ref().unwrap()["items"][0]["project"]["title"], "Solo");
}

#[tokio::test]
async fn list_include_rejects_non_ref_field() {
    let mut server = load_server();
    let user = puddly_auth::AuthUser::new("owner-19c", "o19c@x.co");
    let token = server.sessions.create(&user, 3600).await.unwrap();
    let mut exec = RecordingExecutor::new();

    let req = RpcRequest::new("todo.list", json!({ "include": ["title"] }))
        .with_cookie(format!("{SESSION_COOKIE_NAME}={token}"));
    let resp = server.handle(&req, &mut exec).await;
    assert!(!resp.ok);
    assert_eq!(resp.error.as_ref().unwrap().code, "bad_request");
}
