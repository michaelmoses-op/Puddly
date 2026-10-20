use crate::diff::SchemaDiff;
use puddly_schema::{ColumnIr, ForeignKeyIr, IndexIr, SchemaIr, TableIr};

fn q(ident: &str) -> String {
    // Always quote for reserved words (e.g. "user") and camelCase columns.
    format!("\"{}\"", ident.replace('"', "\"\""))
}

/// Render a full deterministic CREATE script for the desired schema.
pub fn render_schema_sql(schema: &SchemaIr) -> String {
    let mut out = String::new();
    out.push_str("-- Full schema (postgres)\n");
    out.push_str("CREATE EXTENSION IF NOT EXISTS pgcrypto;\n\n");

    for table in &schema.tables {
        out.push_str(&render_create_table(table));
        out.push('\n');
    }

    for table in &schema.tables {
        for fk in &table.foreign_keys {
            out.push_str(&render_add_fk(&table.name, fk));
            out.push('\n');
        }
    }

    for table in &schema.tables {
        for idx in &table.indexes {
            out.push_str(&render_create_index(&table.name, idx));
            out.push('\n');
        }
    }

    out
}

pub fn render_diff_sql(diff: &SchemaDiff, allow_destructive: bool) -> String {
    let mut out = String::new();
    out.push_str("-- Forward migration (postgres)\n");
    out.push_str("CREATE EXTENSION IF NOT EXISTS pgcrypto;\n\n");

    for table in &diff.create_tables {
        out.push_str(&render_create_table(table));
        out.push('\n');
    }

    for (table, col) in &diff.add_columns {
        out.push_str(&render_add_column(table, col));
        out.push('\n');
    }

    for (table, from, to) in &diff.rename_columns {
        out.push_str(&format!(
            "ALTER TABLE {} RENAME COLUMN {} TO {};\n",
            q(table),
            q(from),
            q(to)
        ));
    }

    for (table, fk) in &diff.add_foreign_keys {
        out.push_str(&render_add_fk(table, fk));
        out.push('\n');
    }

    for (table, idx) in &diff.create_indexes {
        out.push_str(&render_create_index(table, idx));
        out.push('\n');
    }

    if allow_destructive {
        for d in &diff.destructive {
            match d.kind.as_str() {
                "drop_table" => {
                    out.push_str(&format!("DROP TABLE IF EXISTS {};\n", q(&d.target)));
                }
                "drop_column" => {
                    if let Some((table, col)) = d.target.split_once('.') {
                        out.push_str(&format!(
                            "ALTER TABLE {} DROP COLUMN IF EXISTS {};\n",
                            q(table),
                            q(col)
                        ));
                    }
                }
                _ => {}
            }
        }
    }

    let header = "-- Forward migration (postgres)\nCREATE EXTENSION IF NOT EXISTS pgcrypto;\n\n";
    if out == header {
        out.push_str("-- no structural changes\n");
    }

    out
}

fn render_create_table(table: &TableIr) -> String {
    let mut lines = Vec::new();
    lines.push(format!("CREATE TABLE {} (", q(&table.name)));
    let mut cols: Vec<String> = table
        .columns
        .iter()
        .map(|c| format!("  {}", column_def(c)))
        .collect();
    cols.push(format!("  PRIMARY KEY ({})", q(&table.primary_key)));
    lines.push(cols.join(",\n"));
    lines.push(");".into());
    lines.join("\n") + "\n"
}

fn column_def(c: &ColumnIr) -> String {
    let mut s = format!("{} {}", q(&c.name), c.sql_type);
    if c.not_null {
        s.push_str(" NOT NULL");
    }
    if let Some(def) = &c.default_sql {
        s.push_str(" DEFAULT ");
        s.push_str(def);
    }
    s
}

fn render_add_column(table: &str, c: &ColumnIr) -> String {
    format!("ALTER TABLE {} ADD COLUMN {};", q(table), column_def(c))
}

fn render_add_fk(table: &str, fk: &ForeignKeyIr) -> String {
    format!(
        "ALTER TABLE {table} ADD CONSTRAINT {constraint} FOREIGN KEY ({col}) REFERENCES {ref_table}({ref_col});",
        table = q(table),
        constraint = q(&format!("fk_{table}_{}", fk.column)),
        col = q(&fk.column),
        ref_table = q(&fk.references_table),
        ref_col = q(&fk.references_column),
    )
}

fn render_create_index(table: &str, idx: &IndexIr) -> String {
    let unique = if idx.unique { "UNIQUE " } else { "" };
    let cols = idx
        .columns
        .iter()
        .map(|c| q(c))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "CREATE {unique}INDEX {} ON {} ({});",
        q(&idx.name),
        q(table),
        cols
    )
}
