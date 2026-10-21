//! Step 12: `/__dev` panel, reload helpers, overlay — no long-running server.

use puddly_check::check;
use puddly_db::RecordingExecutor;
use puddly_rpc::RpcServer;
use puddly_server::{
    apply_changes, classify_watch_path, dev_status_response, dispatch_http_dev,
    hot_swap_views, inject_dev_overlay_script, reload_resources, try_dispatch_get_dev,
    ChangeKind, DevHandles, DevState, HttpRequest, DEV_OVERLAY_JS,
};
use puddly_views::ViewBundle;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

fn example_root() -> PathBuf {
    workspace_root().join("examples/todo-slice")
}

fn load_handles() -> Arc<DevHandles> {
    let root = example_root();
    let ir = check(&root).ir.expect("ir");
    let rpc = RpcServer::from_app(&root, ir).expect("rpc");
    let views = ViewBundle::load(&root).expect("views");
    let mut state = DevState::new(root);
    state.mark_reload();
    Arc::new(DevHandles::new(views, state, Some(rpc)))
}

fn load_view_only() -> Arc<DevHandles> {
    let root = example_root();
    let views = ViewBundle::load(&root).expect("views");
    let mut state = DevState::new(root);
    state.mark_reload();
    Arc::new(DevHandles::new(views, state, None))
}

#[test]
fn classify_watch_paths() {
    assert_eq!(
        classify_watch_path(Path::new("x.resource.ts")),
        Some(ChangeKind::Resource)
    );
    assert_eq!(
        classify_watch_path(Path::new("x.action.ts")),
        Some(ChangeKind::Action)
    );
    assert_eq!(
        classify_watch_path(Path::new("x.view.tsx")),
        Some(ChangeKind::View)
    );
    assert_eq!(
        classify_watch_path(Path::new("x.island.tsx")),
        Some(ChangeKind::Island)
    );
}

#[test]
fn dev_status_json_shape() {
    let mut state = DevState::new(PathBuf::from("/app"));
    state.mark_reload();
    state.draft_migration_hint = Some("draft needed".into());
    let resp = dev_status_response(&state);
    assert_eq!(resp.status, 200);
    let v: Value = serde_json::from_slice(&resp.body).unwrap();
    assert_eq!(v["ok"], true);
    assert!(v["lastReload"].is_string());
    assert!(v["errors"].is_array());
    assert_eq!(v["draftMigrationHint"], "draft needed");
    assert_eq!(v["watchedRoot"], "/app");
}

#[test]
fn overlay_js_polls_dev() {
    assert!(DEV_OVERLAY_JS.contains("fetch('/__dev'"));
    assert!(DEV_OVERLAY_JS.contains("puddly-dev-overlay"));
    let html = inject_dev_overlay_script("<html><body>x</body></html>");
    assert!(html.contains("/__dev/overlay.js"));
}

#[tokio::test]
async fn get_dev_panel() {
    let handles = load_handles();
    let req = HttpRequest {
        method: "GET".into(),
        path: "/__dev".into(),
        headers: BTreeMap::new(),
        body: Vec::new(),
    };
    let resp = try_dispatch_get_dev(&req, handles.as_ref())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(resp.status, 200);
    let v: Value = serde_json::from_slice(&resp.body).unwrap();
    assert_eq!(v["ok"], true);
    assert!(v["watchedRoot"].as_str().unwrap().contains("todo-slice"));
}

#[tokio::test]
async fn get_overlay_js() {
    let handles = load_view_only();
    let req = HttpRequest {
        method: "GET".into(),
        path: "/__dev/overlay.js".into(),
        headers: BTreeMap::new(),
        body: Vec::new(),
    };
    let resp = try_dispatch_get_dev(&req, handles.as_ref())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(resp.status, 200);
    let body = String::from_utf8_lossy(&resp.body);
    assert!(body.contains("puddly-dev-overlay"));
}

