//! Action IPC JSONL protocol + Node sidecar runner (Steps 8–10).
//!
//! Protocol:
//! - Host writes one request line to the sidecar stdin.
//! - Sidecar writes one response line to stdout.
//!
//! In-repo sidecar: `tools/puddly-action-sidecar.mjs` (Node 22+ with
//! `--experimental-strip-types` so `*.action.ts` can be imported).
//!
//! Step 10: [`ActionRunner::run`] is async; sync `std::process::Command` runs
//! inside `tokio::task::spawn_blocking`. Protocol and error shapes are unchanged
//! — do **not** freeze [`ActionError`] / IPC error codes in this step.

use crate::error::{ActionError, ActionResult};
use crate::{ActionCall, ActionRunner};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Protocol version — bump when the JSON shape changes incompatibly.
pub const IPC_PROTOCOL_VERSION: u32 = 1;

/// Env var: space-separated argv for the sidecar process.
/// Example: `node --experimental-strip-types /path/to/tools/puddly-action-sidecar.mjs`
pub const ACTION_SIDECAR_ENV: &str = "PUDDLY_ACTION_SIDECAR";

/// Relative path of the in-repo sidecar script (from workspace / app root).
pub const DEFAULT_SIDECAR_SCRIPT: &str = "tools/puddly-action-sidecar.mjs";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct IpcUser {
    pub id: String,
    pub email: String,
    #[serde(default)]
    pub roles: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct IpcRequest {
    pub v: u32,
    pub op: String,
    pub resource: String,
    pub action: String,
    pub input: Value,
    pub source_path: String,
    /// Optional: host may omit source body for large files once sidecar reads path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user: Option<IpcUser>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct IpcErrorBody {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct IpcResponse {
    pub v: u32,
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<IpcErrorBody>,
}

impl IpcRequest {
    pub fn run(call: &ActionCall<'_>) -> Self {
        Self {
            v: IPC_PROTOCOL_VERSION,
            op: "run".into(),
            resource: call.resource.to_string(),
            action: call.action.to_string(),
            input: call.input.clone(),
            source_path: call.source_path.display().to_string(),
            source: Some(call.source.to_string()),
            user: call.user.clone(),
        }
    }

    /// Encode as a single JSON line (no embedded newlines).
    pub fn encode_line(&self) -> ActionResult<String> {
        let mut s = serde_json::to_string(self)
            .map_err(|e| ActionError::msg(format!("ipc encode: {e}")))?;
        if s.contains('\n') {
            return Err(ActionError::msg("ipc request JSON must be a single line"));
        }
        s.push('\n');
        Ok(s)
    }
}

impl IpcResponse {
    pub fn ok_data(data: Value) -> Self {
        Self {
            v: IPC_PROTOCOL_VERSION,
            ok: true,
            data: Some(data),
            error: None,
        }
    }

    pub fn err(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            v: IPC_PROTOCOL_VERSION,
            ok: false,
            data: None,
            error: Some(IpcErrorBody {
                code: code.into(),
                message: message.into(),
            }),
        }
    }

    pub fn encode_line(&self) -> ActionResult<String> {
        let mut s = serde_json::to_string(self)
            .map_err(|e| ActionError::msg(format!("ipc encode: {e}")))?;
        s.push('\n');
        Ok(s)
    }

    pub fn decode_line(line: &str) -> ActionResult<Self> {
        let line = line.trim();
        let resp: Self = serde_json::from_str(line)
            .map_err(|e| ActionError::msg(format!("ipc decode: {e}")))?;
        if resp.v != IPC_PROTOCOL_VERSION {
            return Err(ActionError::msg(format!(
                "ipc protocol mismatch: got v{}, expected v{IPC_PROTOCOL_VERSION}",
                resp.v
            )));
        }
        Ok(resp)
    }
}

/// Runner that speaks the IPC protocol via an argv (typically `node … sidecar.mjs`).
///
/// - [`IpcActionRunner::stub`]: no process — returns `sidecar_not_configured`.
/// - [`IpcActionRunner::argv`]: spawn `argv[0]` with the remaining args per call.
/// - [`IpcActionRunner::node_sidecar`]: convenience for the in-repo script.
/// - [`IpcActionRunner::from_env`]: parse [`ACTION_SIDECAR_ENV`], else stub.
#[derive(Debug, Clone)]
pub struct IpcActionRunner {
    argv: Option<Vec<String>>,
}

impl IpcActionRunner {
    pub fn stub() -> Self {
        Self { argv: None }
    }

    /// Full argv, e.g. `["node", "--experimental-strip-types", "/abs/sidecar.mjs"]`.
    pub fn argv(parts: impl IntoIterator<Item = impl Into<String>>) -> Self {
        let argv: Vec<String> = parts.into_iter().map(Into::into).collect();
        Self {
            argv: if argv.is_empty() { None } else { Some(argv) },
        }
    }

    /// Legacy helper: single program path with no args.
    pub fn command(program: impl Into<PathBuf>) -> Self {
        Self::argv([program.into().display().to_string()])
    }

    /// In-repo Node sidecar under `root/tools/puddly-action-sidecar.mjs`.
    pub fn node_sidecar(root: &Path) -> Self {
        let script = root.join(DEFAULT_SIDECAR_SCRIPT);
        Self::argv([
            "node".into(),
            "--experimental-strip-types".into(),
            script.display().to_string(),
        ])
    }

    /// Prefer `$PUDDLY_ACTION_SIDECAR` (shell-style words); else stub.
    pub fn from_env() -> Self {
        match std::env::var(ACTION_SIDECAR_ENV) {
            Ok(s) if !s.trim().is_empty() => {
                let parts: Vec<String> = shell_words(&s);
                Self::argv(parts)
            }
            _ => Self::stub(),
        }
    }

