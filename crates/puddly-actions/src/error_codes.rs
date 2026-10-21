//! Frozen IPC sidecar error codes (Step 13 / ADR-031; extended ADR-046).
//!
//! Wire values are snake_case strings. The Node sidecar must emit the same
//! literals; do not invent ad-hoc codes without bumping the protocol ADR.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

/// Canonical IPC error codes exchanged in `{ error: { code, message } }`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IpcErrorCode {
    ProtocolMismatch,
    UnknownOp,
    BadRequest,
    NotFound,
    ImportFailed,
    NoRunExport,
    RunFailed,
    SidecarNotConfigured,
    /// Nested `ctx.db` auth failure (ADR-046).
    Unauthorized,
    /// Nested `ctx.db` access denial (ADR-046).
    Forbidden,
    /// Nested `ctx.db` failure not covered above (ADR-046).
    DbFailed,
}

impl IpcErrorCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ProtocolMismatch => "protocol_mismatch",
            Self::UnknownOp => "unknown_op",
            Self::BadRequest => "bad_request",
            Self::NotFound => "not_found",
            Self::ImportFailed => "import_failed",
            Self::NoRunExport => "no_run_export",
            Self::RunFailed => "run_failed",
            Self::SidecarNotConfigured => "sidecar_not_configured",
            Self::Unauthorized => "unauthorized",
            Self::Forbidden => "forbidden",
            Self::DbFailed => "db_failed",
        }
    }

    /// All frozen codes (stable order for docs / tests).
    pub const ALL: &'static [IpcErrorCode] = &[
        Self::ProtocolMismatch,
        Self::UnknownOp,
        Self::BadRequest,
        Self::NotFound,
        Self::ImportFailed,
        Self::NoRunExport,
        Self::RunFailed,
        Self::SidecarNotConfigured,
        Self::Unauthorized,
        Self::Forbidden,
        Self::DbFailed,
    ];
}

impl fmt::Display for IpcErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for IpcErrorCode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "protocol_mismatch" => Ok(Self::ProtocolMismatch),
            "unknown_op" => Ok(Self::UnknownOp),
            "bad_request" => Ok(Self::BadRequest),
            "not_found" => Ok(Self::NotFound),
            "import_failed" => Ok(Self::ImportFailed),
            "no_run_export" => Ok(Self::NoRunExport),
            "run_failed" => Ok(Self::RunFailed),
            "sidecar_not_configured" => Ok(Self::SidecarNotConfigured),
            "unauthorized" => Ok(Self::Unauthorized),
            "forbidden" => Ok(Self::Forbidden),
            "db_failed" => Ok(Self::DbFailed),
            other => Err(format!("unknown ipc error code `{other}`")),
        }
    }
}
