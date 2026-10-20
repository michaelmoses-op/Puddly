//! Puddly views / islands vertical slice (ADR-007, ADR-029).
//!
//! Discovers `*.view.tsx` + `*.island.tsx`, Oxc-parses a restricted TSX dialect,
//! and SSR-renders HTML with island placeholders. Client JS is limited to
//! [`ssr::ISLANDS_RUNTIME_JS`] (no React / VDOM).
//!
//! See module docs on [`ir`] for the accepted dialect.

mod discover;
mod error;
mod ir;
mod lower;
mod parse;
mod ssr;

pub use discover::{
    discover_island_files, discover_view_files, island_name_from_path, view_name_from_path,
};
pub use error::{ViewError, ViewResult};
pub use ir::{
    Attr, AttrValue, CompiledModule, ModuleKind, NodeIr, PropPath,
};
pub use ssr::{
    demo_todo_props, render_page, render_view_chunks, render_view_html, wrap_document,
    RenderChunks, ISLANDS_RUNTIME_JS,
};

use parse::parse_module_path;
use std::collections::BTreeMap;
use std::path::Path;

/// Loaded views + islands for an app root.
#[derive(Debug, Clone, Default)]
pub struct ViewBundle {
    pub views: BTreeMap<String, CompiledModule>,
    pub islands: BTreeMap<String, CompiledModule>,
}

impl ViewBundle {
    pub fn load(root: &Path) -> ViewResult<Self> {
        let mut views = BTreeMap::new();
        for path in discover_view_files(root)? {
            let compiled = parse_module_path(&path, ModuleKind::View)?;
            views.insert(compiled.name.clone(), compiled);
        }
        let mut islands = BTreeMap::new();
        for path in discover_island_files(root)? {
            let compiled = parse_module_path(&path, ModuleKind::Island)?;
            islands.insert(compiled.name.clone(), compiled);
        }
        Ok(Self { views, islands })
    }

    pub fn default_view_name(&self) -> Option<&str> {
        if self.views.contains_key("todo") {
            Some("todo")
        } else {
            self.views.keys().next().map(|s| s.as_str())
        }
    }
}
