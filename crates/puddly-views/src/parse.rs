//! Oxc-backed parse + lower for a single `*.view.tsx` / `*.island.tsx` file.

use crate::discover::{island_name_from_path, view_name_from_path};
use crate::error::{ViewError, ViewResult};
use crate::ir::{CompiledModule, LoadSpec, ModuleKind};
use crate::load::lower_load_export;
use crate::lower::lower_default_export;
use oxc_allocator::Allocator;
use oxc_parser::{Parser, ParserReturn};
use oxc_span::SourceType;
use std::path::Path;

pub fn parse_module(path: &Path, source: &str, kind: ModuleKind) -> ViewResult<CompiledModule> {
    let name = match kind {
        ModuleKind::View => view_name_from_path(path).ok_or_else(|| {
            ViewError::Msg(format!("not a *.view.tsx path: {}", path.display()))
        })?,
        ModuleKind::Island => island_name_from_path(path).ok_or_else(|| {
            ViewError::Msg(format!("not a *.island.tsx path: {}", path.display()))
        })?,
    };

    let source_type = SourceType::from_path(path).unwrap_or_else(|_| SourceType::tsx());

    let allocator = Allocator::default();
    let ParserReturn {
        program,
        diagnostics,
        ..
    } = Parser::new(&allocator, source, source_type).parse();

    let oxc_errors: Vec<_> = diagnostics.errors().cloned().collect();
    if !oxc_errors.is_empty() {
        let message = oxc_errors
            .iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>()
            .join("; ");
        return Err(ViewError::Parse {
            path: path.to_path_buf(),
            message,
        });
    }

    let load = if kind == ModuleKind::View {
        lower_load_export(&program, path)?
    } else {
        LoadSpec::default()
    };
    let lowered = lower_default_export(&program, path)?;
    Ok(CompiledModule {
        name,
        path: path.to_path_buf(),
        kind,
        root: lowered.root,
        load,
    })
}

pub fn parse_module_path(path: &Path, kind: ModuleKind) -> ViewResult<CompiledModule> {
    let source = std::fs::read_to_string(path)?;
    parse_module(path, &source, kind)
}
