//! Step 15: list dialect — `.map((item) => jsx)` → NodeIr::Map + SSR.

use puddly_views::{demo_todo_props, render_page, NodeIr, ViewBundle};
use serde_json::json;
use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

fn example_root() -> PathBuf {
    workspace_root().join("examples/todo-slice")
}

fn find_map(node: &NodeIr) -> Option<&NodeIr> {
    match node {
        NodeIr::Map { .. } => Some(node),
        NodeIr::Element { children, .. } => children.iter().find_map(find_map),
        _ => None,
    }
}

#[test]
fn parses_todos_items_map_into_ir() {
    let bundle = ViewBundle::load(&example_root()).expect("load views");
    let todo = bundle.views.get("todo").expect("todo view");
    let map = find_map(&todo.root).expect("expected NodeIr::Map in todo view");
    match map {
        NodeIr::Map {
            items,
            item_ident,
            key,
            body,
        } => {
            assert_eq!(items.segments, vec!["todos", "items"]);
            assert_eq!(item_ident, "item");
            assert_eq!(
                key.as_ref().map(|k| k.segments.clone()),
                Some(vec!["item".into(), "id".into()])
            );
            match body.as_ref() {
                NodeIr::Island { name, props } => {
                    assert_eq!(name, "TodoRow");
                    assert!(!props.iter().any(|a| a.name == "key"));
                    assert!(props.iter().any(|a| a.name == "id"));
                    assert!(props.iter().any(|a| a.name == "title"));
                    assert!(props.iter().any(|a| a.name == "done"));
                    assert!(props.iter().any(|a| a.name == "projectTitle"));
                }
                other => panic!("expected island body, got {other:?}"),
            }
        }
        _ => unreachable!(),
    }
}

#[test]
fn ssr_two_items_produces_two_islands() {
    let bundle = ViewBundle::load(&example_root()).unwrap();
    let props = json!({
        "heading": "Todos",
        "todos": {
            "items": [
                { "id": "a", "title": "First", "done": false },
                { "id": "b", "title": "Second", "done": true }
            ],
            "limit": 50
        }
    });
    let html = render_page(&bundle, "todo", &props).expect("ssr");
    assert!(html.contains("Todos"), "{html}");
    assert!(html.contains("First"), "{html}");
    assert!(html.contains("Second"), "{html}");
    let islands = html.matches("data-puddly-island=\"TodoRow\"").count();
    assert_eq!(islands, 2, "expected 2 islands:\n{html}");
    assert!(html.contains("data-puddly-key=\"a\""), "{html}");
    assert!(html.contains("data-puddly-key=\"b\""), "{html}");
}

#[test]
fn ssr_empty_items_renders_heading_only() {
    let bundle = ViewBundle::load(&example_root()).unwrap();
    let props = json!({
        "heading": "Todos",
        "todos": { "items": [], "limit": 50 }
    });
    let html = render_page(&bundle, "todo", &props).expect("ssr");
    assert!(html.contains("Todos"), "{html}");
    assert!(
        !html.contains("data-puddly-island=\"TodoRow\""),
        "empty map should emit no islands:\n{html}"
    );
}

#[test]
fn demo_props_still_ssr_one_island() {
    let bundle = ViewBundle::load(&example_root()).unwrap();
    let html = render_page(&bundle, "todo", &demo_todo_props()).unwrap();
    assert!(html.contains("Buy milk"), "{html}");
    assert_eq!(html.matches("data-puddly-island=\"TodoRow\"").count(), 1);
}

#[test]
fn bare_array_at_items_path_works() {
    let bundle = ViewBundle::load(&example_root()).unwrap();
    // Path is todos.items; value is a bare array (not { items: [...] }).
    let props = json!({
        "heading": "Todos",
        "todos": {
            "items": [
                { "id": "x", "title": "Bare", "done": false }
            ]
        }
    });
    let html = render_page(&bundle, "todo", &props).unwrap();
    assert!(html.contains("Bare"), "{html}");
    assert!(html.contains("data-puddly-key=\"x\""), "{html}");
}
