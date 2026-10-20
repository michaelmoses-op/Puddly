//! Oxc-backed parser for `*.resource.ts` → unvalidated [`ParsedResource`].

mod lower;

use puddly_diagnostics::{Diagnostic, ErrorCode, Report};
use puddly_ir::{SourceFile, Span};
use oxc_allocator::Allocator;
use oxc_parser::{Parser, ParserReturn};
use oxc_span::SourceType;
use std::path::{Path, PathBuf};

pub use lower::ParsedResource;

/// Parse one resource file. Never panics on user input.
pub fn parse_resource_file(path: &Path, source: &str) -> (Option<ParsedResource>, Report) {
    let mut report = Report::new();
    let path_buf = path.to_path_buf();

    let source_type = SourceType::from_path(path).unwrap_or_else(|_| {
        SourceType::default().with_typescript(true).with_module(true)
    });

    let allocator = Allocator::default();
    let ParserReturn {
        program,
        diagnostics,
        ..
    } = Parser::new(&allocator, source, source_type).parse();

    let oxc_errors: Vec<_> = diagnostics.errors().cloned().collect();
    if !oxc_errors.is_empty() {
        for err in oxc_errors {
            let span = oxc_diag_span(&err);
            let msg = err.to_string();
            report.push(
                Diagnostic::error(ErrorCode::E0001, path_buf.clone(), span, msg)
                    .with_label(span, "syntax error"),
            );
        }
        report.finalize();
        return (None, report);
    }

    match lower::lower_program(&program, path, source) {
        Ok(parsed) => {
            report.finalize();
            (Some(parsed), report)
        }
        Err(diags) => {
            report.extend(diags);
            report.finalize();
            (None, report)
        }
    }
}

fn oxc_diag_span(err: &oxc_diagnostics::OxcDiagnostic) -> Span {
    if let Some(label) = err.labels.first() {
        let s = label.span();
        return Span::new(s.start, s.end);
    }
    Span::dummy()
}

/// Read and parse a file from disk.
pub fn parse_resource_path(path: &Path) -> (Option<ParsedResource>, Report, Option<SourceFile>) {
    match std::fs::read_to_string(path) {
        Ok(source) => {
            let file = SourceFile::new(path.to_path_buf(), source.clone());
            let (parsed, report) = parse_resource_file(path, &source);
            (parsed, report, Some(file))
        }
        Err(e) => {
            let mut report = Report::new();
            report.push(Diagnostic::error(
                ErrorCode::E0900,
                path.to_path_buf(),
                Span::dummy(),
                format!("failed to read {}: {e}", path.display()),
            ));
            report.finalize();
            (None, report, None)
        }
    }
}

/// Discover `*.resource.ts` files under `root`, sorted by path for determinism.
pub fn discover_resource_files(root: &Path) -> Result<Vec<PathBuf>, std::io::Error> {
    let mut files = Vec::new();
    visit(root, &mut files)?;
    files.sort();
    Ok(files)
}

fn visit(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), std::io::Error> {
    if !dir.is_dir() {
        return Ok(());
    }
    let mut entries: Vec<_> = std::fs::read_dir(dir)?.filter_map(|e| e.ok()).collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
            if matches!(name, "node_modules" | "target" | ".git" | ".puddly") {
                continue;
            }
            visit(&path, out)?;
        } else if path
            .file_name()
            .and_then(|s| s.to_str())
            .is_some_and(|n| n.ends_with(".resource.ts"))
        {
            out.push(path);
        }
    }
    Ok(())
}
