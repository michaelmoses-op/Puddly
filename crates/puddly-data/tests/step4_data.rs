use puddly_check::check;
use puddly_data::{
    compile_app_statements, exec_prepared, query_prepared, OrderDir, PreparedKind, QueryBuilder,
};
use puddly_db::{RecordingExecutor, SqlValue};
use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

fn todo_ir() -> puddly_ir::AppIr {
    let root = workspace_root().join("examples/todo-slice");
    let result = check(&root);
    assert!(result.ok, "{}", result.text);
    result.ir.unwrap()
}

#[test]
fn compiled_statements_are_deterministic() {
    let ir = todo_ir();
    let a = compile_app_statements(&ir);
    let b = compile_app_statements(&ir);
    assert_eq!(a, b);
    let todo_find = a.get("todo", PreparedKind::FindById).unwrap();
    assert!(todo_find.sql.contains("FROM \"todo\""));
    assert_eq!(todo_find.param_names, vec!["id"]);
    insta::assert_snapshot!(
        "todo_statements",
        serde_json::to_string_pretty(
            &a.statements
                .iter()
                .filter(|s| s.resource == "todo")
                .collect::<Vec<_>>()
        )
        .unwrap()
    );
}

#[test]
fn builder_rejects_non_filterable_and_huge_pages() {
    let ir = todo_ir();
    let todo = ir.resources.iter().find(|r| r.name == "todo").unwrap();
    // title is not filterable
    let err = QueryBuilder::for_resource(todo)
        .eq("title", SqlValue::Text("x".into()))
        .unwrap_err();
    assert!(matches!(err, puddly_data::DataError::NotFilterable(_)));

    let err = QueryBuilder::for_resource(todo)
        .limit(10_000)
        .unwrap_err();
    assert!(matches!(err, puddly_data::DataError::PageTooLarge(_, _)));
}

#[test]
fn builder_and_prepared_record_sql() {
    let ir = todo_ir();
    let todo = ir.resources.iter().find(|r| r.name == "todo").unwrap();
    let page = QueryBuilder::for_resource(todo)
        .eq("done", SqlValue::Bool(false))
        .unwrap()
        .order_by("createdAt", OrderDir::Desc)
        .unwrap()
        .limit(10)
        .unwrap()
        .build()
        .unwrap();
    assert!(page.sql.contains("WHERE"));
    assert!(page.sql.contains("ORDER BY"));
    assert!(page.sql.contains("LIMIT"));

    let book = compile_app_statements(&ir);
    let find = book.get("todo", PreparedKind::FindById).unwrap();
    let mut exec = RecordingExecutor::new();
    query_prepared(&mut exec, find, &[SqlValue::Uuid("00000000-0000-0000-0000-000000000001".into())])
        .unwrap();
    exec_prepared(
        &mut exec,
        book.get("todo", PreparedKind::DeleteById).unwrap(),
        &[SqlValue::Uuid("00000000-0000-0000-0000-000000000001".into())],
    )
    .unwrap();
    assert!(exec.log.iter().any(|q| q.sql.contains("SELECT")));
    assert!(exec.log.iter().any(|q| q.sql.contains("DELETE")));

    insta::assert_snapshot!("todo_list_sql", format!("{}\n{:?}", page.sql, page.params));
}
