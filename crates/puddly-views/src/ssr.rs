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
            let mut open = format!("<{tag}");
            for attr in attrs {
                open.push(' ');
                open.push_str(&attr.name);
                open.push_str("=\"");
                open.push_str(&escape_attr(&attr_string(attr, props)));
                open.push('"');
            }
            if *self_closing || is_void(tag) {
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
        }
        NodeIr::Island {
            name,
            props: island_attrs,
        } => {
            let island = bundle
                .islands
                .get(name)
                .ok_or_else(|| ViewError::UnknownIsland {
                    name: name.clone(),
                })?;
            let island_props = attrs_to_json(island_attrs, props);
            let props_json = serde_json::to_string(&island_props)
                .map_err(|e| ViewError::Render(e.to_string()))?;
            let open = format!(
                "<div data-puddly-island=\"{}\" data-puddly-props=\"{}\">",
                escape_attr(name),
                escape_attr(&props_json)
            );
            out.push_back(open.into_bytes());
            emit_node(&island.root, &island_props, bundle, out)?;
            out.push_back(b"</div>".to_vec());
        }
    }
    Ok(())
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
        "title": "Buy milk",
        "done": false
    })
}

/// Temporary Step 14 bridge: islands need scalar props until the list dialect (Step 15).
///
/// Keeps `todos` (full list payload) and, when `todos.items[0]` exists, also sets
/// top-level `title` / `done` for the existing `TodoRow` island.
pub fn apply_todo_island_bridge(props: &mut Value) {
    let Some(obj) = props.as_object_mut() else {
        return;
    };
    let Some(first) = obj
        .get("todos")
        .and_then(|t| t.get("items"))
        .and_then(|items| items.as_array())
        .and_then(|arr| arr.first())
        .cloned()
    else {
        return;
    };
    if let Some(title) = first.get("title").cloned() {
        obj.insert("title".into(), title);
    }
    if let Some(done) = first.get("done").cloned() {
        obj.insert("done".into(), done);
    }
}

/// Wrap a body fragment in a minimal HTML document + island runtime script.
pub fn wrap_document(body_inner: &str) -> String {
    format!(
        "<!DOCTYPE html>\n<html><head><meta charset=\"utf-8\" /><title>Puddly</title></head><body>{body_inner}<script src=\"/puddly-islands.js\"></script></body></html>"
    )
}

/// Render a named view; if the root is not already `<html>`, wrap as a document.
pub fn render_page(bundle: &ViewBundle, view_name: &str, props: &Value) -> ViewResult<String> {
    let html = render_view_html(bundle, view_name, props)?;
    if html.trim_start().starts_with("<html") || html.trim_start().starts_with("<!DOCTYPE") {
        if html.contains("puddly-islands.js") {
            Ok(html)
        } else if html.contains("</body>") {
            Ok(html.replacen(
                "</body>",
                "<script src=\"/puddly-islands.js\"></script></body>",
                1,
            ))
        } else {
            Ok(format!(
                "{html}<script src=\"/puddly-islands.js\"></script>"
            ))
        }
    } else {
        Ok(wrap_document(&html))
    }
}
