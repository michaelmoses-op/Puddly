//! Lower an Oxc `Program` into unvalidated resource IR.

use puddly_diagnostics::{Diagnostic, ErrorCode};
use puddly_ir::{
    AccessIr, AccessOpIr, ActionIr, ConstraintIr, FieldIr, FieldKind, FieldOptions, LiteralValue,
    RuleIr, Span,
};
use oxc_ast::ast::{
    Argument, BindingPattern, CallExpression, Declaration, Expression, ObjectExpression,
    ObjectPropertyKind, Program, PropertyKey, Statement, VariableDeclarationKind,
};
use std::path::{Path, PathBuf};

/// Resource as parsed from a single file, before cross-file validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedResource {
    pub name: String,
    pub export_name: String,
    pub path: PathBuf,
    pub span: Span,
    pub fields: Vec<FieldIr>,
    pub constraints: Vec<ConstraintIr>,
    pub access: Option<AccessIr>,
    pub access_missing_span: Span,
    pub actions: Vec<ActionIr>,
}

type Diag = Diagnostic;

pub fn lower_program(
    program: &Program<'_>,
    path: &Path,
    source: &str,
) -> Result<ParsedResource, Vec<Diag>> {
    let mut diags = Vec::new();
    let mut resources = Vec::new();

    for stmt in &program.body {
        match stmt {
            Statement::ExportDeclaration(export) => {
                match &export.declaration {
                    Declaration::VariableDeclaration(var) => {
                        if var.kind != VariableDeclarationKind::Const {
                            diags.push(
                                Diagnostic::error(
                                    ErrorCode::E0101,
                                    path.to_path_buf(),
                                    ox_span(var.span),
                                    "resource files must use `export const`",
                                )
                                .with_suggestion(
                                    "change to `export const Name = resource(...)`",
                                    Some("export const".into()),
                                ),
                            );
                            continue;
                        }
                        for decl in &var.declarations {
                            match lower_declarator(decl, path, source) {
                                Ok(r) => resources.push(r),
                                Err(mut d) => diags.append(&mut d),
                            }
                        }
                    }
                    other => {
                        diags.push(
                            Diagnostic::error(
                                ErrorCode::E0101,
                                path.to_path_buf(),
                                ox_span(other.span()),
                                "disallowed export in resource file",
                            )
                            .with_suggestion(
                                "export a single `const` bound to `resource(...)`",
                                None,
                            ),
                        );
                    }
                }
            }
            Statement::EmptyStatement(_) => {}
            other => {
                diags.push(
                    Diagnostic::error(
                        ErrorCode::E0101,
                        path.to_path_buf(),
                        ox_span(other.span()),
                        "disallowed construct in resource file",
                    )
                    .with_suggestion(
                        "only `export const Name = resource(...)` is allowed",
                        None,
                    ),
                );
            }
        }
    }

    // Also reject non-export module decls that somehow appear (imports etc. via ModuleDeclaration)
    for stmt in &program.body {
        if let Statement::ImportDeclaration(imp) = stmt {
            diags.push(
                Diagnostic::error(
                    ErrorCode::E0101,
                    path.to_path_buf(),
                    ox_span(imp.span),
                    "imports are not allowed in resource files yet",
                )
                .with_suggestion(
                    "remove the import; `resource` / `field` / `rule` / `limit` are ambient",
                    None,
                ),
            );
        }
        if let Statement::ExportNamedDeclaration(exp) = stmt {
            diags.push(
                Diagnostic::error(
                    ErrorCode::E0101,
                    path.to_path_buf(),
                    ox_span(exp.span),
                    "named re-exports are not allowed in resource files",
                )
                .with_suggestion("export a single resource const instead", None),
            );
        }
    }

    if !diags.is_empty() && resources.is_empty() {
        return Err(diags);
    }

    match resources.len() {
        0 => {
            diags.push(
                Diagnostic::error(
                    ErrorCode::E0209,
                    path.to_path_buf(),
                    Span::dummy(),
                    "resource file must export exactly one resource",
                )
                .with_suggestion(
                    "add `export const Name = resource(\"name\", { ... })`",
                    None,
                ),
            );
            Err(diags)
        }
        1 => {
            if diags.is_empty() {
                Ok(resources.remove(0))
            } else {
                Err(diags)
            }
        }
        _ => {
            for r in &resources[1..] {
                diags.push(
                    Diagnostic::error(
                        ErrorCode::E0209,
                        path.to_path_buf(),
                        r.span,
                        "multiple resources in one file",
                    )
                    .with_suggestion("move each resource to its own `*.resource.ts` file", None)
                    .with_root_id(format!("E0209:{}", path.display())),
                );
            }
            Err(diags)
        }
    }
}

