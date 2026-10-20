//! Compile fixed prepared statements from resource IR (deterministic SQL).

use puddly_ir::{AppIr, FieldKind, ResourceIr};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PreparedKind {
    FindById,
    Insert,
    UpdateById,
    DeleteById,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreparedStatement {
    pub name: String,
    pub resource: String,
    pub kind: PreparedKind,
    pub sql: String,
    /// Ordered parameter names matching `$1..$n`.
    pub param_names: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatementBook {
    /// Sorted by (resource, name).
    pub statements: Vec<PreparedStatement>,
}

impl StatementBook {
    pub fn get(&self, resource: &str, kind: PreparedKind) -> Option<&PreparedStatement> {
        self.statements
            .iter()
            .find(|s| s.resource == resource && s.kind == kind)
    }
}

pub fn compile_app_statements(ir: &AppIr) -> StatementBook {
    let mut statements = Vec::new();
    for r in &ir.resources {
        statements.extend(compile_resource_statements(r));
    }
    statements.sort_by(|a, b| (&a.resource, &a.name).cmp(&(&b.resource, &b.name)));
    StatementBook { statements }
}

pub fn compile_resource_statements(r: &ResourceIr) -> Vec<PreparedStatement> {
    let table = q(&r.name);
    let cols: Vec<&str> = r
        .fields
        .iter()
        .filter(|f| !matches!(f.kind, FieldKind::Computed { .. }))
        .map(|f| f.name.as_str())
        .collect();

    let select_list = cols
        .iter()
        .map(|c| q(c))
        .collect::<Vec<_>>()
        .join(", ");

    let find = PreparedStatement {
        name: format!("{}.find_by_id", r.name),
        resource: r.name.clone(),
        kind: PreparedKind::FindById,
        sql: format!("SELECT {select_list} FROM {table} WHERE {} = $1", q("id")),
        param_names: vec!["id".into()],
    };

    // Include all persisted columns (including id) for explicit inserts.
    let insert_list = cols.iter().map(|c| q(c)).collect::<Vec<_>>().join(", ");
    let placeholders = (1..=cols.len())
        .map(|i| format!("${i}"))
        .collect::<Vec<_>>()
        .join(", ");
    let insert = PreparedStatement {
        name: format!("{}.insert", r.name),
        resource: r.name.clone(),
        kind: PreparedKind::Insert,
        sql: format!("INSERT INTO {table} ({insert_list}) VALUES ({placeholders})"),
        param_names: cols.iter().map(|s| (*s).to_string()).collect(),
    };

    let set_cols: Vec<&str> = cols.iter().copied().filter(|c| *c != "id").collect();
    let set_clause = set_cols
        .iter()
        .enumerate()
        .map(|(i, c)| format!("{} = ${}", q(c), i + 1))
        .collect::<Vec<_>>()
        .join(", ");
    let id_param = set_cols.len() + 1;
    let mut update_params: Vec<String> = set_cols.iter().map(|s| (*s).to_string()).collect();
    update_params.push("id".into());
    let update = PreparedStatement {
        name: format!("{}.update_by_id", r.name),
        resource: r.name.clone(),
        kind: PreparedKind::UpdateById,
        sql: format!(
            "UPDATE {table} SET {set_clause} WHERE {} = ${id_param}",
            q("id")
        ),
        param_names: update_params,
    };

    let delete = PreparedStatement {
        name: format!("{}.delete_by_id", r.name),
        resource: r.name.clone(),
        kind: PreparedKind::DeleteById,
        sql: format!("DELETE FROM {table} WHERE {} = $1", q("id")),
        param_names: vec!["id".into()],
    };

    vec![find, insert, update, delete]
}

fn q(ident: &str) -> String {
    format!("\"{}\"", ident.replace('"', "\"\""))
}
