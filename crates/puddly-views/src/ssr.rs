//! Server-side render of view IR → HTML (chunked iterator).

use crate::error::{ViewError, ViewResult};
use crate::ir::{Attr, AttrValue, NodeIr, PropPath};
use crate::ViewBundle;
use serde_json::Value;
use std::collections::VecDeque;

/// Minimal in-repo island runtime (also served at `GET /puddly-islands.js`).
pub const ISLANDS_RUNTIME_JS: &str = include_str!("../../../tools/puddly-islands.js");

/// Stream SSR as UTF-8 byte chunks (open tags / text / close tags).
pub fn render_view_chunks(
    bundle: &ViewBundle,
    view_name: &str,
    props: &Value,
) -> ViewResult<RenderChunks> {
    let view = bundle
        .views
        .get(view_name)
        .ok_or_else(|| ViewError::UnknownView {
            name: view_name.to_string(),
        })?;
    let mut chunks = VecDeque::new();
    emit_node(&view.root, props, bundle, &mut chunks)?;
    Ok(RenderChunks { chunks })
}

pub fn render_view_html(bundle: &ViewBundle, view_name: &str, props: &Value) -> ViewResult<String> {
    let chunks = render_view_chunks(bundle, view_name, props)?;
    Ok(chunks
        .into_iter()
        .map(|c| String::from_utf8_lossy(&c).into_owned())
        .collect())
}

pub struct RenderChunks {
    chunks: VecDeque<Vec<u8>>,
}

impl Iterator for RenderChunks {
    type Item = Vec<u8>;

    fn next(&mut self) -> Option<Self::Item> {
        self.chunks.pop_front()
    }
}

fn emit_node(
    node: &NodeIr,
    props: &Value,
    bundle: &ViewBundle,
    out: &mut VecDeque<Vec<u8>>,
) -> ViewResult<()> {
    match node {
        NodeIr::Text(t) => {
            out.push_back(escape_text(t).into_bytes());
        }
        NodeIr::Expr { path, puddly_id: _ } => {
            // Text only — put `data-puddly-id` on a parent element for client patches.
            let text = resolve_text(path, props);
            out.push_back(escape_text(&text).into_bytes());
        }
        NodeIr::Element {
            tag,
            attrs,
            children,
            self_closing,
        } => {
            emit_element(tag, attrs, children, *self_closing, props, None, bundle, out)?;
        }
        NodeIr::Island {
            name,
            props: island_attrs,
        } => {
            emit_island(name, island_attrs, props, None, bundle, out)?;
        }
        NodeIr::Map {
            items,
            item_ident,
            key,
            body,
        } => {
            for elem in resolve_array(items, props) {
                let child_props = bind_item(props, item_ident, &elem);
                let key_val = key
                    .as_ref()
                    .map(|k| resolve_text(k, &child_props))
                    .filter(|s| !s.is_empty());
                emit_map_body(body, &child_props, key_val.as_deref(), bundle, out)?;
            }
        }
    }
    Ok(())
}

fn emit_map_body(
    body: &NodeIr,
    props: &Value,
    key: Option<&str>,
    bundle: &ViewBundle,
    out: &mut VecDeque<Vec<u8>>,
) -> ViewResult<()> {
    match body {
        NodeIr::Element {
            tag,
            attrs,
            children,
            self_closing,
        } => emit_element(tag, attrs, children, *self_closing, props, key, bundle, out),
        NodeIr::Island {
            name,
            props: island_attrs,
        } => emit_island(name, island_attrs, props, key, bundle, out),
        other => emit_node(other, props, bundle, out),
    }
}

fn emit_element(
    tag: &str,
    attrs: &[Attr],
    children: &[NodeIr],
    self_closing: bool,
    props: &Value,
    key: Option<&str>,
    bundle: &ViewBundle,
    out: &mut VecDeque<Vec<u8>>,
) -> ViewResult<()> {
    let mut open = format!("<{tag}");
    if let Some(k) = key {
        open.push_str(" data-puddly-key=\"");
        open.push_str(&escape_attr(k));
        open.push('"');
    }
    for attr in attrs {
        open.push(' ');
        open.push_str(&attr.name);
        open.push_str("=\"");
        open.push_str(&escape_attr(&attr_string(attr, props)));
        open.push('"');
    }
    if self_closing || is_void(tag) {
        open.push_str(" />");
        out.push_back(open.into_bytes());
    } else {
        open.push('>');
        out.push_back(open.into_bytes());
        for child in children {
            emit_node(child, props, bundle, out)?;
        }
        out.push_back(format!("</{tag}>").into_bytes());
    }
    Ok(())
}

fn emit_island(
    name: &str,
    island_attrs: &[Attr],
    props: &Value,
    key: Option<&str>,
    bundle: &ViewBundle,
    out: &mut VecDeque<Vec<u8>>,
) -> ViewResult<()> {
    let island = bundle
        .islands
        .get(name)
        .ok_or_else(|| ViewError::UnknownIsland {
            name: name.to_string(),
        })?;
    let island_props = attrs_to_json(island_attrs, props);
    let props_json =
        serde_json::to_string(&island_props).map_err(|e| ViewError::Render(e.to_string()))?;
    let mut open = format!(
        "<div data-puddly-island=\"{}\" data-puddly-props=\"{}\"",
        escape_attr(name),
        escape_attr(&props_json)
    );
    if let Some(k) = key {
        open.push_str(" data-puddly-key=\"");
        open.push_str(&escape_attr(k));
        open.push('"');
    }
    open.push('>');
    out.push_back(open.into_bytes());
    emit_node(&island.root, &island_props, bundle, out)?;
    out.push_back(b"</div>".to_vec());
    Ok(())
}