fn lower_declarator(
    decl: &oxc_ast::ast::VariableDeclarator<'_>,
    path: &Path,
    source: &str,
) -> Result<ParsedResource, Vec<Diag>> {
    let mut diags = Vec::new();
    let export_name = match binding_name(&decl.id) {
        Some(n) => n,
        None => {
            return Err(vec![Diagnostic::error(
                ErrorCode::E0101,
                path.to_path_buf(),
                ox_span(decl.span),
                "resource binding must be a simple identifier",
            )]);
        }
    };

    let Some(init) = &decl.init else {
        return Err(vec![Diagnostic::error(
            ErrorCode::E0101,
            path.to_path_buf(),
            ox_span(decl.span),
            "resource const must be initialized with `resource(...)`",
        )]);
    };

    let Expression::CallExpression(call) = init else {
        return Err(vec![Diagnostic::error(
            ErrorCode::E0101,
            path.to_path_buf(),
            ox_span(init.span()),
            "expected `resource(\"name\", { ... })` call",
        )
        .with_suggestion("wrap the declaration in resource(...)", None)]);
    };

    if !is_ident_call(call, "resource") {
        return Err(vec![Diagnostic::error(
            ErrorCode::E0101,
            path.to_path_buf(),
            ox_span(call.span),
            "expected top-level `resource(...)` call",
        )]);
    }

    if call.arguments.len() != 2 {
        return Err(vec![Diagnostic::error(
            ErrorCode::E0101,
            path.to_path_buf(),
            ox_span(call.span),
            "`resource` expects exactly two arguments: name and config object",
        )]);
    }

    let name = match string_arg(&call.arguments[0]) {
        Some((n, _)) => n,
        None => {
            return Err(vec![Diagnostic::error(
                ErrorCode::E0101,
                path.to_path_buf(),
                ox_span(call.arguments[0].span()),
                "resource name must be a string literal",
            )]);
        }
    };

    let Some(obj) = object_arg(&call.arguments[1]) else {
        return Err(vec![Diagnostic::error(
            ErrorCode::E0101,
            path.to_path_buf(),
            ox_span(call.arguments[1].span()),
            "resource config must be an object literal",
        )]);
    };

    let mut fields = Vec::new();
    let mut constraints = Vec::new();
    let mut access = None;
    let mut actions = Vec::new();
    let mut seen_keys = std::collections::BTreeSet::new();

    for prop in &obj.properties {
        let ObjectPropertyKind::ObjectProperty(p) = prop else {
            diags.push(Diagnostic::error(
                ErrorCode::E0101,
                path.to_path_buf(),
                ox_span(obj.span),
                "spread properties are not allowed in resource config",
            ));
            continue;
        };
        let Some(key) = static_key(&p.key) else {
            diags.push(Diagnostic::error(
                ErrorCode::E0101,
                path.to_path_buf(),
                ox_span(p.span),
                "resource config keys must be static identifiers",
            ));
            continue;
        };
        if !seen_keys.insert(key.clone()) {
            diags.push(Diagnostic::error(
                ErrorCode::E0101,
                path.to_path_buf(),
                ox_span(p.span),
                format!("duplicate resource config key `{key}`"),
            ));
            continue;
        }
        match key.as_str() {
            "fields" => match lower_fields(&p.value, path) {
                Ok(f) => fields = f,
                Err(mut d) => diags.append(&mut d),
            },
            "constraints" => match lower_constraints(&p.value, path) {
                Ok(c) => constraints = c,
                Err(mut d) => diags.append(&mut d),
            },
            "access" => match lower_access(&p.value, path) {
                Ok(a) => access = Some(a),
                Err(mut d) => diags.append(&mut d),
            },
            "actions" => match lower_actions(&p.value, path, source) {
                Ok(a) => actions = a,
                Err(mut d) => diags.append(&mut d),
            },
            other => {
                diags.push(
                    Diagnostic::error(
                        ErrorCode::E0101,
                        path.to_path_buf(),
                        ox_span(p.span),
                        format!("unknown resource config key `{other}`"),
                    )
                    .with_suggestion(
                        "allowed keys: fields, constraints, access, actions",
                        None,
                    ),
                );
            }
        }
    }

    if !diags.is_empty() {
        return Err(diags);
    }

    Ok(ParsedResource {
        name,
        export_name,
        path: path.to_path_buf(),
        span: ox_span(call.span),
        fields,
        constraints,
        access_missing_span: ox_span(obj.span),
        access,
        actions,
    })
}

