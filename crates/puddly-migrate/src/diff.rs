use puddly_schema::{ColumnIr, SchemaIr, TableIr};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaDiff {
    pub create_tables: Vec<TableIr>,
    pub add_columns: Vec<(String, ColumnIr)>,
    pub rename_columns: Vec<(String, String, String)>, // table, from, to
    pub create_indexes: Vec<(String, puddly_schema::IndexIr)>, // table, index
    pub add_foreign_keys: Vec<(String, puddly_schema::ForeignKeyIr)>,
    pub destructive: Vec<DestructiveChange>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DestructiveChange {
    pub kind: String,
    pub target: String,
}

impl SchemaDiff {
    pub fn is_empty(&self) -> bool {
        self.create_tables.is_empty()
            && self.add_columns.is_empty()
            && self.rename_columns.is_empty()
            && self.create_indexes.is_empty()
            && self.add_foreign_keys.is_empty()
            && self.destructive.is_empty()
    }
}

/// Structural diff from `before` → `after` (additive + rename + destructive list).
pub fn diff_schemas(before: &SchemaIr, after: &SchemaIr) -> SchemaDiff {
    let mut diff = SchemaDiff {
        create_tables: Vec::new(),
        add_columns: Vec::new(),
        rename_columns: Vec::new(),
        create_indexes: Vec::new(),
        add_foreign_keys: Vec::new(),
        destructive: Vec::new(),
    };

    let before_tables: std::collections::BTreeMap<&str, &TableIr> =
        before.tables.iter().map(|t| (t.name.as_str(), t)).collect();
    let after_tables: std::collections::BTreeMap<&str, &TableIr> =
        after.tables.iter().map(|t| (t.name.as_str(), t)).collect();

    for (name, table) in &after_tables {
        match before_tables.get(name) {
            None => diff.create_tables.push((*table).clone()),
            Some(old) => diff_table(old, table, &mut diff),
        }
    }

    for name in before_tables.keys() {
        if !after_tables.contains_key(name) {
            diff.destructive.push(DestructiveChange {
                kind: "drop_table".into(),
                target: (*name).to_string(),
            });
        }
    }

    diff.create_tables.sort_by(|a, b| a.name.cmp(&b.name));
    diff.add_columns.sort_by(|a, b| (&a.0, &a.1.name).cmp(&(&b.0, &b.1.name)));
    diff.rename_columns
        .sort_by(|a, b| (&a.0, &a.1, &a.2).cmp(&(&b.0, &b.1, &b.2)));
    diff.create_indexes
        .sort_by(|a, b| (&a.0, &a.1.name).cmp(&(&b.0, &b.1.name)));
    diff.add_foreign_keys
        .sort_by(|a, b| (&a.0, &a.1.column).cmp(&(&b.0, &b.1.column)));
    diff.destructive
        .sort_by(|a, b| (&a.kind, &a.target).cmp(&(&b.kind, &b.target)));

    diff
}

fn diff_table(before: &TableIr, after: &TableIr, diff: &mut SchemaDiff) {
    let before_cols: std::collections::BTreeMap<&str, &ColumnIr> =
        before.columns.iter().map(|c| (c.name.as_str(), c)).collect();
    let after_cols: std::collections::BTreeMap<&str, &ColumnIr> =
        after.columns.iter().map(|c| (c.name.as_str(), c)).collect();

    for (name, col) in &after_cols {
        if before_cols.contains_key(name) {
            continue;
        }
        if let Some(from) = &col.renamed_from {
            if before_cols.contains_key(from.as_str()) {
                diff.rename_columns
                    .push((after.name.clone(), from.clone(), col.name.clone()));
                continue;
            }
        }
        diff.add_columns.push((after.name.clone(), (*col).clone()));
    }

    for name in before_cols.keys() {
        if after_cols.contains_key(name) {
            continue;
        }
        // Skipped if renamed_from targets this name
        let renamed = after.columns.iter().any(|c| c.renamed_from.as_deref() == Some(*name));
        if !renamed {
            diff.destructive.push(DestructiveChange {
                kind: "drop_column".into(),
                target: format!("{}.{}", after.name, name),
            });
        }
    }

    let before_idx: std::collections::BTreeSet<&str> =
        before.indexes.iter().map(|i| i.name.as_str()).collect();
    for idx in &after.indexes {
        if !before_idx.contains(idx.name.as_str()) {
            diff.create_indexes.push((after.name.clone(), idx.clone()));
        }
    }

    let before_fk: std::collections::BTreeSet<&str> =
        before.foreign_keys.iter().map(|f| f.column.as_str()).collect();
    for fk in &after.foreign_keys {
        if !before_fk.contains(fk.column.as_str()) {
            diff.add_foreign_keys.push((after.name.clone(), fk.clone()));
        }
    }
}