/// Resolve a map source: bare array, or `{ items: [...] }` list-RPC payload.
fn resolve_array(path: &PropPath, props: &Value) -> Vec<Value> {
    match path.resolve(props) {
        Some(Value::Array(a)) => a.clone(),
        Some(Value::Object(o)) => match o.get("items") {
            Some(Value::Array(a)) => a.clone(),
            _ => Vec::new(),
        },
        _ => Vec::new(),
    }
}

/// Merge parent props with `{ item_ident: element }` (child binding wins for that key).
fn bind_item(parent: &Value, item_ident: &str, element: &Value) -> Value {
    let mut map = match parent {
        Value::Object(o) => o.clone(),
        _ => serde_json::Map::new(),
    };
    map.insert(item_ident.to_string(), element.clone());
    Value::Object(map)
}

fn attrs_to_json(attrs: &[Attr], parent: &Value) -> Value {
    let mut map = serde_json::Map::new();
    for attr in attrs {
        let v = match &attr.value {
            AttrValue::String(s) => Value::String(s.clone()),
            AttrValue::Expr(path) => path.resolve(parent).cloned().unwrap_or(Value::Null),
        };
        map.insert(attr.name.clone(), v);
    }
    Value::Object(map)
}

fn attr_string(attr: &Attr, props: &Value) -> String {
    match &attr.value {
        AttrValue::String(s) => s.clone(),
        AttrValue::Expr(path) => resolve_text(path, props),
    }
}

fn resolve_text(path: &PropPath, props: &Value) -> String {
    match path.resolve(props) {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Bool(b)) => b.to_string(),
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::Null) | None => String::new(),
        Some(other) => other.to_string(),
    }
}

fn escape_text(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn escape_attr(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
}

fn is_void(tag: &str) -> bool {
    matches!(
        tag,
        "area"
            | "base"
            | "br"
            | "col"
            | "embed"
            | "hr"
            | "img"
            | "input"
            | "link"
            | "meta"
            | "param"
            | "source"
            | "track"
            | "wbr"
    )
}

/// Test-only fixture props for the todo slice (not used by the server happy path).
pub fn demo_todo_props() -> Value {
    serde_json::json!({
        "heading": "Todos",
        "todos": {
            "items": [
                { "id": "demo-1", "title": "Buy milk", "done": false }
            ],
            "limit": 50
        }
    })
}

/// Wrap a body fragment in a minimal HTML document + island runtime script.
pub fn wrap_document(body_inner: &str) -> String {
    format!(
        "<!DOCTYPE html>\n<html><head><meta charset=\"utf-8\" /><title>Puddly</title></head><body>{body_inner}<script src=\"/puddly-islands.js\"></script></body></html>"
    )
}

/// Ensure `data-puddly-live="1"` on `<html>` (or `<body>` if no html root).
pub fn inject_live_attr(html: &str) -> String {
    if html.contains("data-puddly-live=") {
        return html.to_string();
    }
    if let Some(rest) = html.strip_prefix("<html") {
        // `<html>` or `<html …>`
        if rest.starts_with('>') || rest.starts_with(' ') || rest.starts_with('\n') {
            return format!("<html data-puddly-live=\"1\"{rest}");
        }
    }
    if let Some(idx) = html.find("<html") {
        let after = idx + 5;
        return format!(
            "{}<html data-puddly-live=\"1\"{}",
            &html[..idx],
            &html[after..]
        );
    }
    if let Some(rest) = html.strip_prefix("<body") {
        if rest.starts_with('>') || rest.starts_with(' ') || rest.starts_with('\n') {
            return format!("<body data-puddly-live=\"1\"{rest}");
        }
    }
    if let Some(idx) = html.find("<body") {
        let after = idx + 5;
        return format!(
            "{}<body data-puddly-live=\"1\"{}",
            &html[..idx],
            &html[after..]
        );
    }
    html.to_string()
}

/// Render a named view; if the root is not already `<html>`, wrap as a document.
/// When the compiled view has `live: true`, injects `data-puddly-live="1"`.
pub fn render_page(bundle: &ViewBundle, view_name: &str, props: &Value) -> ViewResult<String> {
    let live = bundle
        .views
        .get(view_name)
        .map(|m| m.live)
        .unwrap_or(false);
    let html = render_view_html(bundle, view_name, props)?;
    let mut page = if html.trim_start().starts_with("<html") || html.trim_start().starts_with("<!DOCTYPE") {
        if html.contains("puddly-islands.js") {
            html
        } else if html.contains("</body>") {
            html.replacen(
                "</body>",
                "<script src=\"/puddly-islands.js\"></script></body>",
                1,
            )
        } else {
            format!("{html}<script src=\"/puddly-islands.js\"></script>")
        }
    } else {
        wrap_document(&html)
    };
    if live {
        page = inject_live_attr(&page);
    }
    Ok(page)
}
