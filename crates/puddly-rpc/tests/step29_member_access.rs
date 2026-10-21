//! Step 29: join-table `rule.member` list/get + owner regression (ADR-047).

use puddly_auth::SessionStore;
use puddly_check::check;
use puddly_core::{CSRF_COOKIE_NAME, CSRF_HEADER_NAME, SESSION_COOKIE_NAME};
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

#[test]
fn project_list_uses_member_join_in_ir() {
    let ir = check(&todo_slice()).ir.unwrap();
    let project = ir.resources.iter().find(|r| r.name == "project").unwrap();
    let list = project
        .access
        .operations
        .iter()
        .find(|o| o.name == "list")
        .unwrap();
    match &list.rule {
        puddly_ir::RuleIr::Member {
            via,
            on,
            user_field,
            ..
        } => {
            assert_eq!(via, "projectMember");
            assert_eq!(on, "project");
            assert_eq!(user_field, "user");
        }
        other => panic!("expected member list rule, got {other:?}"),
    }
    let delete = project
        .access
        .operations
        .iter()
        .find(|o| o.name == "delete")
        .unwrap();
    match &delete.rule {
        puddly_ir::RuleIr::Owner { field, .. } => assert_eq!(field, "owner"),
        other => panic!("expected owner delete, got {other:?}"),
    }
}

#[tokio::test]
async fn list_injects_member_in_subquery() {
    let mut server = load_server();
    let user = puddly_auth::AuthUser::new("member-1", "m@x.co");
    let token = server.sessions.create(&user, 3600).await.unwrap();
    let mut exec = RecordingExecutor::new();
    exec.push_canned(vec![]);

    let req = RpcRequest::new("project.list", json!({ "limit": 10 }))
        .with_cookie(format!("{SESSION_COOKIE_NAME}={token}"));
    let resp = server.handle(&req, &mut exec).await;
    assert!(resp.ok, "{resp:?}");

    let q = exec.log.iter().find(|q| q.sql.contains("SELECT")).unwrap();
    assert!(
        q.sql.contains("IN (SELECT") && q.sql.contains("\"projectMember\""),
        "expected member IN subquery, got {}",
        q.sql
    );
    assert!(q.sql.contains("\"project\""));
    assert!(q.sql.contains("\"user\""));
    assert_eq!(q.params[0], SqlValue::Uuid("member-1".into()));
}

#[tokio::test]
async fn get_allows_member() {
    let mut server = load_server();
    let user = puddly_auth::AuthUser::new("member-1", "m@x.co");
    let token = server.sessions.create(&user, 3600).await.unwrap();
    let mut exec = RecordingExecutor::new();

    let mut project = DbRow::new();
    project.insert("id", SqlValue::Uuid("p1".into()));
    project.insert("title", SqlValue::Text("Shared".into()));
    project.insert("owner", SqlValue::Uuid("owner-x".into()));
    exec.push_canned(vec![project]);

    let mut membership = DbRow::new();
    membership.insert("ok", SqlValue::Int(1));
    exec.push_canned(vec![membership]);

    let req = RpcRequest::new("project.get", json!({ "id": "p1" }))
        .with_cookie(format!("{SESSION_COOKIE_NAME}={token}"));
    let resp = server.handle(&req, &mut exec).await;
    assert!(resp.ok, "{resp:?}");

    let member_q = exec
        .log
        .iter()
        .find(|q| q.sql.contains("projectMember"))
        .expect("membership check query");
    assert!(member_q.sql.contains("LIMIT 1"));
    assert_eq!(member_q.params[0], SqlValue::Uuid("p1".into()));
    assert_eq!(member_q.params[1], SqlValue::Uuid("member-1".into()));
}

#[tokio::test]
async fn get_forbids_non_member() {
    let mut server = load_server();
    let user = puddly_auth::AuthUser::new("outsider", "o@x.co");
    let token = server.sessions.create(&user, 3600).await.unwrap();
    let mut exec = RecordingExecutor::new();

    let mut project = DbRow::new();
    project.insert("id", SqlValue::Uuid("p1".into()));
    project.insert("title", SqlValue::Text("Shared".into()));
    project.insert("owner", SqlValue::Uuid("owner-x".into()));
    exec.push_canned(vec![project]);
    // Empty membership → forbidden
    exec.push_canned(vec![]);

    let req = RpcRequest::new("project.get", json!({ "id": "p1" }))
        .with_cookie(format!("{SESSION_COOKIE_NAME}={token}"));
    let resp = server.handle(&req, &mut exec).await;
    assert!(!resp.ok);
    assert_eq!(resp.error.as_ref().unwrap().code, "forbidden");
}

#[tokio::test]
async fn owner_list_still_injects_equality() {
    let mut server = load_server();
    let user = puddly_auth::AuthUser::new("owner-1", "o@x.co");
    let token = server.sessions.create(&user, 3600).await.unwrap();
    let mut exec = RecordingExecutor::new();
    exec.push_canned(vec![]);

    let req = RpcRequest::new("todo.list", json!({ "limit": 10 }))
        .with_cookie(format!("{SESSION_COOKIE_NAME}={token}"));
    let resp = server.handle(&req, &mut exec).await;
    assert!(resp.ok, "{resp:?}");

    let q = exec.log.iter().find(|q| q.sql.contains("FROM \"todo\"")).unwrap();
    assert!(
        q.sql.contains("\"owner\"") && !q.sql.contains("projectMember"),
        "todo list should stay owner equality, got {}",
        q.sql
    );
    assert_eq!(q.params[0], SqlValue::Uuid("owner-1".into()));
}

#[tokio::test]
async fn project_delete_still_owner_only() {
    let mut server = load_server();
    let user = puddly_auth::AuthUser::new("member-1", "m@x.co");
    let token = server.sessions.create(&user, 3600).await.unwrap();
    let mut exec = RecordingExecutor::new();

    let mut project = DbRow::new();
    project.insert("id", SqlValue::Uuid("p1".into()));
    project.insert("title", SqlValue::Text("Shared".into()));
    project.insert("owner", SqlValue::Uuid("owner-x".into()));
    exec.push_canned(vec![project]);

    let req = RpcRequest::new("project.delete", json!({ "id": "p1" }))
        .with_header(CSRF_HEADER_NAME, "1")
        .with_cookie(format!("{SESSION_COOKIE_NAME}={token}; {CSRF_COOKIE_NAME}=1"));
    let resp = server.handle(&req, &mut exec).await;
    assert!(!resp.ok);
    assert_eq!(resp.error.as_ref().unwrap().code, "forbidden");
}
