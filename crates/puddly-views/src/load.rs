//! Parse `export const load = { … }` from a view module into [`LoadSpec`].

use crate::error::{ViewError, ViewResult};
use crate::ir::{LoadEntry, LoadSource, LoadSpec};
use oxc_ast::ast::{
    BindingPattern, Declaration, Expression, ObjectExpression, ObjectPropertyKind, Program,
    PropertyKey, Statement, VariableDeclarationKind,
};
use serde_json::Value;
use std::path::Path;

/// Result of parsing view-level exports (`load` + optional `live`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ViewFlags {
    pub load: LoadSpec,
    pub live: bool,
}

/// Extract optional `export const load` / `export const live` from a Program.
/// Missing load → empty [`LoadSpec`]. Islands typically have none.
pub fn lower_view_exports(program: &Program<'_>, path: &Path) -> ViewResult<ViewFlags> {
    let mut load: Option<(LoadSpec, bool)> = None;
    let mut live_export: Option<bool> = None;

    for stmt in &program.body {
        let Statement::ExportDeclaration(export) = stmt else {
            continue;
        };
        let Declaration::VariableDeclaration(var) = &export.declaration else {
            continue;
        };
        if var.kind != VariableDeclarationKind::Const {
            continue;
        }
        for decl in &var.declarations {
            let Some(name) = binding_name(&decl.id) else {
                continue;
            };
            match name.as_str() {
                "load" => {
                    if load.is_some() {
                        return Err(dialect(path, "duplicate `export const load`"));
                    }
                    let Some(init) = &decl.init else {
                        return Err(dialect(path, "`load` must be initialized with an object"));
                    };
                    load = Some(lower_load_object(init, path)?);
                }
                "live" => {
                    if live_export.is_some() {
                        return Err(dialect(path, "duplicate `export const live`"));
                    }
                    let Some(init) = &decl.init else {
                        return Err(dialect(path, "`live` must be initialized with a boolean"));
                    };
                    live_export = Some(bool_lit(init).ok_or_else(|| {
                        dialect(path, "`live` must be a boolean literal (`true` / `false`)")
                    })?);
                }
                _ => {}
            }
        }
    }

    let (load_spec, live_from_load) = load.unwrap_or_default();
    Ok(ViewFlags {
        load: load_spec,
        live: live_export.unwrap_or(false) || live_from_load,
    })
}

/// Extract optional `export const load = { … }` from a Program.
/// Missing load → empty [`LoadSpec`]. Islands typically have none.
#[allow(dead_code)] // public helper; Step 14 tests historically used ViewBundle.load
pub fn lower_load_export(program: &Program<'_>, path: &Path) -> ViewResult<LoadSpec> {
    Ok(lower_view_exports(program, path)?.load)
}

fn lower_load_object(expr: &Expression<'_>, path: &Path) -> ViewResult<(LoadSpec, bool)> {
    let obj = as_object(expr).ok_or_else(|| dialect(path, "`load` must be an object literal"))?;
    let mut entries = Vec::new();
    let mut live = false;
    for prop in &obj.properties {
        let ObjectPropertyKind::ObjectProperty(p) = prop else {
            return Err(dialect(path, "spread in `load` is not supported"));
        };
        let key = static_key(&p.key)
            .ok_or_else(|| dialect(path, "`load` keys must be static identifiers"))?;
        if key == "live" {
            live = bool_lit(&p.value).ok_or_else(|| {
                dialect(path, "`load.live` must be a boolean literal (`true` / `false`)")
            })?;
            continue;
        }
        let source = lower_load_entry(&p.value, path, &key)?;
        entries.push(LoadEntry { key, source });
    }
    Ok((LoadSpec { entries }, live))
}

fn bool_lit(expr: &Expression<'_>) -> Option<bool> {
    match expr {
        Expression::BooleanLiteral(b) => Some(b.value),
        Expression::ParenthesizedExpression(p) => bool_lit(&p.expression),
        _ => None,
    }
}

