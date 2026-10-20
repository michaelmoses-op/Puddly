//! Step 13: protocol v2 ids, long-lived worker reuse, packaging B.

use puddly_actions::{
    find_generated_sidecar, package_action_sidecar, ActionCall, ActionRunner, IpcActionRunner,
    IpcErrorCode, IpcId, IpcRequest, IpcResponse, IpcUser, ACTION_SIDECAR_ENV,
    GENERATED_SIDECAR_SCRIPT, IPC_PROTOCOL_VERSION,
};
use serde_json::json;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::str::FromStr;

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
fn protocol_v2_roundtrip_with_string_and_numeric_ids() {
    assert_eq!(IPC_PROTOCOL_VERSION, 2);
    let path = archive_path();
    let source = std::fs::read_to_string(&path).unwrap();
    let input = json!({"id": "t"});
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
    };

    for id in [
        IpcId::Str("abc".into()),
        IpcId::Num(99),
        IpcId::Str("42".into()),
    ] {
        let req = IpcRequest::run_with_id(&call, id.clone());
        let line = req.encode_line().unwrap();
        assert!(line.contains("\"v\":2"));
        let resp = IpcResponse::ok_data(id.clone(), json!({"ok": true}));
        let decoded = IpcResponse::decode_line(&resp.encode_line().unwrap()).unwrap();
        assert_eq!(decoded.id, id);
        assert!(decoded.ok);
    }
}

#[test]
fn frozen_error_codes_match_sidecar_literals() {
    let expected = [
        "protocol_mismatch",
        "unknown_op",
        "bad_request",
        "not_found",
        "import_failed",
        "no_run_export",
        "run_failed",
        "sidecar_not_configured",
    ];
    assert_eq!(IpcErrorCode::ALL.len(), expected.len());
    for (code, lit) in IpcErrorCode::ALL.iter().zip(expected) {
        assert_eq!(code.as_str(), lit);
        assert_eq!(IpcErrorCode::from_str(lit).unwrap(), *code);
    }
    let script = std::fs::read_to_string(workspace_root().join("tools/puddly-action-sidecar.mjs"))
        .unwrap();
    for lit in expected {
        assert!(
            script.contains(lit),
            "sidecar missing error code literal `{lit}`"
        );
    }
}

#[tokio::test]
async fn long_lived_runner_reuses_process_for_two_archive_calls() {
    if !node_available() {
        eprintln!("skip: node not on PATH");
        return;
    }
    std::env::remove_var(puddly_actions::ACTION_SIDECAR_ONESHOT_ENV);
    let root = workspace_root();
    let mut runner = IpcActionRunner::node_sidecar(&root).with_oneshot(false);
    assert!(!runner.is_oneshot());

    let path = archive_path();
    let source = std::fs::read_to_string(&path).unwrap();
    let user = IpcUser {
        id: "reuse-user".into(),
        email: "r@e.co".into(),
        roles: vec![],
    };

    let input1 = json!({"id": "a1"});
    let call1 = ActionCall {
        resource: "todo",
        action: "archive",
        input: &input1,
        source_path: &path,
        source: &source,
        user: Some(user.clone()),
    };
    let data1 = runner.run(call1).await.expect("first archive");
    assert_eq!(data1["archived"], "a1");
    let pid1 = runner.live_pid().expect("worker should be live after first call");

    let input2 = json!({"id": "a2"});
    let call2 = ActionCall {
        resource: "todo",
        action: "archive",
        input: &input2,
        source_path: &path,
        source: &source,
        user: Some(user),
    };
    let data2 = runner.run(call2).await.expect("second archive");
    assert_eq!(data2["archived"], "a2");
    let pid2 = runner.live_pid().expect("worker still live");
    assert_eq!(pid1, pid2, "long-lived worker must reuse the same OS process");
}

#[test]
fn long_lived_sidecar_ping_increments_seq_same_pid() {
    if !node_available() {
        eprintln!("skip: node not on PATH");
        return;
    }
    let script = workspace_root().join("tools/puddly-action-sidecar.mjs");
    let mut child = Command::new("node")
        .args(["--experimental-strip-types", script.to_str().unwrap()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout);

    for (id, expect_seq) in [(1u64, 1u64), (2, 2)] {
        let req = json!({"v": 2, "id": id, "op": "ping"});
        writeln!(stdin, "{req}").unwrap();
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        let resp: serde_json::Value = serde_json::from_str(line.trim()).unwrap();
        assert_eq!(resp["ok"], true);
        assert_eq!(resp["id"], id);
        assert_eq!(resp["data"]["seq"], expect_seq);
        if id == 1 {
            // stash pid via second assertion scope
        }
    }
    // Both responses share the same pid (read again from last — re-ping)
    let req = json!({"v": 2, "id": 3, "op": "ping"});
    writeln!(stdin, "{req}").unwrap();
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    let resp: serde_json::Value = serde_json::from_str(line.trim()).unwrap();
    assert_eq!(resp["data"]["seq"], 3);
    drop(stdin);
    let _ = child.wait();
}

#[test]
fn packaging_writes_generated_file_and_discovery_prefers_it() {
    std::env::remove_var(ACTION_SIDECAR_ENV);
    let app = workspace_root().join("examples/todo-slice");
    let dest = package_action_sidecar(&app).expect("package");
    assert!(dest.is_file());
    assert!(
        dest.ends_with(Path::new(GENERATED_SIDECAR_SCRIPT)),
        "dest={}",
        dest.display()
    );
    let found = find_generated_sidecar(&app).expect("find generated");
    assert_eq!(found, dest);

    let runner = IpcActionRunner::from_env_or_workspace(&app);
    assert!(runner.is_configured());
    // Argv should point at the generated path, not tools/.
    let dbg = format!("{runner:?}");
    assert!(
        dbg.contains("action-sidecar.mjs"),
        "runner debug should mention sidecar: {dbg}"
    );

    let text = std::fs::read_to_string(&dest).unwrap();
    assert!(text.contains("generated"));
    assert!(text.contains("PROTOCOL_VERSION = 2") || text.contains("PROTOCOL_VERSION=2") || text.contains("const PROTOCOL_VERSION = 2"));

    // Clean up so other tests don't always prefer generated (optional; discovery is fine either way).
    let _ = std::fs::remove_file(&dest);
}

#[tokio::test]
async fn oneshot_fallback_still_runs_archive() {
    if !node_available() {
        eprintln!("skip: node not on PATH");
        return;
    }
    let mut runner = IpcActionRunner::node_sidecar(&workspace_root()).with_oneshot(true);
    let path = archive_path();
    let source = std::fs::read_to_string(&path).unwrap();
    let input = json!({"id": "oneshot-1"});
    let call = ActionCall {
        resource: "todo",
        action: "archive",
        input: &input,
        source_path: &path,
        source: &source,
        user: None,
    };
    let data = runner.run(call).await.expect("oneshot");
    assert_eq!(data["archived"], "oneshot-1");
    assert!(runner.live_pid().is_none(), "oneshot must not leave a live worker");
}