    /// `$PUDDLY_ACTION_SIDECAR` if set, otherwise [`node_sidecar`] when the script
    /// is found at `root` or an ancestor (so `serve examples/todo-slice` still
    /// picks up the monorepo `tools/puddly-action-sidecar.mjs`).
    pub fn from_env_or_workspace(root: &Path) -> Self {
        if std::env::var(ACTION_SIDECAR_ENV)
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false)
        {
            return Self::from_env();
        }
        match find_sidecar_workspace(root) {
            Some(ws) => Self::node_sidecar(&ws),
            None => Self::stub(),
        }
    }

    pub fn is_configured(&self) -> bool {
        self.argv.as_ref().map(|a| !a.is_empty()).unwrap_or(false)
    }

    /// Blocking IPC round-trip (runs on a blocking thread pool via `spawn_blocking`).
    fn run_blocking(
        argv: Option<Vec<String>>,
        call_resource: &str,
        call_action: &str,
        call_path: &str,
        req: IpcRequest,
    ) -> ActionResult<Value> {
        let line = req.encode_line()?;

        let Some(argv) = argv else {
            let _ = IpcResponse::err(
                "sidecar_not_configured",
                format!(
                    "IPC protocol v{IPC_PROTOCOL_VERSION} ready; set {ACTION_SIDECAR_ENV} or use IpcActionRunner::node_sidecar for `{call_resource}.{call_action}`",
                ),
            )
            .encode_line()?;
            return Err(ActionError::NotExecutable {
                resource: call_resource.to_string(),
                action: call_action.to_string(),
                path: call_path.to_string(),
            });
        };

        let (program, args) = argv
            .split_first()
            .ok_or_else(|| ActionError::msg("ipc argv empty"))?;

        let mut child = Command::new(program)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| ActionError::msg(format!("ipc spawn `{program}`: {e}")))?;

        {
            let mut stdin = child
                .stdin
                .take()
                .ok_or_else(|| ActionError::msg("ipc missing stdin"))?;
            stdin
                .write_all(line.as_bytes())
                .map_err(|e| ActionError::msg(format!("ipc write: {e}")))?;
            // Drop stdin so the sidecar sees EOF after one line.
        }

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| ActionError::msg("ipc missing stdout"))?;
        let mut reader = BufReader::new(stdout);
        let mut out_line = String::new();
        reader
            .read_line(&mut out_line)
            .map_err(|e| ActionError::msg(format!("ipc read: {e}")))?;

        let stderr = child.stderr.take().map(|err| {
            let mut s = String::new();
            let mut r = BufReader::new(err);
            let _ = r.read_line(&mut s);
            s
        });

        let status = child
            .wait()
            .map_err(|e| ActionError::msg(format!("ipc wait: {e}")))?;
        if !status.success() {
            let err_hint = stderr.unwrap_or_default();
            return Err(ActionError::msg(format!(
                "ipc sidecar exited with {status}: {err_hint}"
            )));
        }

        let resp = IpcResponse::decode_line(&out_line)?;
        if resp.ok {
            Ok(resp.data.unwrap_or_else(|| json!({})))
        } else {
            let msg = resp
                .error
                .map(|e| format!("{}: {}", e.code, e.message))
                .unwrap_or_else(|| "ipc error".into());
            Err(ActionError::msg(msg))
        }
    }
}

/// Walk `start` and parents for a directory containing [`DEFAULT_SIDECAR_SCRIPT`].
pub fn find_sidecar_workspace(start: &Path) -> Option<PathBuf> {
    let mut cur = if start.is_file() {
        start.parent()?.to_path_buf()
    } else {
        start.to_path_buf()
    };
    loop {
        if cur.join(DEFAULT_SIDECAR_SCRIPT).is_file() {
            return Some(cur);
        }
        if !cur.pop() {
            return None;
        }
    }
}

#[async_trait]
impl ActionRunner for IpcActionRunner {
    async fn run(&mut self, call: ActionCall<'_>) -> ActionResult<Value> {
        let req = IpcRequest::run(&call);
        let argv = self.argv.clone();
        let resource = call.resource.to_string();
        let action = call.action.to_string();
        let path = call.source_path.display().to_string();

        // Sync Command stays blocking; wrap so the Tokio runtime is not stalled.
        // Protocol untouched — ActionError / IPC codes are not frozen this step.
        tokio::task::spawn_blocking(move || {
            Self::run_blocking(argv, &resource, &action, &path, req)
        })
        .await
        .map_err(|e| ActionError::msg(format!("ipc spawn_blocking join: {e}")))?
    }
}

/// Minimal whitespace splitter (no quotes) for `PUDDLY_ACTION_SIDECAR`.
fn shell_words(s: &str) -> Vec<String> {
    s.split_whitespace().map(str::to_string).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn roundtrip_request_response_lines() {
        let call = ActionCall {
            resource: "todo",
            action: "archive",
            input: &json!({"id": "1"}),
            source_path: Path::new("./archive.action.ts"),
            source: "export async function run() {}",
            user: Some(IpcUser {
                id: "u1".into(),
                email: "a@b.co".into(),
                roles: vec![],
            }),
        };
        let req = IpcRequest::run(&call);
        assert_eq!(req.user.as_ref().unwrap().id, "u1");
        let line = req.encode_line().unwrap();
        assert!(line.ends_with('\n'));
        assert_eq!(line.matches('\n').count(), 1);

        let resp = IpcResponse::ok_data(json!({"archived": true}));
        let out = resp.encode_line().unwrap();
        let decoded = IpcResponse::decode_line(&out).unwrap();
        assert!(decoded.ok);
        assert_eq!(decoded.data.unwrap()["archived"], true);
    }
}
