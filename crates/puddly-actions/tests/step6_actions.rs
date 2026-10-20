use puddly_actions::catalog_from_app;
use puddly_check::check;
use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

#[test]
fn todo_slice_loads_archive_action() {
    let root = workspace_root().join("examples/todo-slice");
    let ir = check(&root).ir.expect("ir");
    let catalog = catalog_from_app(&root, &ir).unwrap();
    assert_eq!(catalog.len(), 1);
    let a = catalog.get("todo", "archive").unwrap();
    assert!(a.source.contains("run"));
}