fn lower_load_entry(expr: &Expression<'_>, path: &Path, key: &str) -> ViewResult<LoadSource> {
    let obj = as_object(expr).ok_or_else(|| {
        dialect(
            path,
            format!("`load.{key}` must be an object (`list` / `get` / `value`)"),
        )
    })?;

    let mut list: Option<String> = None;
    let mut get: Option<String> = None;
    let mut id_from_query: Option<String> = None;
    let mut value: Option<Value> = None;
    let mut include: Vec<String> = Vec::new();
    let mut saw_include = false;

    for prop in &obj.properties {
        let ObjectPropertyKind::ObjectProperty(p) = prop else {
            return Err(dialect(
                path,
                format!("spread in `load.{key}` is not supported"),
            ));
        };
        let field = static_key(&p.key).ok_or_else(|| {
            dialect(
                path,
                format!("`load.{key}` fields must be static identifiers"),
            )
        })?;
        match field.as_str() {
            "list" => {
                list = Some(string_lit(&p.value).ok_or_else(|| {
                    dialect(path, format!("`load.{key}.list` must be a string resource name"))
                })?);
            }
            "get" => {
                get = Some(string_lit(&p.value).ok_or_else(|| {
                    dialect(path, format!("`load.{key}.get` must be a string resource name"))
                })?);
            }
            "idFromQuery" => {
                id_from_query = Some(string_lit(&p.value).ok_or_else(|| {
                    dialect(
                        path,
                        format!("`load.{key}.idFromQuery` must be a string query param name"),
                    )
                })?);
            }
            "value" => {
                value = Some(literal_value(&p.value).ok_or_else(|| {
                    dialect(
                        path,
                        format!(
                            "`load.{key}.value` must be a string/number/bool/null literal"
                        ),
                    )
                })?);
            }
            "include" => {
                saw_include = true;
                include = parse_include(&p.value, path, key)?;
            }
            other => {
                return Err(dialect(
                    path,
                    format!(
                        "unknown `load.{key}` field `{other}` (expected list|get|idFromQuery|value|include)"
                    ),
                ));
            }
        }
    }

    match (list, get, value, id_from_query) {
        (Some(resource), None, None, None) => Ok(LoadSource::List { resource, include }),
        (None, Some(resource), None, Some(id_from_query)) => {
            if saw_include {
                return Err(dialect(
                    path,
                    format!("`load.{key}.include` is only valid with `list`"),
                ));
            }
            Ok(LoadSource::Get {
                resource,
                id_from_query,
            })
        }
        (None, Some(_), None, None) => Err(dialect(
            path,
            format!("`load.{key}.get` requires `idFromQuery`"),
        )),
        (None, None, Some(value), None) => {
            if saw_include {
                return Err(dialect(
                    path,
                    format!("`load.{key}.include` is only valid with `list`"),
                ));
            }
            Ok(LoadSource::Value { value })
        }
        (None, None, None, None) => Err(dialect(
            path,
            format!("`load.{key}` needs `list`, `get`+`idFromQuery`, or `value`"),
        )),
        _ => Err(dialect(
            path,
            format!("`load.{key}` must use exactly one of list / get+idFromQuery / value"),
        )),
    }
}

fn parse_include(expr: &Expression<'_>, path: &Path, key: &str) -> ViewResult<Vec<String>> {
    if let Some(s) = string_lit(expr) {
        return Ok(vec![s]);
    }
    match expr {
        Expression::ArrayExpression(arr) => {
            let mut out = Vec::new();
            for el in &arr.elements {
                let Some(expr) = el.as_expression() else {
                    return Err(dialect(
                        path,
                        format!("`load.{key}.include` entries must be strings"),
                    ));
                };
                let s = string_lit(expr).ok_or_else(|| {
                    dialect(
                        path,
                        format!("`load.{key}.include` entries must be strings"),
                    )
                })?;
                out.push(s);
            }
            Ok(out)
        }
        Expression::ParenthesizedExpression(p) => parse_include(&p.expression, path, key),
        _ => Err(dialect(
            path,
            format!("`load.{key}.include` must be a string or string array"),
        )),
    }
}

fn binding_name(id: &BindingPattern<'_>) -> Option<String> {
    id.get_identifier_name().map(|n| n.as_str().to_string())
}

fn static_key(key: &PropertyKey<'_>) -> Option<String> {
    match key {
        PropertyKey::StaticIdentifier(id) => Some(id.name.as_str().to_string()),
        PropertyKey::StringLiteral(s) => Some(s.value.as_str().to_string()),
        _ => None,
    }
}

fn as_object<'a>(expr: &'a Expression<'a>) -> Option<&'a ObjectExpression<'a>> {
    match expr {
        Expression::ObjectExpression(o) => Some(o),
        Expression::ParenthesizedExpression(p) => as_object(&p.expression),
        _ => None,
    }
}

fn string_lit(expr: &Expression<'_>) -> Option<String> {
    match expr {
        Expression::StringLiteral(s) => Some(s.value.as_str().to_string()),
        Expression::ParenthesizedExpression(p) => string_lit(&p.expression),
        _ => None,
    }
}

fn literal_value(expr: &Expression<'_>) -> Option<Value> {
    match expr {
        Expression::StringLiteral(s) => Some(Value::String(s.value.as_str().to_string())),
        Expression::BooleanLiteral(b) => Some(Value::Bool(b.value)),
        Expression::NullLiteral(_) => Some(Value::Null),
        Expression::NumericLiteral(n) => {
            if n.value.fract() == 0.0 && n.value >= i64::MIN as f64 && n.value <= i64::MAX as f64 {
                Some(Value::Number((n.value as i64).into()))
            } else {
                serde_json::Number::from_f64(n.value).map(Value::Number)
            }
        }
        Expression::ParenthesizedExpression(p) => literal_value(&p.expression),
        _ => None,
    }
}

fn dialect(path: &Path, message: impl Into<String>) -> ViewError {
    ViewError::Dialect {
        path: path.to_path_buf(),
        message: message.into(),
    }
}