fn lower_fields(expr: &Expression<'_>, path: &Path) -> Result<Vec<FieldIr>, Vec<Diag>> {
    let Some(obj) = as_object(expr) else {
        return Err(vec![Diagnostic::error(
            ErrorCode::E0101,
            path.to_path_buf(),
            ox_span(expr.span()),
            "`fields` must be an object literal",
        )]);
    };
    let mut fields = Vec::new();
    let mut diags = Vec::new();
    let mut names = std::collections::BTreeSet::new();

    for prop in &obj.properties {
        let ObjectPropertyKind::ObjectProperty(p) = prop else {
            diags.push(Diagnostic::error(
                ErrorCode::E0101,
                path.to_path_buf(),
                ox_span(obj.span),
                "spread is not allowed in fields",
            ));
            continue;
        };
        let Some(name) = static_key(&p.key) else {
            diags.push(Diagnostic::error(
                ErrorCode::E0101,
                path.to_path_buf(),
                ox_span(p.span),
                "field names must be static identifiers",
            ));
            continue;
        };
        if !names.insert(name.clone()) {
            diags.push(
                Diagnostic::error(
                    ErrorCode::E0203,
                    path.to_path_buf(),
                    ox_span(p.span),
                    format!("duplicate field name `{name}`"),
                )
                .with_suggestion(format!("rename one of the `{name}` fields"), None),
            );
            continue;
        }
        match lower_field(&name, &p.value, path) {
            Ok(f) => fields.push(f),
            Err(mut d) => diags.append(&mut d),
        }
    }

    if diags.is_empty() {
        Ok(fields)
    } else {
        Err(diags)
    }
}

fn lower_field(name: &str, expr: &Expression<'_>, path: &Path) -> Result<FieldIr, Vec<Diag>> {
    let Expression::CallExpression(call) = expr else {
        return Err(vec![Diagnostic::error(
            ErrorCode::E0101,
            path.to_path_buf(),
            ox_span(expr.span()),
            format!("field `{name}` must be a `field.<kind>(...)` call"),
        )]);
    };

    let (receiver, method) = match member_call(call) {
        Some(m) => m,
        None => {
            return Err(vec![Diagnostic::error(
                ErrorCode::E0101,
                path.to_path_buf(),
                ox_span(call.span),
                format!("field `{name}` must be a `field.<kind>(...)` call"),
            )]);
        }
    };
    if receiver != "field" {
        return Err(vec![Diagnostic::error(
            ErrorCode::E0101,
            path.to_path_buf(),
            ox_span(call.span),
            format!("expected `field.{method}(...)`, found `{receiver}.{method}`"),
        )]);
    }

    let kind = match method.as_str() {
        "id" => FieldKind::Id,
        "text" => FieldKind::Text,
        "bool" => FieldKind::Bool,
        "date" => FieldKind::Date,
        "timestamp" => FieldKind::Timestamp,
        "ref" => {
            let Some((target, span)) = call.arguments.first().and_then(string_arg) else {
                return Err(vec![Diagnostic::error(
                    ErrorCode::E0205,
                    path.to_path_buf(),
                    ox_span(call.span),
                    "`field.ref` requires a string resource name",
                )
                .with_suggestion(
                    "pass the target resource name",
                    Some("field.ref(\"user\")".into()),
                )]);
            };
            let _ = span;
            FieldKind::Ref { target }
        }
        "computed" => {
            let src = if let Some(arg) = call.arguments.first() {
                source_slice_for_arg(arg)
            } else {
                String::new()
            };
            FieldKind::Computed { source: src }
        }
        other => {
            return Err(vec![Diagnostic::error(
                ErrorCode::E0208,
                path.to_path_buf(),
                ox_span(call.span),
                format!("unknown field builder `field.{other}`"),
            )
            .with_suggestion(
                "use one of: id, text, bool, date, timestamp, ref, computed",
                None,
            )]);
        }
    };

    let options = options_from_args(call, &kind, path)?;

    Ok(FieldIr {
        name: name.to_string(),
        span: ox_span(call.span),
        kind,
        options,
    })
}

