//! Action IPC JSONL protocol + Node sidecar runner (Steps 8–13 / 28).
//!
//! Protocol v2 (ADR-031 / ADR-046 / ADR-049):
//! - Host and sidecar exchange JSONL on stdin/stdout.
//! - Every request/response carries `id` (string or u64) for multiplexing.
//! - Long-lived worker is the default; set `PUDDLY_ACTION_SIDECAR_ONESHOT=1`
//!   for process-per-call.
//! - Additive nested `op: "db"` from sidecar → host while a `run` is in flight
//!   (action `ctx.db` helpers). Keep stdin open until the final `run` response.
//! - Sidecar error codes map to RPC/REST HTTP status (ADR-049).
//!
//! Sidecar template: `tools/puddly-action-sidecar.mjs`.
//! Generated copy: `.puddly/generated/action-sidecar.mjs` (`puddly build`).

use crate::db_host::ActionDbHost;
use crate::error::{ActionError, ActionResult};
use crate::error_codes::IpcErrorCode;
use crate::{ActionCall, ActionRunner};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::str::FromStr;
use std::sync::Mutex;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};

/// Protocol version — bump when the JSON shape changes incompatibly.
pub const IPC_PROTOCOL_VERSION: u32 = 2;

/// Env var: space-separated argv for the sidecar process.
/// Example: `node --experimental-strip-types /path/to/tools/puddly-action-sidecar.mjs`
pub const ACTION_SIDECAR_ENV: &str = "PUDDLY_ACTION_SIDECAR";

/// When set to `1` / `true` / `yes`, spawn a fresh sidecar process per call.
pub const ACTION_SIDECAR_ONESHOT_ENV: &str = "PUDDLY_ACTION_SIDECAR_ONESHOT";

/// Relative path of the in-repo sidecar script (from workspace / app root).
pub const DEFAULT_SIDECAR_SCRIPT: &str = "tools/puddly-action-sidecar.mjs";

/// Relative path of the packaging-B generated sidecar.
pub const GENERATED_SIDECAR_SCRIPT: &str = ".puddly/generated/action-sidecar.mjs";

/// Request/response correlation id (string or u64 on the wire).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(untagged)]
pub enum IpcId {
    Str(String),
    Num(u64),
}

impl IpcId {
    pub fn from_u64(n: u64) -> Self {
        Self::Num(n)
    }

    pub fn as_display(&self) -> String {
        match self {
            Self::Str(s) => s.clone(),
            Self::Num(n) => n.to_string(),
        }
    }
}

impl PartialEq<str> for IpcId {
    fn eq(&self, other: &str) -> bool {
        match self {
            Self::Str(s) => s == other,
            Self::Num(n) => n.to_string() == other,
        }
    }
}

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
    pub id: IpcId,
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
    pub id: IpcId,
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<IpcErrorBody>,
}

/// Nested DB request from sidecar → host during a `run` (ADR-046).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct IpcDbRequest {
    pub v: u32,
    pub id: IpcId,
    pub op: String,
    pub method: String,
    pub resource: String,
    #[serde(default)]
    pub input: Value,
}

impl IpcDbRequest {
    pub fn decode_line(line: &str) -> Option<Self> {
        let line = line.trim();
        let v: Value = serde_json::from_str(line).ok()?;
        if v.get("op").and_then(|o| o.as_str()) != Some("db") {
            return None;
        }
        serde_json::from_value(v).ok()
    }
}

impl IpcRequest {
    pub fn run(call: &ActionCall<'_>) -> Self {
        Self::run_with_id(call, IpcId::Num(0))
    }

