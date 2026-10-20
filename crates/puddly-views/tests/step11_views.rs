//! Step 11: discover + compile + SSR HTML for views/islands.

use puddly_views::{
    demo_todo_props, discover_island_files, discover_view_files, render_page, render_view_chunks,
    ViewBundle,
};
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

#[test]
fn discovers_todo_view_and_island() {
    let root = example_root();
    let views = discover_view_files(&root).unwrap();
    let islands = discover_island_files(&root).unwrap();
    assert!(
        views.iter().any(|p| p.ends_with("todo.view.tsx")),
        "expected todo.view.tsx, got {views:?}"
    );
    assert!(
        islands.iter().any(|p| p.ends_with("todo-row.island.tsx")),
        "expected todo-row.island.tsx, got {islands:?}"
    );
}

#[test]
fn compiles_and_ssr_contains_text_and_island_marker() {
    let bundle = ViewBundle::load(&example_root()).expect("load views");
    assert!(bundle.views.contains_key("todo"));
    assert!(bundle.islands.contains_key("TodoRow"));

    let props = demo_todo_props();
    let html = render_page(&bundle, "todo", &props).expect("ssr");

    assert!(html.contains("Buy milk"), "SSR missing title text:\n{html}");
    assert!(html.contains("Todos"), "SSR missing heading:\n{html}");
    assert!(
        html.contains("data-puddly-island=\"TodoRow\""),
        "missing island marker:\n{html}"
    );
    assert!(
        html.contains("data-puddly-props"),
        "missing serialized props:\n{html}"
    );
    assert!(
        html.contains("data-puddly-id=\"title\""),
        "missing puddly id:\n{html}"
    );
    assert!(
        html.contains("/puddly-islands.js"),
        "missing runtime script:\n{html}"
    );
    assert!(html.contains("false") || html.contains("done"), "missing done:\n{html}");
}

#[test]
fn ssr_streams_chunks() {
    let bundle = ViewBundle::load(&example_root()).unwrap();
    let chunks: Vec<_> = render_view_chunks(&bundle, "todo", &demo_todo_props())
        .unwrap()
        .collect();
    assert!(chunks.len() > 1, "expected chunked emit, got {}", chunks.len());
    let joined = chunks
        .iter()
        .map(|c| String::from_utf8_lossy(c))
        .collect::<String>();
    assert!(joined.contains("Buy milk"));
}

#[test]
fn islands_runtime_js_is_nonempty() {
    assert!(puddly_views::ISLANDS_RUNTIME_JS.contains("data-puddly-island"));
    assert!(puddly_views::ISLANDS_RUNTIME_JS.contains("update"));
}
