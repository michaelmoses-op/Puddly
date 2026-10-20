//! Restricted runtime query builder — closed vocabulary only (ADR-011).

use crate::error::{DataError, DataResult};
use puddly_db::SqlValue;
use puddly_ir::{FieldKind, ResourceIr};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Enforced max page size (ADR-011).
pub const DEFAULT_MAX_PAGE_SIZE: u32 = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FilterOp {
    Eq,
    Before,
    After,
    In,
    Contains,
}

impl FilterOp {
    pub fn parse(s: &str) -> DataResult<Self> {
        match s {
            "eq" => Ok(Self::Eq),
            "before" => Ok(Self::Before),
            "after" => Ok(Self::After),
            "in" => Ok(Self::In),
            "contains" => Ok(Self::Contains),
            "orderBy" | "limit" | "include" | "cursor" => Err(DataError::msg(format!(
                "`{s}` is not a filter operator; use the dedicated builder method"
            ))),
            other => Err(DataError::UnknownOperator(other.into())),
        }
    }

    fn sql_op(self) -> &'static str {
        match self {
            Self::Eq => "=",
            Self::Before => "<",
            Self::After => ">",
            Self::In => "IN",
            Self::Contains => "LIKE",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OrderDir {
    Asc,
    Desc,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Page {
    pub sql: String,
    pub params: Vec<SqlValue>,
    pub limit: u32,
}

#[derive(Debug, Clone)]
struct Filter {
    field: String,
    op: FilterOp,
    value: SqlValue,
}

#[derive(Debug, Clone)]
struct Order {
    field: String,
    dir: OrderDir,
}

/// Builder that only accepts declared filterable/sortable fields and closed ops.
#[derive(Debug, Clone)]
pub struct QueryBuilder {
    resource: String,
    table: String,
    select: Vec<String>,
    filterable: BTreeSet<String>,
    sortable: BTreeSet<String>,
    known_fields: BTreeSet<String>,
    filters: Vec<Filter>,
    order: Vec<Order>,
    limit: Option<u32>,
    cursor: Option<SqlValue>,
    max_page_size: u32,
    /// `include` names recorded for later join planning (Step 4 records only).
    includes: Vec<String>,
}

impl QueryBuilder {
    pub fn for_resource(r: &ResourceIr) -> Self {
        let mut filterable = BTreeSet::new();
        let mut sortable = BTreeSet::new();
        let mut known = BTreeSet::new();
        let mut select = Vec::new();
        for f in &r.fields {
            if matches!(f.kind, FieldKind::Computed { .. }) {
                continue;
            }
            known.insert(f.name.clone());
            select.push(f.name.clone());
            if f.options.filterable == Some(true) {
                filterable.insert(f.name.clone());
            }
            if f.options.sortable == Some(true) {
                sortable.insert(f.name.clone());
            }
        }
        // id always filterable for cursor / eq
        filterable.insert("id".into());
        sortable.insert("id".into());

        Self {
            resource: r.name.clone(),
            table: r.name.clone(),
            select,
            filterable,
            sortable,
            known_fields: known,
            filters: Vec::new(),
            order: Vec::new(),
            limit: None,
            cursor: None,
            max_page_size: DEFAULT_MAX_PAGE_SIZE,
            includes: Vec::new(),
        }
    }

    pub fn eq(mut self, field: &str, value: SqlValue) -> DataResult<Self> {
        self.push_filter(field, FilterOp::Eq, value)?;
        Ok(self)
    }

    pub fn before(mut self, field: &str, value: SqlValue) -> DataResult<Self> {
        self.push_filter(field, FilterOp::Before, value)?;
        Ok(self)
    }

    pub fn after(mut self, field: &str, value: SqlValue) -> DataResult<Self> {
        self.push_filter(field, FilterOp::After, value)?;
        Ok(self)
    }

    pub fn filter_in(mut self, field: &str, value: SqlValue) -> DataResult<Self> {
        self.push_filter(field, FilterOp::In, value)?;
        Ok(self)
    }

    pub fn contains(mut self, field: &str, value: SqlValue) -> DataResult<Self> {
        self.push_filter(field, FilterOp::Contains, value)?;
        Ok(self)
    }

    pub fn order_by(mut self, field: &str, dir: OrderDir) -> DataResult<Self> {
        if !self.known_fields.contains(field) {
            return Err(DataError::UnknownField(field.into()));
        }
        if !self.sortable.contains(field) {
            return Err(DataError::NotSortable(field.into()));
        }
        self.order.push(Order {
            field: field.into(),
            dir,
        });
        Ok(self)
    }

    pub fn limit(mut self, n: u32) -> DataResult<Self> {
        if n > self.max_page_size {
            return Err(DataError::PageTooLarge(n, self.max_page_size));
        }
        self.limit = Some(n);
        Ok(self)
    }

    /// Cursor pagination default: continue after this id (opaque cursor = id text).
    pub fn cursor(mut self, value: SqlValue) -> Self {
        self.cursor = Some(value);
        self
    }

    /// Record an include; join SQL is deferred (N+1 prevention later).
    pub fn include(mut self, relation: &str) -> DataResult<Self> {
        if !self.known_fields.contains(relation) {
            return Err(DataError::UnknownField(relation.into()));
        }
        self.includes.push(relation.into());
        Ok(self)
    }

    fn push_filter(&mut self, field: &str, op: FilterOp, value: SqlValue) -> DataResult<()> {
        if !self.known_fields.contains(field) {
            return Err(DataError::UnknownField(field.into()));
        }
        if !self.filterable.contains(field) {
            return Err(DataError::NotFilterable(field.into()));
        }
        self.filters.push(Filter {
            field: field.into(),
            op,
            value,
        });
        Ok(())
    }

    /// Build SELECT SQL + params. Cursor pagination is default when limit set.
    pub fn build(self) -> DataResult<Page> {
        let limit = self.limit.unwrap_or(DEFAULT_MAX_PAGE_SIZE).min(self.max_page_size);
        let select_list = self
            .select
            .iter()
            .map(|c| q(c))
            .collect::<Vec<_>>()
            .join(", ");
        let mut sql = format!(
            "SELECT {select_list} FROM {}",
            q(&self.table)
        );
        let mut params = Vec::new();
        let mut where_parts = Vec::new();

        for f in &self.filters {
            params.push(match f.op {
                FilterOp::Contains => match &f.value {
                    SqlValue::Text(s) => SqlValue::Text(format!("%{s}%")),
                    other => other.clone(),
                },
                _ => f.value.clone(),
            });
            let idx = params.len();
            match f.op {
                FilterOp::In => {
                    // Single-param array placeholder sketch: `= ANY($n)` with text for now.
                    where_parts.push(format!("{} = ANY(${idx})", q(&f.field)));
                }
                FilterOp::Contains => {
                    where_parts.push(format!("{} LIKE ${idx}", q(&f.field)));
                }
                other => {
                    where_parts.push(format!("{} {} ${idx}", q(&f.field), other.sql_op()));
                }
            }
        }

        if let Some(cursor) = &self.cursor {
            params.push(cursor.clone());
            let idx = params.len();
            where_parts.push(format!("{} > ${idx}", q("id")));
        }

        if !where_parts.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&where_parts.join(" AND "));
        }

        let mut order = self.order;
        if order.is_empty() {
            // Cursor pagination default: order by id asc
            order.push(Order {
                field: "id".into(),
                dir: OrderDir::Asc,
            });
        }
        sql.push_str(" ORDER BY ");
        sql.push_str(
            &order
                .iter()
                .map(|o| {
                    format!(
                        "{} {}",
                        q(&o.field),
                        match o.dir {
                            OrderDir::Asc => "ASC",
                            OrderDir::Desc => "DESC",
                        }
                    )
                })
                .collect::<Vec<_>>()
                .join(", "),
        );

        params.push(SqlValue::Int(limit as i64));
        let lim_idx = params.len();
        sql.push_str(&format!(" LIMIT ${lim_idx}"));

        // includes are intentional no-ops for SQL in Step 4 (recorded for later)
        let _ = self.includes;
        let _ = self.resource;

        Ok(Page {
            sql,
            params,
            limit,
        })
    }
}

fn q(ident: &str) -> String {
    format!("\"{}\"", ident.replace('"', "\"\""))
}
