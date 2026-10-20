//! `check` as a library function — CLI is a thin wrapper (ADR-005).

use puddly_diagnostics::{render_json, render_report, Report};
use puddly_ir::AppIr;
use puddly_validate::validate_app;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckResult {
    pub ok: bool,
    pub root: PathBuf,
    #[serde(skip)]
    pub ir: Option<AppIr>,
    #[serde(skip)]
    pub report: Report,
    /// Human-readable diagnostics (empty when `json` was requested at format time).
    pub text: String,
    /// Stable JSON document when formatted with json=true.
    pub json: String,
}

/// Check a Puddly app. Never panics on user input.
pub fn check(root: impl AsRef<Path>) -> CheckResult {
    let root = root.as_ref().to_path_buf();
    let (ir, mut report, files) = validate_app(&root);
    report.finalize();
    let text = render_report(&report, &files);
    let json = render_json(&report, &files);
    CheckResult {
        ok: ir.is_some() && !report.has_errors(),
        root,
        ir,
        report,
        text,
        json,
    }
}

/// Format a prior check result for display.
pub fn format_check(result: &CheckResult, json: bool) -> String {
    if json {
        result.json.clone()
    } else if result.ok {
        format!(
            "ok — {} resource{}\n",
            result.ir.as_ref().map(|i| i.resources.len()).unwrap_or(0),
            ""
        )
    } else {
        result.text.clone()
    }
}
