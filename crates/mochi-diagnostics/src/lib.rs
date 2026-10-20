//! Diagnostic codes, rendering, grouping, and `mochi explain` registry.

mod codes;
mod render;
mod report;

pub use codes::{explain, format_explain, ErrorCode, ERROR_REGISTRY};
pub use render::{explain_text, render_diagnostic, render_json, render_report, report_to_json};
pub use report::{
    Diagnostic, DiagnosticJson, Label, Report, ReportJson, Severity, Suggestion,
};
