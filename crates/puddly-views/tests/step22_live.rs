//! Step 22: parse `export const live = true` and SSR `data-puddly-live`.

use puddly_views::{demo_todo_props, inject_live_attr, render_page, ViewBundle};
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
fn todo_view_parses_live_true() {
    let bundle = ViewBundle::load(&example_root()).expect("load views");
    let todo = bundle.views.get("todo").expect("todo view");
    assert!(todo.live, "todo.view.tsx should export live = true");
}

#[test]
fn auth_views_default_live_false() {
    let bundle = ViewBundle::load(&example_root()).unwrap();
    let login = bundle.views.get("login").expect("login");
    assert!(!login.live);
}

#[test]
fn render_page_injects_data_puddly_live() {
    let bundle = ViewBundle::load(&example_root()).unwrap();
    let html = render_page(&bundle, "todo", &demo_todo_props()).unwrap();
    assert!(
        html.contains("data-puddly-live=\"1\""),
        "SSR must mark live pages: {html}"
    );
    assert!(html.contains("puddly-islands.js"));
}

#[test]
fn inject_live_attr_on_html() {
    let out = inject_live_attr("<html><body>hi</body></html>");
    assert!(out.starts_with("<html data-puddly-live=\"1\">"));
}