fn options_from_args(
    call: &CallExpression<'_>,
    kind: &FieldKind,
    path: &Path,
) -> Result<FieldOptions, Vec<Diag>> {
    // field.ref("x") or field.ref("x", { ... })
    // field.id() or field.id({ ... })
    // field.computed(fn) — no options object expected as first arg
    let opts_arg = match kind {
        FieldKind::Ref { .. } => call.arguments.get(1),
        FieldKind::Computed { .. } => {
            if call.arguments.len() > 1 {
                return Err(vec![Diagnostic::error(
                    ErrorCode::E0205,
                    path.to_path_buf(),
                    ox_span(call.span),
                    "`field.computed` does not take an options object yet",
                )]);
            }
            return Ok(FieldOptions::default());
        }
        _ => call.arguments.first(),
    };

    let Some(arg) = opts_arg else {
        return Ok(FieldOptions::default());
    };
    let Some(obj) = object_arg(arg) else {
        // field.ref with only string is fine; other kinds with non-object arg is error
        if matches!(kind, FieldKind::Ref { .. }) && string_arg(arg).is_some() {
            return Ok(FieldOptions::default());
        }
        return Err(vec![Diagnostic::error(
            ErrorCode::E0205,
            path.to_path_buf(),
            ox_span(arg.span()),
            "field options must be an object literal",
        )]);
    };

    let mut options = FieldOptions::default();
    let mut diags = Vec::new();

    for prop in &obj.properties {
        let ObjectPropertyKind::ObjectProperty(p) = prop else {
            diags.push(Diagnostic::error(
                ErrorCode::E0101,
                path.to_path_buf(),
                ox_span(obj.span),
                "spread is not allowed in field options",
            ));
            continue;
        };
        let Some(key) = static_key(&p.key) else {
            diags.push(Diagnostic::error(
                ErrorCode::E0205,
                path.to_path_buf(),
                ox_span(p.span),
                "option keys must be static identifiers",
            ));
            continue;
        };
        match key.as_str() {
            "optional" => match bool_lit(&p.value) {
                Some(v) => options.optional = Some(v),
                None => diags.push(bad_option(path, p.span, "optional", "boolean")),
            },
            "default" => match literal_value(&p.value) {
                Some(v) => options.default = Some(v),
                None => diags.push(bad_option(path, p.span, "default", "literal")),
            },
            "max" => match int_lit(&p.value) {
                Some(v) => options.max = Some(v),
                None => diags.push(
                    Diagnostic::error(
                        ErrorCode::E0205,
                        path.to_path_buf(),
                        ox_span(p.value.span()),
                        "`max` must be an integer literal",
                    )
                    .with_suggestion("use a number", Some("max: 200".into())),
                ),
            },
            "filterable" => match bool_lit(&p.value) {
                Some(v) => options.filterable = Some(v),
                None => diags.push(bad_option(path, p.span, "filterable", "boolean")),
            },
            "sortable" => match bool_lit(&p.value) {
                Some(v) => options.sortable = Some(v),
                None => diags.push(bad_option(path, p.span, "sortable", "boolean")),
            },
            "auto" => match bool_lit(&p.value) {
                Some(v) => options.auto = Some(v),
                None => diags.push(bad_option(path, p.span, "auto", "boolean")),
            },
            "renamedFrom" => match string_lit(&p.value) {
                Some((v, _)) => options.renamed_from = Some(v),
                None => diags.push(bad_option(path, p.span, "renamedFrom", "string")),
            },
            other => {
                diags.push(
                    Diagnostic::error(
                        ErrorCode::E0205,
                        path.to_path_buf(),
                        ox_span(p.span),
                        format!("unknown field option `{other}`"),
                    )
                    .with_suggestion(
                        "allowed: optional, default, max, filterable, sortable, auto, renamedFrom",
                        None,
                    ),
                );
            }
        }
    }

    if diags.is_empty() {
        Ok(options)
    } else {
        Err(diags)
    }
}

