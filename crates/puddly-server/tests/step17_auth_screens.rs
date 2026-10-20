//! Step 17: login/register HTML views + aliases; 401 links to /login.

use puddly_auth::{hash_password, AuthUser, SessionStore};
use puddly_check::check;
use puddly_core::{CSRF_HEADER_NAME, SESSION_COOKIE_NAME};
use puddly_db::{DbRow, RecordingExecutor, SqlValue};
use puddly_rpc::{RpcRequest, RpcServer};
use puddly_server::{dispatch_http, try_dispatch_get_dev, DevHandles, DevState, HttpRequest};
use puddly_views::ViewBundle;
use serde_json::json;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use tokio::sync::Mutex;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

fn example_root() -> PathBuf {
    workspace_root().join("examples/todo-slice")
}

fn load_rpc() -> RpcServer {
    let root = example_root();
    let ir = check(&root).ir.expect("ir");
    RpcServer::from_app(&root, ir).expect("rpc")
}

#[test]
fn discovers_login_and_register_views() {
    let bundle = ViewBundle::load(&example_root()).expect("views");
    assert!(
        bundle.views.contains_key("login"),
        "keys: {:?}",
        bundle.views.keys().collect::<Vec<_>>()
    );
    assert!(bundle.views.contains_key("register"));
    assert!(!bundle.views["login"].load.needs_db(), "login must be static-only");
    assert!(
        !bundle.views["register"].load.needs_db(),
        "register must be static-only"
    );
}

#[tokio::test]
async fn get_login_and_register_return_200_forms_without_db() {
    let root = example_root();
    let views = ViewBundle::load(&root).unwrap();
    let handles = DevHandles::new(views, DevState::new(root), None::<RpcServer>);

    for (path, rpc_op, cross_link) in [
        ("/login", "auth.login", "/register"),
        ("/register", "auth.register", "/login"),
        ("/v/login", "auth.login", "/register"),
        ("/v/register", "auth.register", "/login"),
    ] {
        let req = HttpRequest {
            method: "GET".into(),
            path: path.into(),
            headers: BTreeMap::new(),
            body: Vec::new(),
        };
        let resp = try_dispatch_get_dev(&req, &handles).await.unwrap().unwrap();
        assert_eq!(
            resp.status,
            200,
            "{path}: {}",
            String::from_utf8_lossy(&resp.body)
        );
        let body = String::from_utf8_lossy(&resp.body);
        assert!(
            body.contains(&format!("data-puddly-rpc=\"{rpc_op}\"")),
            "{path} missing rpc form:\n{body}"
        );
        assert!(body.contains("name=\"email\""), "{path}:\n{body}");
        assert!(body.contains("name=\"password\""), "{path}:\n{body}");
        assert!(
            body.contains("data-puddly-redirect=\"/\""),
            "{path} missing redirect:\n{body}"
        );
        assert!(
            body.contains(cross_link),
            "{path} missing link {cross_link}:\n{body}"
        );
        assert!(body.contains("/puddly-islands.js"), "{path}:\n{body}");
    }
}

#[tokio::test]
async fn anonymous_401_html_links_to_login() {
    let rpc = Mutex::new(load_rpc());
    let mut exec = RecordingExecutor::new();
    let views = ViewBundle::load(&example_root()).unwrap();
    let req = HttpRequest {
        method: "GET".into(),
        path: "/".into(),
        headers: BTreeMap::new(),
        body: Vec::new(),
    };
    let resp = dispatch_http(&req, &rpc, &mut exec, Some(&views), None)
        .await
        .unwrap();
    assert_eq!(resp.status, 401);
    let body = String::from_utf8_lossy(&resp.body);
    assert!(body.contains("Sign in required"), "{body}");
    assert!(
        body.contains("href=\"/login\""),
        "401 must link to /login:\n{body}"
    );
}

#[tokio::test]
async fn after_login_cookie_root_succeeds_with_recording_executor() {
    let mut rpc = load_rpc();
    let user = AuthUser::new("owner-17", "o17@x.co");
    let token = rpc.sessions.create(&user, 3600).await.unwrap();
    let rpc = Mutex::new(rpc);

    let mut exec = RecordingExecutor::new();
    let mut row = DbRow::new();
    row.insert("id", SqlValue::Uuid("t17".into()));
    row.insert("title", SqlValue::Text("After login".into()));
    row.insert("done", SqlValue::Bool(false));
    row.insert("owner", SqlValue::Uuid("owner-17".into()));
    exec.push_canned(vec![row]);

    let views = ViewBundle::load(&example_root()).unwrap();
    let mut headers = BTreeMap::new();
    headers.insert(
        "cookie".into(),
        format!("{SESSION_COOKIE_NAME}={token}"),
    );
    let req = HttpRequest {
        method: "GET".into(),
        path: "/".into(),
        headers,
        body: Vec::new(),
    };
    let resp = dispatch_http(&req, &rpc, &mut exec, Some(&views), None)
        .await
        .unwrap();
    assert_eq!(resp.status, 200, "{}", String::from_utf8_lossy(&resp.body));
    let body = String::from_utf8_lossy(&resp.body);
    assert!(body.contains("After login"), "{body}");
    assert!(
        body.contains("data-puddly-rpc=\"auth.logout\""),
        "todo view should offer logout:\n{body}"
    );
}

#[tokio::test]
async fn register_then_login_rpc_still_works() {
    let mut server = load_rpc();
    let mut exec = RecordingExecutor::new();

    exec.push_canned(vec![]); // register: email not taken
    let register = RpcRequest::new(
        "auth.register",
        json!({
            "email": "step17@x.co",
            "password": "secret-17",
            "userId": "user-17"
        }),
    )
    .with_header(CSRF_HEADER_NAME, "1");
    let reg = server.handle(&register, &mut exec).await;
    assert!(reg.ok, "{reg:?}");
    assert!(!reg.set_cookies.is_empty(), "register sets session cookie");

    let hash = hash_password("secret-17").unwrap();
    let mut row = DbRow::new();
    row.insert("user_id", SqlValue::Uuid("user-17".into()));
    row.insert("email", SqlValue::Text("step17@x.co".into()));
    row.insert("password_hash", SqlValue::Text(hash));
    row.insert("roles", SqlValue::Text("[]".into()));
    exec.push_canned(vec![row]);

    let login = RpcRequest::new(
        "auth.login",
        json!({ "email": "step17@x.co", "password": "secret-17" }),
    )
    .with_header(CSRF_HEADER_NAME, "1");
    let resp = server.handle(&login, &mut exec).await;
    assert!(resp.ok, "{resp:?}");
    assert!(!resp.set_cookies.is_empty());
    assert_eq!(resp.data.as_ref().unwrap()["user"]["id"], "user-17");
}
