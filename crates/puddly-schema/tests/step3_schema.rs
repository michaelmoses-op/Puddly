use puddly_check::check;
use puddly_schema::{canonical_schema_json, schema_fingerprint, schema_from_app};
use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

#[test]
fn todo_slice_schema_deterministic() {
    let root = workspace_root().join("examples/todo-slice");
    let result = check(&root);
    assert!(result.ok, "{}", result.text);
    let ir = result.ir.unwrap();
    let a = schema_from_app(&ir);
    let b = schema_from_app(&ir);
    assert_eq!(schema_fingerprint(&a), schema_fingerprint(&b));
    assert_eq!(canonical_schema_json(&a), canonical_schema_json(&b));
    // computed fields omitted
    let todo = a.tables.iter().find(|t| t.name == "todo").unwrap();
    assert!(!todo.columns.iter().any(|c| c.name == "isOverdue"));
    assert!(todo.foreign_keys.iter().any(|f| f.references_table == "user"));
    insta::assert_snapshot!("todo_slice_schema", serde_json::to_string_pretty(&a).unwrap());
}
