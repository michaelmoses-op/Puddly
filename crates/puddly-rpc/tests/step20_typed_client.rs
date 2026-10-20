//! Step 20: typed TS client generation (ADR-038).

use puddly_check::check;
use puddly_rpc::{generate_ts_client, write_ts_client, GENERATED_CLIENT_TS};
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
fn generate_for_todo_slice_contains_todo_list_and_auth_login() {
    let ir = check(&todo_slice()).ir.expect("todo-slice check");
    let src = generate_ts_client(&ir);
    assert!(
        src.contains("todo.list") || src.contains("\"todo.list\""),
        "missing todo.list:\n{src}"
    );
    assert!(
        src.contains("auth.login") || src.contains("\"auth.login\""),
        "missing auth.login:\n{src}"
    );
    assert!(src.contains("api.todo") || src.contains("todo: {"), "missing todo namespace");
    assert!(src.contains("credentials: \"include\""), "must send cookies");
    assert!(src.contains("X-Puddly-CSRF"), "must send CSRF header");
    assert!(src.contains("TodoFields"), "field type from IR");
    assert!(src.contains("archive:"), "custom action method");
}

#[test]
fn write_ts_client_to_generated_path() {
    let root = todo_slice();
    let ir = check(&root).ir.unwrap();
    let dest = write_ts_client(&root, &ir).expect("write");
    assert!(
        dest.ends_with(Path::new(GENERATED_CLIENT_TS)),
        "dest={dest:?}"
    );
    assert!(dest.is_file());
    let text = std::fs::read_to_string(&dest).unwrap();
    assert!(text.contains("\"todo.list\""));
    assert!(text.contains("\"auth.login\""));
}