fn bad_option(path: &Path, span: oxc_span::Span, name: &str, expected: &str) -> Diag {
    Diagnostic::error(
        ErrorCode::E0205,
        path.to_path_buf(),
        ox_span(span),
        format!("`{name}` must be a {expected}"),
    )
}

fn lower_constraints(expr: &Expression<'_>, path: &Path) -> Result<Vec<ConstraintIr>, Vec<Diag>> {
    let Expression::ArrayExpression(arr) = expr else {
        return Err(vec![Diagnostic::error(
            ErrorCode::E0101,
            path.to_path_buf(),
            ox_span(expr.span()),
            "`constraints` must be an array literal",
        )]);
    };
    let mut out = Vec::new();
    let mut diags = Vec::new();
    for el in &arr.elements {
        let Some(expr) = el.as_expression() else {
            diags.push(Diagnostic::error(
                ErrorCode::E0101,
                path.to_path_buf(),
                ox_span(arr.span),
                "invalid constraint element",
            ));
            continue;
        };
        match lower_constraint(expr, path) {
            Ok(c) => out.push(c),
            Err(mut d) => diags.append(&mut d),
        }
    }
    if diags.is_empty() {
        Ok(out)
    } else {
        Err(diags)
    }
}

fn lower_constraint(expr: &Expression<'_>, path: &Path) -> Result<ConstraintIr, Vec<Diag>> {
    let Expression::CallExpression(call) = expr else {
        return Err(vec![Diagnostic::error(
            ErrorCode::E0101,
            path.to_path_buf(),
            ox_span(expr.span()),
            "constraint must be a `limit(...)` call",
        )]);
    };
    if !is_ident_call(call, "limit") {
        return Err(vec![Diagnostic::error(
            ErrorCode::E0208,
            path.to_path_buf(),
            ox_span(call.span),
            "unknown constraint helper",
        )
        .with_suggestion("use `limit({ per, where, max })`", None)]);
    }
    let Some(obj) = call.arguments.first().and_then(object_arg) else {
        return Err(vec![Diagnostic::error(
            ErrorCode::E0205,
            path.to_path_buf(),
            ox_span(call.span),
            "`limit` requires an options object",
        )]);
    };

    let mut per = None;
    let mut where_fields = Vec::new();
    let mut max = None;
    let mut diags = Vec::new();

    for prop in &obj.properties {
        let ObjectPropertyKind::ObjectProperty(p) = prop else {
            continue;
        };
        let Some(key) = static_key(&p.key) else {
            continue;
        };
        match key.as_str() {
            "per" => match string_lit(&p.value) {
                Some((v, _)) => per = Some(v),
                None => diags.push(bad_option(path, p.span, "per", "string field name")),
            },
            "max" => match int_lit(&p.value) {
                Some(v) => max = Some(v),
                None => diags.push(bad_option(path, p.span, "max", "integer")),
            },
            "where" => {
                let Some(wobj) = as_object(&p.value) else {
                    diags.push(bad_option(path, p.span, "where", "object literal"));
                    continue;
                };
                for wp in &wobj.properties {
                    let ObjectPropertyKind::ObjectProperty(wp) = wp else {
                        continue;
                    };
                    let Some(wkey) = static_key(&wp.key) else {
                        continue;
                    };
                    match literal_value(&wp.value) {
                        Some(v) => where_fields.push((wkey, v)),
                        None => diags.push(Diagnostic::error(
                            ErrorCode::E0205,
                            path.to_path_buf(),
                            ox_span(wp.span),
                            "constraint where values must be literals",
                        )),
                    }
                }
            }
            other => diags.push(
                Diagnostic::error(
                    ErrorCode::E0205,
                    path.to_path_buf(),
                    ox_span(p.span),
                    format!("unknown limit option `{other}`"),
                )
                .with_suggestion("allowed: per, where, max", None),
            ),
        }
    }

    let (Some(per), Some(max)) = (per, max) else {
        diags.push(
            Diagnostic::error(
                ErrorCode::E0205,
                path.to_path_buf(),
                ox_span(call.span),
                "`limit` requires `per` and `max`",
            )
            .with_suggestion(
                "provide both",
                Some("limit({ per: \"owner\", where: { done: false }, max: 10 })".into()),
            ),
        );
        return Err(diags);
    };

    if !diags.is_empty() {
        return Err(diags);
    }

    where_fields.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(ConstraintIr::Limit {
        span: ox_span(call.span),
        per,
        where_fields,
        max,
    })
}

