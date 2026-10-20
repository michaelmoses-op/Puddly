//! Action IPC protocol stub (Step 8 / Step 7 open question 3.5 B).
//!
//! Frozen **JSON Lines** handshake for a future Node/Bun sidecar:
//! - Host writes one request line to the sidecar stdin.
//! - Sidecar writes one response line to stdout.
//!
//! Full TypeScript execution is not required yet; [`IpcActionRunner`] frames
//! calls through these types and either returns a stub error or talks to a
//! configured process.

use crate::error::{ActionError, ActionResult};
use crate::{ActionCall, ActionRunner};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};

/// Protocol version — bump when the JSON shape changes incompatibly.
pub const IPC_PROTOCOL_VERSION: u32 = 1;

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
    pub fn run(call: &ActionCall<'_>, user: Option<IpcUser>) -> Self {
        Self {
            v: IPC_PROTOCOL_VERSION,
            op: "run".into(),
            resource: call.resource.to_string(),
            action: call.action.to_string(),
            input: call.input.clone(),
            source_path: call.source_path.display().to_string(),
            source: Some(call.source.to_string()),
            user,
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

/// Runner that speaks the IPC protocol.
///
/// - [`IpcActionRunner::stub`]: no process — returns a framed `sidecar_not_configured` error.
/// - [`IpcActionRunner::command`]: spawn `program` per call (stdin/stdout JSONL).
#[derive(Debug, Clone)]
pub struct IpcActionRunner {
    program: Option<PathBuf>,
    pub user: Option<IpcUser>,
}

impl IpcActionRunner {
    pub fn stub() -> Self {
        Self {
            program: None,
            user: None,
        }
    }

    pub fn command(program: impl Into<PathBuf>) -> Self {
        Self {
            program: Some(program.into()),
            user: None,
        }
    }

    pub fn with_user(mut self, user: IpcUser) -> Self {
        self.user = Some(user);
        self
    }
}

impl ActionRunner for IpcActionRunner {
    fn run(&mut self, call: ActionCall<'_>) -> ActionResult<Value> {
        let req = IpcRequest::run(&call, self.user.clone());
        let line = req.encode_line()?;

        let Some(program) = &self.program else {
            // Protocol seam exercised; sidecar binary not required for Step 8 stub.
            let resp = IpcResponse::err(
                "sidecar_not_configured",
                format!(
                    "IPC protocol v{IPC_PROTOCOL_VERSION} ready; no sidecar for `{}.{}` ({})",
                    call.resource,
                    call.action,
                    call.source_path.display()
                ),
            );
            let _framed = resp.encode_line()?;
            return Err(ActionError::NotExecutable {
                resource: call.resource.to_string(),
                action: call.action.to_string(),
                path: call.source_path.display().to_string(),
            });
        };

        let mut child = Command::new(program)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| ActionError::msg(format!("ipc spawn {}: {e}", program.display())))?;

        {
            let mut stdin = child
                .stdin
                .take()
                .ok_or_else(|| ActionError::msg("ipc missing stdin"))?;
            stdin
                .write_all(line.as_bytes())
                .map_err(|e| ActionError::msg(format!("ipc write: {e}")))?;
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
        let status = child
            .wait()
            .map_err(|e| ActionError::msg(format!("ipc wait: {e}")))?;
        if !status.success() {
            return Err(ActionError::msg(format!(
                "ipc sidecar exited with {status}"
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
        };
        let req = IpcRequest::run(&call, None);
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
