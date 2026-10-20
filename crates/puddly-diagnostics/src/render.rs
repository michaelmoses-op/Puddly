use crate::codes::ErrorCode;
use crate::report::{Diagnostic, DiagnosticJson, LabelJson, Report, ReportJson, SuggestionJson};
use puddly_core::CLI_NAME;
use puddly_ir::SourceFile;
use std::collections::BTreeMap;
use std::path::Path;

pub fn render_diagnostic(d: &Diagnostic, sources: &BTreeMap<String, &SourceFile>) -> String {
    let path_key = d.path.to_string_lossy().to_string();
    let source = sources.get(&path_key).copied();
    let (line, col) = source
        .map(|s| s.line_col(d.span.start))
        .unwrap_or((1, 1));
    let line_text = source.map(|s| s.line_text(line)).unwrap_or("");
    let display_path = display_path(&d.path);

    let mut out = String::new();
    out.push_str(&format!(
        "error[{code}]: {msg}\n",
        code = d.code,
        msg = d.message
    ));
    out.push_str(&format!("  --> {display_path}:{line}:{col}\n"));
    out.push_str("   |\n");
    out.push_str(&format!("{line:>3} | {line_text}\n"));

    let caret = caret_line(source, d.span.start, d.span.end, line_text);
    let label = d
        .labels
        .first()
        .map(|l| l.message.as_str())
        .unwrap_or("");
    if label.is_empty() {
        out.push_str(&format!("   | {caret}\n"));
    } else {
        out.push_str(&format!("   | {caret} {label}\n"));
    }
    out.push_str("   |\n");

    if let Some(sug) = &d.suggestion {
        out.push_str(&format!("help: {}\n", sug.message));
        if let Some(patch) = &sug.patch {
            out.push_str(&format!("      {patch}\n"));
        }
    }
    out.push_str(&format!("docs: {CLI_NAME} explain {}\n", d.code));
    out
}

fn caret_line(
    source: Option<&SourceFile>,
    start: u32,
    end: u32,
    line_text: &str,
) -> String {
    let Some(source) = source else {
        return "^".to_string();
    };
    let (line, col) = source.line_col(start);
    let line_start_offset = {
        let mut offset = 0usize;
        for (i, l) in source.source.split_inclusive('\n').enumerate() {
            if (i as u32) + 1 == line {
                break;
            }
            offset += l.len();
        }
        offset
    };
    let col0 = (col as usize).saturating_sub(1);
    let span_len = (end as usize)
        .saturating_sub(start as usize)
        .max(1)
        .min(line_text.len().saturating_sub(col0).max(1));
    // Account for leading spaces matching the line prefix "   | " is handled by caller;
    // caret aligns under line_text which starts after "| ".
    let _ = line_start_offset;
    format!("{}{}", " ".repeat(col0), "^".repeat(span_len))
}

fn display_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

pub fn render_report(report: &Report, sources: &[SourceFile]) -> String {
    let map: BTreeMap<String, &SourceFile> = sources
        .iter()
        .map(|s| (s.path.to_string_lossy().to_string(), s))
        .collect();
    let mut parts: Vec<String> = report
        .primary()
        .map(|d| render_diagnostic(d, &map))
        .collect();
    if report.error_count() > 0 {
        parts.push(format!(
            "{} error{} emitted",
            report.error_count(),
            if report.error_count() == 1 { "" } else { "s" }
        ));
    }
    parts.join("\n")
}

pub fn report_to_json(report: &Report, sources: &[SourceFile]) -> ReportJson {
    let map: BTreeMap<String, &SourceFile> = sources
        .iter()
        .map(|s| (s.path.to_string_lossy().to_string(), s))
        .collect();
    let diagnostics = report
        .diagnostics
        .iter()
        .filter(|d| !d.is_follow_on)
        .map(|d| diagnostic_to_json(d, &map))
        .collect::<Vec<_>>();
    ReportJson {
        ok: !report.has_errors(),
        error_count: report.error_count(),
        diagnostics,
    }
}

fn diagnostic_to_json(
    d: &Diagnostic,
    sources: &BTreeMap<String, &SourceFile>,
) -> DiagnosticJson {
    let path_key = d.path.to_string_lossy().to_string();
    let (line, column) = sources
        .get(&path_key)
        .map(|s| s.line_col(d.span.start))
        .unwrap_or((1, 1));
    DiagnosticJson {
        code: d.code.as_str().to_string(),
        severity: d.severity,
        message: d.message.clone(),
        path: display_path(&d.path),
        start: d.span.start,
        end: d.span.end,
        line,
        column,
        labels: d
            .labels
            .iter()
            .map(|l| LabelJson {
                start: l.span.start,
                end: l.span.end,
                message: l.message.clone(),
            })
            .collect(),
        suggestion: d.suggestion.as_ref().map(|s| SuggestionJson {
            message: s.message.clone(),
            patch: s.patch.clone(),
        }),
        root_id: d.root_id.clone(),
        follow_on: d.is_follow_on,
    }
}

pub fn render_json(report: &Report, sources: &[SourceFile]) -> String {
    let value = report_to_json(report, sources);
    // Pretty + trailing newline; serde_json map order follows struct field order (stable).
    let mut s = serde_json::to_string_pretty(&value).expect("ReportJson serializes");
    s.push('\n');
    s
}

/// Lookup helper used by the CLI.
pub fn explain_text(code: &str) -> Result<String, String> {
    let parsed = ErrorCode::parse(code).ok_or_else(|| {
        format!(
            "unknown error code `{code}`; try `{CLI_NAME} explain E0204`"
        )
    })?;
    Ok(crate::codes::format_explain(parsed))
}
