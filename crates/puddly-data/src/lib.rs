//! Data access: compiled prepared statements + restricted builder (ADR-014 / ADR-011).
//!
//! - Default path: [`compile_resource_statements`] — fixed SQL for get/insert/update/delete
//! - Fallback: [`QueryBuilder`] — closed vocabulary only (`eq`, `before`, `after`, `in`,
//!   `contains`, `orderBy`, `limit`, `include`, `cursor`)

mod builder;
mod compile;
mod error;

pub use builder::{FilterOp, OrderDir, Page, QueryBuilder, DEFAULT_MAX_PAGE_SIZE};
pub use compile::{compile_app_statements, compile_resource_statements, PreparedKind, PreparedStatement, StatementBook};
pub use error::{DataError, DataResult};

use puddly_db::{DbResult, DbRow, Executor, SqlValue};
use puddly_ir::AppIr;

/// Run a prepared statement through an executor.
pub fn exec_prepared(
    exec: &mut dyn Executor,
    stmt: &PreparedStatement,
    params: &[SqlValue],
) -> DbResult<u64> {
    if params.len() != stmt.param_names.len() {
        return Err(puddly_db::DbError::msg(format!(
            "expected {} params for `{}`, got {}",
            stmt.param_names.len(),
            stmt.name,
            params.len()
        )));
    }
    exec.execute(&stmt.sql, params)
}

pub fn query_prepared(
    exec: &mut dyn Executor,
    stmt: &PreparedStatement,
    params: &[SqlValue],
) -> DbResult<Vec<DbRow>> {
    if params.len() != stmt.param_names.len() {
        return Err(puddly_db::DbError::msg(format!(
            "expected {} params for `{}`, got {}",
            stmt.param_names.len(),
            stmt.name,
            params.len()
        )));
    }
    exec.query(&stmt.sql, params)
}

/// Convenience: compile the whole app's statement book from IR.
pub fn statement_book_from_app(ir: &AppIr) -> StatementBook {
    compile_app_statements(ir)
}