fn lower_access(expr: &Expression<'_>, path: &Path) -> Result<AccessIr, Vec<Diag>> {
    let Some(obj) = as_object(expr) else {
        return Err(vec![Diagnostic::error(
            ErrorCode::E0101,
            path.to_path_buf(),
            ox_span(expr.span()),
            "`access` must be an object literal",
        )]);
    };
    let mut operations = Vec::new();
    let mut diags = Vec::new();

    for prop in &obj.properties {
        let ObjectPropertyKind::ObjectProperty(p) = prop else {
            diags.push(Diagnostic::error(
                ErrorCode::E0101,
                path.to_path_buf(),
                ox_span(obj.span),
                "spread is not allowed in access",
            ));
            continue;
        };
        let Some(name) = static_key(&p.key) else {
            diags.push(Diagnostic::error(
                ErrorCode::E0101,
                path.to_path_buf(),
                ox_span(p.span),
                "access operation names must be static identifiers",
            ));
            continue;
        };
        match lower_rule(&p.value, path) {
            Ok(rule) => operations.push(AccessOpIr {
                name,
                span: ox_span(p.span),
                rule,
            }),
            Err(mut d) => diags.append(&mut d),
        }
    }

    operations.sort_by(|a, b| a.name.cmp(&b.name));

    if diags.is_empty() {
        Ok(AccessIr {
            span: ox_span(obj.span),
            operations,
        })
    } else {
        Err(diags)
    }
}

fn lower_rule(expr: &Expression<'_>, path: &Path) -> Result<RuleIr, Vec<Diag>> {
    let Expression::CallExpression(call) = expr else {
        return Err(vec![Diagnostic::error(
            ErrorCode::E0101,
            path.to_path_buf(),
            ox_span(expr.span()),
            "access rule must be a `rule.<kind>(...)` call",
        )]);
    };
    let (receiver, method) = match member_call(call) {
        Some(m) => m,
        None => {
            return Err(vec![Diagnostic::error(
                ErrorCode::E0101,
                path.to_path_buf(),
                ox_span(call.span),
                "access rule must be a `rule.<kind>(...)` call",
            )]);
        }
    };
    if receiver != "rule" {
        return Err(vec![Diagnostic::error(
            ErrorCode::E0101,
            path.to_path_buf(),
            ox_span(call.span),
            format!("expected `rule.{method}`, found `{receiver}.{method}`"),
        )]);
    }
    match method.as_str() {
        "authenticated" => Ok(RuleIr::Authenticated {
            span: ox_span(call.span),
        }),
        "owner" => {
            let Some((field, _)) = call.arguments.first().and_then(string_arg) else {
                return Err(vec![Diagnostic::error(
                    ErrorCode::E0205,
                    path.to_path_buf(),
                    ox_span(call.span),
                    "`rule.owner` requires a field name string",
                )
                .with_suggestion(
                    "pass the owner field",
                    Some("rule.owner(\"owner\")".into()),
                )]);
            };
            Ok(RuleIr::Owner {
                span: ox_span(call.span),
                field,
            })
        }
        "role" => {
            let Some((role, _)) = call.arguments.first().and_then(string_arg) else {
                return Err(vec![Diagnostic::error(
                    ErrorCode::E0205,
                    path.to_path_buf(),
                    ox_span(call.span),
                    "`rule.role` requires a role name string",
                )]);
            };
            Ok(RuleIr::Role {
                span: ox_span(call.span),
                role,
            })
        }
        "member" => {
            let Some((field, _)) = call.arguments.first().and_then(string_arg) else {
                return Err(vec![Diagnostic::error(
                    ErrorCode::E0205,
                    path.to_path_buf(),
                    ox_span(call.span),
                    "`rule.member` requires a field name string",
                )]);
            };
            Ok(RuleIr::Member {
                span: ox_span(call.span),
                field,
            })
        }
        other => Err(vec![Diagnostic::error(
            ErrorCode::E0208,
            path.to_path_buf(),
            ox_span(call.span),
            format!("unknown rule helper `rule.{other}`"),
        )
        .with_suggestion(
            "use one of: authenticated, owner, role, member",
            None,
        )]),
    }
}

