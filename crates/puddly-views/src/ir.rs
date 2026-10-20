//! Restricted view / island IR for the Step 11+ vertical slice.
//!
//! # Dialect (tiny)
//!
//! ```tsx
//! export const load = {
//!   todos: { list: "todo" },
//!   heading: { value: "Todos" },
//! };
//! export default function Name(props) {
//!   return (
//!     <div class="shell">
//!       <h1>{props.heading}</h1>
//!       {props.todos.items.map((item) => (
//!         <IslandTag key={item.id} title={item.title} done={item.done} />
//!       ))}
//!     </div>
//!   );
//! }
//! ```
//!
//! Allowed:
//! - Optional `export const load = { … }` (views; see [`LoadSpec`])
//! - `export default function …(props) { return <jsx>; }`
//! - HTML elements (lowercase) and island tags (PascalCase)
//! - Text nodes and `{ident}` / `{props.ident}` / `{item.field}` expressions
//! - String attributes and `{expr}` attribute values
//! - One-level `{expr.map((ident) => <jsx/>)}` → [`NodeIr::Map`] (ADR-033)
//! - Optional `data-puddly-id` (preserved; also auto-assigned for text exprs)
//! - Optional `export const live = true` (or `live: true` in `load`) for SSE (ADR-040)
//!
//! Not yet: full JS, hooks, VDOM, spreads, computed keys, nested maps.

use serde_json::Value;
use std::path::PathBuf;

/// Declarative data loading for a view (`export const load = { … }`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct LoadSpec {
    pub entries: Vec<LoadEntry>,
}

impl LoadSpec {
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// True when any entry needs resource CRUD (list/get) — requires an [`Executor`].
    pub fn needs_db(&self) -> bool {
        self.entries.iter().any(|e| e.source.needs_db())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct LoadEntry {
    /// Becomes a top-level props key.
    pub key: String,
    pub source: LoadSource,
}

#[derive(Debug, Clone, PartialEq)]
pub enum LoadSource {
    /// `list: "resource"` → `resource.list` → `{ items, limit }`.
    /// Optional `include: ["refField"]` (or a single string) batches relations (ADR-037).
    List {
        resource: String,
        include: Vec<String>,
    },
    /// `get: "resource", idFromQuery: "param"` → `resource.get` with `id` from `?param=`.
    Get {
        resource: String,
        id_from_query: String,
    },
    /// `value: <literal>` — static constant, no DB.
    Value { value: Value },
}

impl LoadSource {
    pub fn needs_db(&self) -> bool {
        matches!(self, Self::List { .. } | Self::Get { .. })
    }
}

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
    /// `{items.map((item) => <jsx/>)}` — one-level list dialect (ADR-033).
    Map {
        /// Path to the array (or list payload `{ items: [...] }`).
        items: PropPath,
        /// Arrow parameter name bound into each iteration's props scope.
        item_ident: String,
        /// Optional `key={…}` → `data-puddly-key` on emitted element/island wrapper.
        key: Option<PropPath>,
        body: Box<NodeIr>,
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
    /// Parsed `export const load` (empty for islands / views without load).
    pub load: LoadSpec,
    /// When true, SSR sets `data-puddly-live="1"` so the client opens EventSource.
    pub live: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModuleKind {
    View,
    Island,
}
