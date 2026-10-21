//! Step 9: real Node sidecar smoke (JSONL → archive.action.ts).
//! Updated for protocol v2 (`id`) in Step 13.

use puddly_actions::{
    ActionCall, ActionRunner, IpcActionRunner, IpcRequest, IpcUser, MockActionDbHost,
    ACTION_SIDECAR_ENV,
};
use serde_json::json;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

fn archive_path() -> PathBuf {
    workspace_root().join("examples/todo-slice/features/todos/archive.action.ts")
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

#[test]
fn sidecar_script_exists() {
    let script = workspace_root().join("tools/puddly-action-sidecar.mjs");
    assert!(script.is_file(), "missing {}", script.display());
}

#[tokio::test]
async fn node_sidecar_runs_archive_action() {
    if !node_available() {
        eprintln!("skip: node not on PATH");
        return;
    }
    let root = workspace_root();
    // Oneshot keeps Step 9 semantics for this smoke (process-per-call).
    let mut runner = IpcActionRunner::node_sidecar(&root).with_oneshot(true);
    assert!(runner.is_configured());

    let path = archive_path();
    let source = std::fs::read_to_string(&path).unwrap();
    let input = json!({"id": "todo-42"});
    let user = IpcUser {
        id: "user-7".into(),
        email: "owner@example.com".into(),
        roles: vec!["editor".into()],
    };
    let mut db = MockActionDbHost::new(|method, resource, input| {
        assert_eq!(method, "get");
        assert_eq!(resource, "todo");
        Ok(json!({
            "id": input["id"].clone(),
            "title": "Buy milk",
            "done": false,
            "project": "p1",
            "owner": "user-7",
        }))
    });
    let call = ActionCall {
        resource: "todo",
        action: "archive",
        input: &input,
        source_path: &path,
        source: &source,
        user: Some(user),
        db_host: Some(&mut db),
    };
    let data = runner.run(call).await.expect("sidecar execute");
    assert_eq!(data["archived"], "todo-42");
    assert_eq!(data["userId"], "user-7");
    assert_eq!(data["title"], "Buy milk");
}

#[test]
fn ipc_request_includes_user_on_the_wire() {
    let path = archive_path();
    let source = std::fs::read_to_string(&path).unwrap();
    let input = json!({"id": "x"});
    let call = ActionCall {
        resource: "todo",
        action: "archive",
        input: &input,
        source_path: &path,
        source: &source,
        user: Some(IpcUser {
            id: "u".into(),
            email: "e@e.co".into(),
            roles: vec![],
        }),
        db_host: None,
    };
    let line = IpcRequest::run(&call).encode_line().unwrap();
    assert!(line.contains("\"user\""));
    assert!(line.contains("\"id\":\"u\""));
    assert!(line.contains("\"v\":2"));
}

#[test]
fn direct_node_jsonl_handshake() {
    if !node_available() {
        eprintln!("skip: node not on PATH");
        return;
    }
    use std::io::{BufRead, BufReader};
    let root = workspace_root();
    let script = root.join("tools/puddly-action-sidecar.mjs");
    let path = archive_path();
    let req = json!({
        "v": 2,
        "id": "direct-1",
        "op": "run",
        "resource": "todo",
        "action": "archive",
        "input": { "id": "direct-1" },
        "sourcePath": path.to_string_lossy(),
        "user": { "id": "u9", "email": "x@y.z", "roles": [] }
    });
    let mut child = Command::new("node")
        .args(["--experimental-strip-types", script.to_str().unwrap()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    writeln!(stdin, "{}", req).unwrap();
    let stdout = child.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout);
    let resp = loop {
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        let msg: serde_json::Value = serde_json::from_str(line.trim()).unwrap();
        if msg.get("op").and_then(|o| o.as_str()) == Some("db") {
            let reply = json!({
                "v": 2,
                "id": msg["id"].clone(),
                "ok": true,
                "data": {
                    "id": "direct-1",
                    "title": "Direct",
                    "done": false,
                    "project": "p1",
                    "owner": "u9",
                }
            });
            writeln!(stdin, "{}", reply).unwrap();
            continue;
        }
        break msg;
    };
    drop(stdin);
    let status = child.wait().unwrap();
    assert!(status.success(), "sidecar exit {status}");
    assert_eq!(resp["ok"], true);
    assert_eq!(resp["v"], 2);
    assert_eq!(resp["id"], "direct-1");
    assert_eq!(resp["data"]["archived"], "direct-1");
    assert_eq!(resp["data"]["userId"], "u9");
    assert_eq!(resp["data"]["title"], "Direct");
}

#[test]
fn from_env_or_workspace_finds_script() {
    // Ensure env override is not required when script exists at workspace root.
    std::env::remove_var(ACTION_SIDECAR_ENV);
    let runner = IpcActionRunner::from_env_or_workspace(&workspace_root());
    assert!(runner.is_configured());
}

#[test]
fn from_env_or_workspace_walks_up_from_app_root() {
    std::env::remove_var(ACTION_SIDECAR_ENV);
    let app = workspace_root().join("examples/todo-slice");
    let runner = IpcActionRunner::from_env_or_workspace(&app);
    assert!(
        runner.is_configured(),
        "should discover tools/ sidecar from examples/todo-slice"
    );
}