fn lower_actions(
    expr: &Expression<'_>,
    path: &Path,
    source: &str,
) -> Result<Vec<ActionIr>, Vec<Diag>> {
    let _ = source;
    let Some(obj) = as_object(expr) else {
        return Err(vec![Diagnostic::error(
            ErrorCode::E0101,
            path.to_path_buf(),
            ox_span(expr.span()),
            "`actions` must be an object literal",
        )]);
    };
    let mut actions = Vec::new();
    let mut diags = Vec::new();

    for prop in &obj.properties {
        let ObjectPropertyKind::ObjectProperty(p) = prop else {
            continue;
        };
        let Some(name) = static_key(&p.key) else {
            continue;
        };
        let Some(cfg) = as_object(&p.value) else {
            diags.push(Diagnostic::error(
                ErrorCode::E0101,
                path.to_path_buf(),
                ox_span(p.span),
                "action config must be an object `{ input, run }`",
            ));
            continue;
        };
        let mut input = Vec::new();
        let mut run = None;
        let mut run_span = Span::dummy();

        for cp in &cfg.properties {
            let ObjectPropertyKind::ObjectProperty(cp) = cp else {
                continue;
            };
            let Some(ckey) = static_key(&cp.key) else {
                continue;
            };
            match ckey.as_str() {
                "input" => match lower_fields(&cp.value, path) {
                    Ok(f) => input = f,
                    Err(mut d) => diags.append(&mut d),
                },
                "run" => match string_lit(&cp.value) {
                    Some((v, sp)) => {
                        run = Some(v);
                        run_span = sp;
                    }
                    None => diags.push(Diagnostic::error(
                        ErrorCode::E0205,
                        path.to_path_buf(),
                        ox_span(cp.span),
                        "`run` must be a string path to an action file",
                    )),
                },
                other => diags.push(
                    Diagnostic::error(
                        ErrorCode::E0101,
                        path.to_path_buf(),
                        ox_span(cp.span),
                        format!("unknown action key `{other}`"),
                    )
                    .with_suggestion("allowed: input, run", None),
                ),
            }
        }

        let Some(run) = run else {
            diags.push(
                Diagnostic::error(
                    ErrorCode::E0205,
                    path.to_path_buf(),
                    ox_span(p.span),
                    "action requires `run`",
                )
                .with_suggestion(
                    "add a relative path",
                    Some("run: \"./archive.action.ts\"".into()),
                ),
            );
            continue;
        };

        actions.push(ActionIr {
            name,
            span: ox_span(p.span),
            input,
            run,
            run_span,
        });
    }

    actions.sort_by(|a, b| a.name.cmp(&b.name));
    if diags.is_empty() {
        Ok(actions)
    } else {
        Err(diags)
    }
}

// --- helpers ---

fn ox_span(s: oxc_span::Span) -> Span {
    Span::new(s.start, s.end)
}

fn binding_name(id: &BindingPattern<'_>) -> Option<String> {
    id.get_identifier_name().map(|n| n.as_str().to_string())
}

fn is_ident_call(call: &CallExpression<'_>, name: &str) -> bool {
    matches!(&call.callee, Expression::Identifier(id) if id.name.as_str() == name)
}

fn member_call(call: &CallExpression<'_>) -> Option<(String, String)> {
    match &call.callee {
        Expression::StaticMemberExpression(m) => {
            let Expression::Identifier(obj) = &m.object else {
                return None;
            };
            Some((obj.name.as_str().to_string(), m.property.name.as_str().to_string()))
        }
        _ => None,
    }
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

fn object_arg<'a>(arg: &'a Argument<'a>) -> Option<&'a ObjectExpression<'a>> {
    match arg {
        Argument::ObjectExpression(o) => Some(o),
        Argument::ParenthesizedExpression(p) => as_object(&p.expression),
        _ => None,
    }
}