    pub fn run_with_id(call: &ActionCall<'_>, id: IpcId) -> Self {
        Self {
            v: IPC_PROTOCOL_VERSION,
            id,
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
    pub fn ok_data(id: IpcId, data: Value) -> Self {
        Self {
            v: IPC_PROTOCOL_VERSION,
            id,
            ok: true,
            data: Some(data),
            error: None,
        }
    }

    pub fn err(id: IpcId, code: IpcErrorCode, message: impl Into<String>) -> Self {
        Self {
            v: IPC_PROTOCOL_VERSION,
            id,
            ok: false,
            data: None,
            error: Some(IpcErrorBody {
                code: code.as_str().to_string(),
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

struct LiveWorker {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
}

/// Runner that speaks the IPC protocol via an argv (typically `node … sidecar.mjs`).
///
/// - [`IpcActionRunner::stub`]: no process — returns `sidecar_not_configured`.
/// - [`IpcActionRunner::argv`]: spawn `argv[0]` with the remaining args.
/// - [`IpcActionRunner::node_sidecar`]: convenience for the in-repo script.
/// - [`IpcActionRunner::from_env`]: parse [`ACTION_SIDECAR_ENV`], else stub.
/// - Default mode: long-lived worker (one process, many JSONL requests).
/// - Oneshot: [`ACTION_SIDECAR_ONESHOT_ENV`] or [`IpcActionRunner::with_oneshot`].
pub struct IpcActionRunner {
    argv: Option<Vec<String>>,
    oneshot: bool,
    worker: Mutex<Option<LiveWorker>>,
}

impl std::fmt::Debug for IpcActionRunner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IpcActionRunner")
            .field("argv", &self.argv)
            .field("oneshot", &self.oneshot)
            .field("live", &self.worker.lock().ok().and_then(|g| g.as_ref().map(|_| true)))
            .finish()
    }
}

impl Drop for IpcActionRunner {
    fn drop(&mut self) {
        if let Ok(mut guard) = self.worker.lock() {
            if let Some(mut w) = guard.take() {
                let _ = w.child.start_kill();
            }
        }
    }
}

impl IpcActionRunner {
    pub fn stub() -> Self {
        Self {
            argv: None,
            oneshot: oneshot_from_env(),
            worker: Mutex::new(None),
        }
    }

    /// Full argv, e.g. `["node", "--experimental-strip-types", "/abs/sidecar.mjs"]`.
    pub fn argv(parts: impl IntoIterator<Item = impl Into<String>>) -> Self {
        let argv: Vec<String> = parts.into_iter().map(Into::into).collect();
        Self {
            argv: if argv.is_empty() { None } else { Some(argv) },
            oneshot: oneshot_from_env(),
            worker: Mutex::new(None),
        }
    }

    /// Legacy helper: single program path with no args.
    pub fn command(program: impl Into<PathBuf>) -> Self {
        Self::argv([program.into().display().to_string()])
    }

    /// Force process-per-call (or clear it).
    pub fn with_oneshot(mut self, oneshot: bool) -> Self {
        self.oneshot = oneshot;
        self
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

    /// Prefer generated `.puddly/generated/action-sidecar.mjs` under `root`.
    pub fn generated_sidecar(root: &Path) -> Self {
        let script = root.join(GENERATED_SIDECAR_SCRIPT);
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

    /// `$PUDDLY_ACTION_SIDECAR` if set; else prefer
    /// `.puddly/generated/action-sidecar.mjs` (walk-up); else `tools/` script
    /// (walk-up); else stub.
    pub fn from_env_or_workspace(root: &Path) -> Self {
        if std::env::var(ACTION_SIDECAR_ENV)
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false)
        {
            return Self::from_env();
        }
        if let Some(path) = find_generated_sidecar(root) {
            return Self::argv([
                "node".into(),
                "--experimental-strip-types".into(),
                path.display().to_string(),
            ]);
        }
        match find_sidecar_workspace(root) {
            Some(ws) => Self::node_sidecar(&ws),
            None => Self::stub(),
        }
    }

    pub fn is_configured(&self) -> bool {
        self.argv.as_ref().map(|a| !a.is_empty()).unwrap_or(false)
    }

    pub fn is_oneshot(&self) -> bool {
        self.oneshot
    }

    /// OS pid of the long-lived worker, if running.
    pub fn live_pid(&self) -> Option<u32> {
        self.worker
            .lock()
            .ok()?
            .as_ref()
            .and_then(|w| w.child.id())
    }

    async fn run_oneshot(
        argv: &[String],
        call_resource: &str,
        call_action: &str,
        call_path: &str,
        mut req: IpcRequest,
        db_host: Option<&mut dyn ActionDbHost>,
    ) -> ActionResult<Value> {
        let id = IpcId::Num(1);
        req.id = id.clone();
        let line = req.encode_line()?;

        let (program, args) = argv
            .split_first()
            .ok_or_else(|| ActionError::msg("ipc argv empty"))?;

        let mut child = Command::new(program)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| ActionError::msg(format!("ipc spawn `{program}`: {e}")))?;

        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| ActionError::msg("ipc missing stdin"))?;
        stdin
            .write_all(line.as_bytes())
            .await
            .map_err(|e| ActionError::msg(format!("ipc write: {e}")))?;
        stdin
            .flush()
            .await
            .map_err(|e| ActionError::msg(format!("ipc flush: {e}")))?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| ActionError::msg("ipc missing stdout"))?;
        let mut reader = BufReader::new(stdout);

        // Keep stdin open for nested `op: "db"` round-trips (ADR-046), then EOF.
        let result = drive_roundtrip(&mut stdin, &mut reader, &id, db_host).await;

        drop(stdin);
        let status = child
            .wait()
            .await
            .map_err(|e| ActionError::msg(format!("ipc wait: {e}")))?;
        if result.is_ok() && !status.success() {
            return Err(ActionError::msg(format!(
                "ipc sidecar exited with {status} for `{call_resource}.{call_action}` ({call_path})"
            )));
        }
        result
    }

    async fn ensure_worker(argv: &[String]) -> ActionResult<LiveWorker> {
        let (program, args) = argv
            .split_first()
            .ok_or_else(|| ActionError::msg("ipc argv empty"))?;

        let mut child = Command::new(program)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| ActionError::msg(format!("ipc spawn `{program}`: {e}")))?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| ActionError::msg("ipc missing stdin"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| ActionError::msg("ipc missing stdout"))?;

        Ok(LiveWorker {
            child,
            stdin,
            stdout: BufReader::new(stdout),
            next_id: 1,
        })
    }

    async fn run_live(
        worker: &mut LiveWorker,
        mut req: IpcRequest,
        db_host: Option<&mut dyn ActionDbHost>,
    ) -> ActionResult<Value> {
        let id = IpcId::Num(worker.next_id);
        worker.next_id = worker.next_id.saturating_add(1);
        req.id = id.clone();
        let line = req.encode_line()?;

        worker
            .stdin
            .write_all(line.as_bytes())
            .await
            .map_err(|e| ActionError::msg(format!("ipc write: {e}")))?;
        worker
            .stdin
            .flush()
            .await
            .map_err(|e| ActionError::msg(format!("ipc flush: {e}")))?;

        drive_roundtrip(&mut worker.stdin, &mut worker.stdout, &id, db_host).await
    }
}

/// Read sidecar stdout until the final `run` response for `expected_id`.
/// Nested `op: "db"` lines are answered on stdin via `db_host`.
async fn drive_roundtrip(
    stdin: &mut ChildStdin,
    stdout: &mut BufReader<ChildStdout>,
    expected_id: &IpcId,
    mut db_host: Option<&mut dyn ActionDbHost>,
) -> ActionResult<Value> {
    loop {
        let mut out_line = String::new();
        let n = stdout
            .read_line(&mut out_line)
            .await
            .map_err(|e| ActionError::msg(format!("ipc read: {e}")))?;
        if n == 0 {
            return Err(ActionError::msg("ipc sidecar closed stdout"));
        }

        if let Some(db_req) = IpcDbRequest::decode_line(&out_line) {
            let resp = match db_host.as_mut() {
                Some(host) => match host
                    .db_op(&db_req.method, &db_req.resource, &db_req.input)
                    .await
                {
                    Ok(data) => IpcResponse::ok_data(db_req.id, data),
                    Err(ActionError::Db { code, message }) => {
                        IpcResponse::err(db_req.id, code, message)
                    }
                    Err(e) => IpcResponse::err(db_req.id, IpcErrorCode::DbFailed, e.to_string()),
                },
                None => IpcResponse::err(
                    db_req.id,
                    IpcErrorCode::DbFailed,
                    "no db host configured for this action call",
                ),
            };
            let resp_line = resp.encode_line()?;
            stdin
                .write_all(resp_line.as_bytes())
                .await
                .map_err(|e| ActionError::msg(format!("ipc db reply write: {e}")))?;
            stdin
                .flush()
                .await
                .map_err(|e| ActionError::msg(format!("ipc db reply flush: {e}")))?;
            continue;
        }

        let resp = IpcResponse::decode_line(&out_line)?;
        if &resp.id != expected_id {
            continue;
        }
        if resp.ok {
            return Ok(resp.data.unwrap_or_else(|| json!({})));
        }
        // Preserve frozen wire codes for HTTP mapping (ADR-049).
        return Err(match resp.error {
            Some(e) => match IpcErrorCode::from_str(&e.code) {
                Ok(code) => ActionError::db(code, e.message),
                Err(_) => ActionError::msg(format!("{}: {}", e.code, e.message)),
            },
            None => ActionError::msg("ipc error"),
        });
    }
}

/// Walk `start` and parents for a directory containing [`DEFAULT_SIDECAR_SCRIPT`].
pub fn find_sidecar_workspace(start: &Path) -> Option<PathBuf> {
    walk_for_file(start, DEFAULT_SIDECAR_SCRIPT)
}

/// Walk `start` and parents for [`.puddly/generated/action-sidecar.mjs`](GENERATED_SIDECAR_SCRIPT).
pub fn find_generated_sidecar(start: &Path) -> Option<PathBuf> {
    let mut cur = if start.is_file() {
        start.parent()?.to_path_buf()
    } else {
        start.to_path_buf()
    };
    loop {
        let candidate = cur.join(GENERATED_SIDECAR_SCRIPT);
        if candidate.is_file() {
            return Some(candidate);
        }
        if !cur.pop() {
            return None;
        }
    }
}

fn walk_for_file(start: &Path, rel: &str) -> Option<PathBuf> {
    let mut cur = if start.is_file() {
        start.parent()?.to_path_buf()
    } else {
        start.to_path_buf()
    };
    loop {
        if cur.join(rel).is_file() {
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
        let resource = call.resource;
        let action = call.action;
        let path = call.source_path.display().to_string();
        let db_host = call.db_host;

        let Some(argv) = self.argv.clone() else {
            let _ = IpcResponse::err(
                IpcId::Num(0),
                IpcErrorCode::SidecarNotConfigured,
                format!(
                    "IPC protocol v{IPC_PROTOCOL_VERSION} ready; set {ACTION_SIDECAR_ENV} or use IpcActionRunner::node_sidecar for `{resource}.{action}`",
                ),
            )
            .encode_line()?;
            return Err(ActionError::NotExecutable {
                resource: resource.to_string(),
                action: action.to_string(),
                path,
            });
        };

        if self.oneshot {
            return Self::run_oneshot(&argv, resource, action, &path, req, db_host).await;
        }

        // Long-lived: take the worker out of the mutex (never hold std::sync::MutexGuard
        // across `.await`), run the round-trip, then put it back. Contended callers
        // either wait on the mutex or spawn briefly if the slot is empty mid-flight
        // (ActionRunner is `&mut self`, so the common path is single-flight).
        let existing = {
            let mut guard = self
                .worker
                .lock()
                .map_err(|_| ActionError::msg("ipc worker lock poisoned"))?;
            guard.take()
        };
        let mut worker = match existing {
            Some(w) => w,
            None => Self::ensure_worker(&argv).await?,
        };

        let result = Self::run_live(&mut worker, req, db_host).await;

        // Put the worker back unless the session is dead.
        let restore = result.is_ok();
        {
            let mut guard = self
                .worker
                .lock()
                .map_err(|_| ActionError::msg("ipc worker lock poisoned"))?;
            if restore {
                *guard = Some(worker);
            } else {
                let _ = worker.child.start_kill();
                *guard = None;
            }
        }
        result
    }
}

/// Minimal whitespace splitter (no quotes) for `PUDDLY_ACTION_SIDECAR`.
fn shell_words(s: &str) -> Vec<String> {
    s.split_whitespace().map(str::to_string).collect()
}

fn oneshot_from_env() -> bool {
    match std::env::var(ACTION_SIDECAR_ONESHOT_ENV) {
        Ok(s) => {
            let t = s.trim();
            t == "1" || t.eq_ignore_ascii_case("true") || t.eq_ignore_ascii_case("yes")
        }
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use std::str::FromStr;

    #[test]
    fn roundtrip_request_response_lines_v2() {
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
            db_host: None,
        };
        let req = IpcRequest::run_with_id(&call, IpcId::Str("req-1".into()));
        assert_eq!(req.v, 2);
        assert_eq!(req.id, IpcId::Str("req-1".into()));
        assert_eq!(req.user.as_ref().unwrap().id, "u1");
        let line = req.encode_line().unwrap();
        assert!(line.ends_with('\n'));
        assert_eq!(line.matches('\n').count(), 1);
        assert!(line.contains("\"id\":\"req-1\""));

        let resp = IpcResponse::ok_data(IpcId::Str("req-1".into()), json!({"archived": true}));
        let out = resp.encode_line().unwrap();
        let decoded = IpcResponse::decode_line(&out).unwrap();
        assert!(decoded.ok);
        assert_eq!(decoded.id, IpcId::Str("req-1".into()));
        assert_eq!(decoded.data.unwrap()["archived"], true);

        let num_id = IpcId::Num(42);
        let resp2 = IpcResponse::ok_data(num_id.clone(), json!({}));
        let decoded2 = IpcResponse::decode_line(&resp2.encode_line().unwrap()).unwrap();
        assert_eq!(decoded2.id, num_id);
    }

    #[test]
    fn error_codes_frozen_and_parseable() {
        for code in IpcErrorCode::ALL {
            assert_eq!(IpcErrorCode::from_str(code.as_str()).unwrap(), *code);
        }
        let resp = IpcResponse::err(
            IpcId::Num(1),
            IpcErrorCode::ImportFailed,
            "boom",
        );
        assert_eq!(resp.error.as_ref().unwrap().code, "import_failed");
    }

    #[test]
    fn protocol_version_is_two() {
        assert_eq!(IPC_PROTOCOL_VERSION, 2);
    }
}
