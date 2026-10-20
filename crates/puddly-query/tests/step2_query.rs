use puddly_check::check;
use puddly_manifest::build_manifest;
use puddly_query::{impact, inspect, render_json, QueryTarget};
use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

fn todo_manifest() -> puddly_manifest::Manifest {
    let root = workspace_root().join("examples/todo-slice");
    let result = check(&root);
    assert!(result.ok, "{}", result.text);
    build_manifest(&root, result.ir.as_ref().unwrap())
}

#[test]
fn inspect_resource_and_field() {
    let m = todo_manifest();
    let r = inspect(&m, &QueryTarget::parse("todo").unwrap());
    assert!(r.ok);
    assert_eq!(r.resource.as_ref().unwrap().name, "todo");

    let f = inspect(&m, &QueryTarget::parse("todo.owner").unwrap());
    assert!(f.ok);
    assert_eq!(f.field.as_ref().unwrap().kind, "ref");
    assert_eq!(f.field.as_ref().unwrap().target.as_deref(), Some("user"));

    let a = inspect(&m, &QueryTarget::parse("todo.action:archive").unwrap());
    assert!(a.ok);
    assert_eq!(a.action.as_ref().unwrap().run, "./archive.action.ts");

    insta::assert_snapshot!("inspect_todo", render_json(&r));
}

#[test]
fn impact_resource_graph() {
    let m = todo_manifest();
    let i = impact(&m, &QueryTarget::parse("user").unwrap());
    assert!(i.ok);
    // project and todo both ref user
    assert!(i.edges.iter().any(|e| e.kind == "referencedBy" && e.from == "todo"));
    assert!(i.edges.iter().any(|e| e.kind == "referencedBy" && e.from == "project"));
    insta::assert_snapshot!("impact_user", render_json(&i));

    let field = impact(&m, &QueryTarget::parse("todo.owner").unwrap());
    assert!(field.ok);
    assert!(field.edges.iter().any(|e| e.kind == "ref" && e.to == "user"));
    assert!(field.edges.iter().any(|e| e.kind == "access"));
}
