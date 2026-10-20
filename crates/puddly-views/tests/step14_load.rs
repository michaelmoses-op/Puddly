//! Step 14: parse `export const load` from views into LoadSpec IR.

use puddly_views::{LoadSource, ViewBundle};
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
fn parses_load_from_todo_view() {
    let bundle = ViewBundle::load(&example_root()).expect("load views");
    let todo = bundle.views.get("todo").expect("todo view");
    assert!(!todo.load.is_empty());
    assert!(todo.load.needs_db());

    let mut keys: Vec<_> = todo.load.entries.iter().map(|e| e.key.as_str()).collect();
    keys.sort();
    assert_eq!(keys, vec!["heading", "todos"]);

    let todos = todo
        .load
        .entries
        .iter()
        .find(|e| e.key == "todos")
        .unwrap();
    match &todos.source {
        LoadSource::List { resource } => assert_eq!(resource, "todo"),
        other => panic!("expected list, got {other:?}"),
    }

    let heading = todo
        .load
        .entries
        .iter()
        .find(|e| e.key == "heading")
        .unwrap();
    match &heading.source {
        LoadSource::Value { value } => assert_eq!(value.as_str(), Some("Todos")),
        other => panic!("expected value, got {other:?}"),
    }
}

#[test]
fn island_has_empty_load() {
    let bundle = ViewBundle::load(&example_root()).unwrap();
    let island = bundle.islands.get("TodoRow").unwrap();
    assert!(island.load.is_empty());
}