fn string_arg(arg: &Argument<'_>) -> Option<(String, Span)> {
    match arg {
        Argument::StringLiteral(s) => Some((s.value.as_str().to_string(), ox_span(s.span))),
        _ => None,
    }
}

fn string_lit(expr: &Expression<'_>) -> Option<(String, Span)> {
    match expr {
        Expression::StringLiteral(s) => Some((s.value.as_str().to_string(), ox_span(s.span))),
        Expression::ParenthesizedExpression(p) => string_lit(&p.expression),
        _ => None,
    }
}

fn bool_lit(expr: &Expression<'_>) -> Option<bool> {
    match expr {
        Expression::BooleanLiteral(b) => Some(b.value),
        Expression::ParenthesizedExpression(p) => bool_lit(&p.expression),
        _ => None,
    }
}

fn int_lit(expr: &Expression<'_>) -> Option<i64> {
    match expr {
        Expression::NumericLiteral(n) => {
            if n.value.fract() == 0.0 && n.value >= i64::MIN as f64 && n.value <= i64::MAX as f64 {
                Some(n.value as i64)
            } else {
                None
            }
        }
        Expression::UnaryExpression(u)
            if u.operator == oxc_ast::ast::UnaryOperator::UnaryNegation =>
        {
            int_lit(&u.argument).map(|v| -v)
        }
        Expression::ParenthesizedExpression(p) => int_lit(&p.expression),
        _ => None,
    }
}

fn literal_value(expr: &Expression<'_>) -> Option<LiteralValue> {
    match expr {
        Expression::BooleanLiteral(b) => Some(LiteralValue::Bool(b.value)),
        Expression::NumericLiteral(n) => {
            if n.value.fract() == 0.0 {
                Some(LiteralValue::Int(n.value as i64))
            } else {
                Some(LiteralValue::Float(n.raw.map(|r| r.to_string()).unwrap_or_else(|| n.value.to_string())))
            }
        }
        Expression::StringLiteral(s) => Some(LiteralValue::String(s.value.as_str().to_string())),
        Expression::NullLiteral(_) => Some(LiteralValue::Null),
        Expression::UnaryExpression(u)
            if u.operator == oxc_ast::ast::UnaryOperator::UnaryNegation =>
        {
            match literal_value(&u.argument)? {
                LiteralValue::Int(i) => Some(LiteralValue::Int(-i)),
                LiteralValue::Float(f) => Some(LiteralValue::Float(format!("-{f}"))),
                _ => None,
            }
        }
        Expression::ParenthesizedExpression(p) => literal_value(&p.expression),
        _ => None,
    }
}

fn source_slice_for_arg(arg: &Argument<'_>) -> String {
    // Prefer a stable placeholder describing the shape; full expression subset is open.
    match arg {
        Argument::ArrowFunctionExpression(_) => "(() => /* computed */)".to_string(),
        Argument::FunctionExpression(_) => "(function /* computed */)".to_string(),
        other => format!("{:?}", std::mem::discriminant(other)),
    }
}

trait ArgSpan {
    fn span(&self) -> oxc_span::Span;
}

impl ArgSpan for Argument<'_> {
    fn span(&self) -> oxc_span::Span {
        use oxc_span::GetSpan;
        GetSpan::span(self)
    }
}

trait ExprSpan {
    fn span(&self) -> oxc_span::Span;
}

impl ExprSpan for Expression<'_> {
    fn span(&self) -> oxc_span::Span {
        use oxc_span::GetSpan;
        GetSpan::span(self)
    }
}

trait StmtSpan {
    fn span(&self) -> oxc_span::Span;
}

impl StmtSpan for Statement<'_> {
    fn span(&self) -> oxc_span::Span {
        use oxc_span::GetSpan;
        GetSpan::span(self)
    }
}

trait DeclSpan {
    fn span(&self) -> oxc_span::Span;
}

impl DeclSpan for Declaration<'_> {
    fn span(&self) -> oxc_span::Span {
        use oxc_span::GetSpan;
        GetSpan::span(self)
    }
}
