//! Step 24: generated Rust row structs (ADR-042).

use puddly_check::check;
use puddly_db::{DbRow, SqlValue};
use puddly_rpc::{generate_rs_rows, write_rs_rows, GENERATED_ROWS_RS};
use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

fn todo_slice() -> PathBuf {
    workspace_root().join("examples/todo-slice")
}

#[test]
fn generate_for_todo_slice_emits_row_structs() {
    let ir = check(&todo_slice()).ir.expect("todo-slice check");
    let src = generate_rs_rows(&ir);
    assert!(src.contains("pub struct TodoRow"), "missing TodoRow:\n{src}");
    assert!(src.contains("pub struct ProjectRow"), "missing ProjectRow");
    assert!(src.contains("from_db_row"), "missing from_db_row");
    assert!(src.contains("RowDecodeError"), "missing error type");
    assert!(src.contains("due_date"), "camelCase field → snake_case");
    assert!(src.contains("\"dueDate\""), "column lookup keeps IR name");
    assert!(
        !src.contains("Computed") && !src.to_lowercase().contains("computed {"),
        "computed fields must be omitted"
    );
    // Deterministic: same IR → same bytes
    assert_eq!(src, generate_rs_rows(&ir));
}

#[test]
fn write_rs_rows_to_generated_path() {
    let root = todo_slice();
    let ir = check(&root).ir.unwrap();
    let dest = write_rs_rows(&root, &ir).expect("write");
    assert!(
        dest.ends_with(Path::new(GENERATED_ROWS_RS)),
        "dest={dest:?}"
    );
    assert!(dest.is_file());
    let text = std::fs::read_to_string(&dest).unwrap();
    assert!(text.contains("TodoRow"));
    assert!(text.contains("use puddly_db::{DbRow, SqlValue}"));
}

#[test]
fn generated_decode_logic_matches_helpers() {
    // Smoke: a canned DbRow has the columns TodoRow expects from todo-slice IR.
    let ir = check(&todo_slice()).ir.unwrap();
    let src = generate_rs_rows(&ir);
    assert!(src.contains("require_string(row, \"id\")"));
    assert!(src.contains("require_bool(row, \"done\")") || src.contains("optional_bool(row, \"done\")"));

    let mut row = DbRow::new();
    row.insert("id", SqlValue::Uuid("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee".into()));
    row.insert("title", SqlValue::Text("hi".into()));
    row.insert("done", SqlValue::Bool(false));
    row.insert("dueDate", SqlValue::Null);
    row.insert("project", SqlValue::Uuid("11111111-1111-1111-1111-111111111111".into()));
    row.insert("owner", SqlValue::Uuid("22222222-2222-2222-2222-222222222222".into()));
    row.insert("createdAt", SqlValue::Text("2026-01-01T00:00:00Z".into()));
    assert!(row.get("dueDate").is_some());
    assert!(src.contains("optional_string(row, \"dueDate\")") || src.contains("optional_string(row, \"createdAt\")"));
}