#[tokio::test]
async fn ssr_injects_overlay_in_dev_when_load_available() {
    // View-only (no DB): todo `load` needs list → 503, still injects overlay into HTML.
    let handles = load_view_only();
    let req = HttpRequest {
        method: "GET".into(),
        path: "/".into(),
        headers: BTreeMap::new(),
        body: Vec::new(),
    };
    let resp = dispatch_http_dev(&req, handles.as_ref(), None)
        .await
        .unwrap();
    assert_eq!(resp.status, 503, "{}", String::from_utf8_lossy(&resp.body));
    let body = String::from_utf8_lossy(&resp.body);
    assert!(body.contains("Unavailable") || body.contains("database"), "{body}");
    assert!(body.contains("/__dev/overlay.js"), "{body}");
}

#[tokio::test]
async fn ssr_overlay_with_session_and_canned_data() {
    let handles = load_handles();
    let user = puddly_auth::AuthUser::new("owner-1", "o@x.co");
    let token = {
        let mut rpc = handles.rpc.as_ref().unwrap().lock().await;
        puddly_auth::SessionStore::create(&mut rpc.sessions, &user, 3600)
            .await
            .unwrap()
    };
    let mut exec = RecordingExecutor::new();
    let mut row = puddly_db::DbRow::new();
    row.insert("id", puddly_db::SqlValue::Uuid("t1".into()));
    row.insert("title", puddly_db::SqlValue::Text("Dev milk".into()));
    row.insert("done", puddly_db::SqlValue::Bool(true));
    row.insert("dueDate", puddly_db::SqlValue::Null);
    row.insert("project", puddly_db::SqlValue::Uuid("p1".into()));
    row.insert("owner", puddly_db::SqlValue::Uuid("owner-1".into()));
    row.insert(
        "createdAt",
        puddly_db::SqlValue::Text("2026-01-01T00:00:00Z".into()),
    );
    exec.push_canned(vec![row]);

    let mut headers = BTreeMap::new();
    headers.insert(
        "cookie".into(),
        format!("{}={token}", puddly_core::SESSION_COOKIE_NAME),
    );
    let req = HttpRequest {
        method: "GET".into(),
        path: "/".into(),
        headers,
        body: Vec::new(),
    };
    let resp = dispatch_http_dev(&req, handles.as_ref(), Some(&mut exec))
        .await
        .unwrap();
    assert_eq!(resp.status, 200, "{}", String::from_utf8_lossy(&resp.body));
    let body = String::from_utf8_lossy(&resp.body);
    assert!(body.contains("Dev milk"), "{body}");
    assert!(body.contains("/__dev/overlay.js"), "{body}");
}

#[tokio::test]
async fn hot_swap_views_roundtrip() {
    let handles = load_view_only();
    let root = example_root();
    hot_swap_views(&handles.views, &root).await.expect("swap");
    let bundle = handles.views.read().await;
    assert!(bundle.views.contains_key("todo"));
}

#[tokio::test]
async fn resource_reload_sets_draft_hint() {
    let handles = load_handles();
    let root = example_root();
    let outcome = reload_resources(&root, &handles.state, handles.rpc.as_ref()).await;
    assert!(outcome.resources_checked);
    let st = handles.state.read().await;
    assert!(st.ok, "{:?}", st.errors);
    assert!(
        st.draft_migration_hint
            .as_deref()
            .unwrap_or("")
            .contains("migrate generate"),
        "{:?}",
        st.draft_migration_hint
    );
}

#[tokio::test]
async fn apply_view_change_reloads_bundle() {
    let handles = load_view_only();
    let root = example_root();
    let outcome = apply_changes::<puddly_auth::MemorySessionStore, puddly_actions::StubActionRunner>(
        &root,
        &[ChangeKind::View],
        &handles.views,
        &handles.state,
        None,
    )
    .await;
    assert!(outcome.views_reloaded);
    assert!(outcome.messages.iter().any(|m| m.contains("ViewBundle")));
}

#[tokio::test]
async fn view_only_rpc_returns_503() {
    let handles = load_view_only();
    let mut exec = RecordingExecutor::new();
    let req = HttpRequest {
        method: "POST".into(),
        path: "/rpc/todo.list".into(),
        headers: BTreeMap::new(),
        body: b"{}".to_vec(),
    };
    let resp = dispatch_http_dev(&req, handles.as_ref(), Some(&mut exec))
        .await
        .unwrap();
    assert_eq!(resp.status, 503);
    assert!(exec.log.is_empty());
}
