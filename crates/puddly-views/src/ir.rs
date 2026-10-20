//! Restricted view / island IR for the Step 11 vertical slice.
//!
//! # Dialect (tiny)
//!
//! ```tsx
//! export default function Name(props) {
//!   return (
//!     <div class="shell">
//!       <h1>{props.heading}</h1>
//!       <IslandTag title={props.title} done={props.done} />
//!     </div>
//!   );
//! }
//! ```
//!
//! Allowed:
//! - `export default function …(props) { return <jsx>; }`
//! - HTML elements (lowercase) and island tags (PascalCase)
//! - Text nodes and `{ident}` / `{props.ident}` expressions
//! - String attributes and `{expr}` attribute values
//! - Optional `data-puddly-id` (preserved; also auto-assigned for text exprs)
//!
//! Not yet: full JS, hooks, VDOM, spreads, computed keys, nested maps, `live: true`.

use serde_json::Value;
use std::path::PathBuf;

/// Path expression bound to render-time JSON props (`props.title` → `["title"]`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PropPath {
    pub segments: Vec<String>,
}

impl PropPath {
    pub fn single(name: impl Into<String>) -> Self {
        Self {
            segments: vec![name.into()],
        }
    }

    pub fn resolve<'a>(&self, props: &'a Value) -> Option<&'a Value> {
        let mut cur = props;
        for seg in &self.segments {
            cur = cur.get(seg)?;
        }
        Some(cur)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttrValue {
    String(String),
    Expr(PropPath),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attr {
    pub name: String,
    pub value: AttrValue,
}

#[derive(Debug, Clone, PartialEq)]
pub enum NodeIr {
    Element {
        tag: String,
        attrs: Vec<Attr>,
        children: Vec<NodeIr>,
        self_closing: bool,
    },
    /// PascalCase component → island placeholder at SSR time.
    Island {
        name: String,
        props: Vec<Attr>,
    },
    Text(String),
    Expr {
        path: PropPath,
        /// Stable id for fine-grained client patches (`data-puddly-id`).
        puddly_id: String,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct CompiledModule {
    /// Logical name: view stem (`todo`) or island component (`TodoRow`).
    pub name: String,
    pub path: PathBuf,
    pub kind: ModuleKind,
    /// Root JSX returned by the default export.
    pub root: NodeIr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModuleKind {
    View,
    Island,
}
