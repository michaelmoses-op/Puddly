use crate::codes::ErrorCode;
use puddly_ir::Span;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Label {
    pub span: Span,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Suggestion {
    pub message: String,
    /// Concrete replacement patch when available.
    pub patch: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diagnostic {
    pub code: ErrorCode,
    pub severity: Severity,
    pub message: String,
    pub path: PathBuf,
    pub span: Span,
    pub labels: Vec<Label>,
    pub suggestion: Option<Suggestion>,
    /// Root-cause grouping key. Diagnostics sharing a key beyond the first are follow-ons.
    pub root_id: String,
    pub is_follow_on: bool,
}

impl Diagnostic {
    pub fn error(
        code: ErrorCode,
        path: PathBuf,
        span: Span,
        message: impl Into<String>,
    ) -> Self {
        let message = message.into();
        let root_id = format!("{code}:{}:{}-{}", path.display(), span.start, span.end);
        Self {
            code,
            severity: Severity::Error,
            message,
            path,
            span,
            labels: Vec::new(),
            suggestion: None,
            root_id,
            is_follow_on: false,
        }
    }

    pub fn with_label(mut self, span: Span, message: impl Into<String>) -> Self {
        self.labels.push(Label {
            span,
            message: message.into(),
        });
        self
    }

    pub fn with_suggestion(
        mut self,
        message: impl Into<String>,
        patch: Option<String>,
    ) -> Self {
        self.suggestion = Some(Suggestion {
            message: message.into(),
            patch,
        });
        self
    }

    pub fn with_root_id(mut self, root_id: impl Into<String>) -> Self {
        self.root_id = root_id.into();
        self
    }

    pub fn follow_on(mut self) -> Self {
        self.is_follow_on = true;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Report {
    pub diagnostics: Vec<Diagnostic>,
}

impl Report {
    pub fn new() -> Self {
        Self {
            diagnostics: Vec::new(),
        }
    }

    pub fn push(&mut self, d: Diagnostic) {
        self.diagnostics.push(d);
    }

    pub fn extend(&mut self, iter: impl IntoIterator<Item = Diagnostic>) {
        self.diagnostics.extend(iter);
    }

    pub fn has_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|d| matches!(d.severity, Severity::Error) && !d.is_follow_on)
    }

    pub fn error_count(&self) -> usize {
        self.diagnostics
            .iter()
            .filter(|d| matches!(d.severity, Severity::Error) && !d.is_follow_on)
            .count()
    }

    /// Sort deterministically and mark follow-ons after the first diagnostic per root_id.
    pub fn finalize(&mut self) {
        self.diagnostics.sort_by(|a, b| {
            (
                a.path.to_string_lossy(),
                a.span.start,
                a.span.end,
                a.code.as_str(),
                &a.message,
            )
                .cmp(&(
                    b.path.to_string_lossy(),
                    b.span.start,
                    b.span.end,
                    b.code.as_str(),
                    &b.message,
                ))
        });

        let mut seen = std::collections::BTreeSet::new();
        for d in &mut self.diagnostics {
            if !seen.insert(d.root_id.clone()) {
                d.is_follow_on = true;
            }
        }
    }

    /// Primary diagnostics only (follow-ons suppressed for human output).
    pub fn primary(&self) -> impl Iterator<Item = &Diagnostic> {
        self.diagnostics.iter().filter(|d| !d.is_follow_on)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosticJson {
    pub code: String,
    pub severity: Severity,
    pub message: String,
    pub path: String,
    pub start: u32,
    pub end: u32,
    pub line: u32,
    pub column: u32,
    pub labels: Vec<LabelJson>,
    pub suggestion: Option<SuggestionJson>,
    pub root_id: String,
    pub follow_on: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LabelJson {
    pub start: u32,
    pub end: u32,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuggestionJson {
    pub message: String,
    pub patch: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportJson {
    pub ok: bool,
    pub error_count: usize,
    pub diagnostics: Vec<DiagnosticJson>,
}
