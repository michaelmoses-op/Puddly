//! Cross-file validation: parsed resources → [`AppIr`] + diagnostics.

use puddly_diagnostics::{Diagnostic, ErrorCode, Report};
use puddly_ir::{
    AppIr, ConstraintIr, FieldKind, ResourceIr, RuleIr, SourceFile, Span,
};
use puddly_parse::{discover_resource_files, parse_resource_path, ParsedResource};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// Validate an app rooted at `root` (directory containing `features/` or resource files).
pub fn validate_app(root: &Path) -> (Option<AppIr>, Report, Vec<SourceFile>) {
    let mut report = Report::new();
    let mut files = Vec::new();

    let resource_paths = match discover_resource_files(root) {
        Ok(p) => p,
        Err(e) => {
            report.push(Diagnostic::error(
                ErrorCode::E0900,
                root.to_path_buf(),
                Span::dummy(),
                format!("failed to scan {}: {e}", root.display()),
            ));
            report.finalize();
            return (None, report, files);
        }
    };

    if resource_paths.is_empty() {
        report.push(
            Diagnostic::error(
                ErrorCode::E0900,
                root.to_path_buf(),
                Span::dummy(),
                format!("no `*.resource.ts` files found under {}", root.display()),
            )
            .with_suggestion(
                "create a resource file under features/",
                Some("export const Todo = resource(\"todo\", { ... })".into()),
            ),
        );
        report.finalize();
        return (None, report, files);
    }

    let mut parsed: Vec<ParsedResource> = Vec::new();
    for path in &resource_paths {
        let (res, file_report, source) = parse_resource_path(path);
        if let Some(s) = source {
            files.push(s);
        }
        report.extend(file_report.diagnostics);
        if let Some(r) = res {
            parsed.push(r);
        }
    }

    // Sort for determinism
    parsed.sort_by(|a, b| a.name.cmp(&b.name).then(a.path.cmp(&b.path)));
    files.sort_by(|a, b| a.path.cmp(&b.path));

    let names: BTreeSet<String> = parsed.iter().map(|r| r.name.clone()).collect();

    // Duplicate resource names
    let mut seen: BTreeMap<String, PathBuf> = BTreeMap::new();
    for r in &parsed {
        if let Some(prev) = seen.insert(r.name.clone(), r.path.clone()) {
            report.push(
                Diagnostic::error(
                    ErrorCode::E0202,
                    r.path.clone(),
                    r.span,
                    format!("duplicate resource name `{}`", r.name),
                )
                .with_label(r.span, "redeclared here")
                .with_suggestion(
                    format!("rename this resource; first declared in {}", prev.display()),
                    Some(format!("resource(\"{}_2\", {{ ... }})", r.name)),
                )
                .with_root_id(format!("E0202:{}", r.name)),
            );
        }
    }

    let by_name: BTreeMap<String, &ParsedResource> =
        parsed.iter().map(|r| (r.name.clone(), r)).collect();

    for r in &parsed {
        validate_one(r, &names, &by_name, &mut report);
    }

    let mut resources = Vec::new();
    for r in parsed {
        // Still emit IR for successfully shaped resources even if cross-file errors exist,
        // but only return AppIr when there are no errors.
        if r.access.is_some() {
            resources.push(resolve_member_defaults(to_resource_ir(r)));
        }
    }

    report.finalize();
    files.sort_by(|a, b| a.path.cmp(&b.path));
    resources.sort_by(|a, b| a.name.cmp(&b.name));

    if report.has_errors() {
        (None, report, files)
    } else {
        (
            Some(AppIr {
                resources,
                files: files.clone(),
            }),
            report,
            files,
        )
    }
}

fn validate_one(
    r: &ParsedResource,
    known: &BTreeSet<String>,
    by_name: &BTreeMap<String, &ParsedResource>,
    report: &mut Report,
) {
    // Reserved built-in RPC resource name
    if r.name == "auth" {
        report.push(
            Diagnostic::error(
                ErrorCode::E0211,
                r.path.clone(),
                r.span,
                "resource name `auth` is reserved for built-in auth RPC",
            )
            .with_suggestion(
                "rename the resource (e.g. `account`)",
                Some("resource(\"account\", { ... })".into()),
            )
            .with_root_id(format!("E0211:{}", r.name)),
        );
    }

    // Missing access
    if r.access.is_none() {
        report.push(
            Diagnostic::error(
                ErrorCode::E0201,
                r.path.clone(),
                r.access_missing_span,
                format!("resource `{}` is missing required `access` block", r.name),
            )
            .with_suggestion(
                "add an access object",
                Some(
                    "access: { list: rule.authenticated(), create: rule.authenticated(), update: rule.authenticated(), delete: rule.authenticated() }"
                        .into(),
                ),
            )
            .with_root_id(format!("E0201:{}", r.name)),
        );
    }

    let field_names: BTreeSet<&str> = r.fields.iter().map(|f| f.name.as_str()).collect();
    let filterable: BTreeSet<&str> = r
        .fields
        .iter()
        .filter(|f| f.options.filterable == Some(true))
        .map(|f| f.name.as_str())
        .collect();

    // Refs
    for f in &r.fields {
        if let FieldKind::Ref { target } = &f.kind {
            if !known.contains(target) {
                let suggestion = closest(target, known.iter().map(|s| s.as_str()));
                let mut d = Diagnostic::error(
                    ErrorCode::E0204,
                    r.path.clone(),
                    f.span,
                    format!("unknown reference target `{target}`"),
                )
                .with_label(f.span, format!("no resource named `{target}`"))
                .with_root_id(format!("E0204:{}:{}", r.name, f.name));
                if let Some(s) = suggestion {
                    d = d.with_suggestion(
                        format!("did you mean `{s}`?"),
                        Some(format!("field.ref(\"{s}\")")),
                    );
                } else {
                    d = d.with_suggestion(
                        "declare the target resource or fix the name",
                        None,
                    );
                }
                report.push(d);
            }
        }
    }

    // Access rule field refs + member join targets
    if let Some(access) = &r.access {
        for op in &access.operations {
            match &op.rule {
                RuleIr::Owner { field, span } if !field_names.contains(field.as_str()) => {
                    let suggestion = closest(field, field_names.iter().copied());
                    let mut d = Diagnostic::error(
                        ErrorCode::E0210,
                        r.path.clone(),
                        *span,
                        format!("access rule references unknown field `{field}`"),
                    )
                    .with_root_id(format!("E0210:{}:{}:{}", r.name, op.name, field));
                    if let Some(s) = suggestion {
                        d = d.with_suggestion(
                            format!("did you mean `{s}`?"),
                            Some(format!("rule.owner(\"{s}\")")),
                        );
                    }
                    report.push(d);
                }
                RuleIr::Member {
                    via,
                    on,
                    user_field,
                    span,
                } => {
                    if op.name == "create" {
                        report.push(
                            Diagnostic::error(
                                ErrorCode::E0214,
                                r.path.clone(),
                                *span,
                                "`rule.member` is not valid on `create`",
                            )
                            .with_suggestion(
                                "use authenticated (or owner) on create; insert join rows separately",
                                Some("create: rule.authenticated()".into()),
                            )
                            .with_root_id(format!("E0214:{}:create", r.name)),
                        );
                    }
                    validate_member_join(r, op.name.as_str(), via, on, user_field, *span, by_name, report);
                }
                _ => {}
            }
        }
    }

    // Constraints
    for c in &r.constraints {
        let ConstraintIr::Limit {
            span,
            per,
            where_fields,
            ..
        } = c;
        if !field_names.contains(per.as_str()) {
            report.push(
                Diagnostic::error(
                    ErrorCode::E0206,
                    r.path.clone(),
                    *span,
                    format!("limit `per` references unknown field `{per}`"),
                )
                .with_suggestion("use a field defined on this resource", None)
                .with_root_id(format!("E0206:{}:per:{}", r.name, per)),
            );
        }
        for (wkey, _) in where_fields {
            if !field_names.contains(wkey.as_str()) {
                report.push(
                    Diagnostic::error(
                        ErrorCode::E0206,
                        r.path.clone(),
                        *span,
                        format!("limit `where` references unknown field `{wkey}`"),
                    )
                    .with_suggestion("use a declared field name", None)
                    .with_root_id(format!("E0206:{}:where:{}", r.name, wkey)),
                );
            } else if !filterable.contains(wkey.as_str()) {
                report.push(
                    Diagnostic::error(
                        ErrorCode::E0206,
                        r.path.clone(),
                        *span,
                        format!("field `{wkey}` is used in `where` but is not filterable"),
                    )
                    .with_suggestion(
                        "mark the field filterable".to_string(),
                        Some(format!("{wkey}: field.bool({{ filterable: true }})")),
                    )
                    .with_root_id(format!("E0206:{}:filter:{}", r.name, wkey)),
                );
            }
        }
    }

    // Action files + optional per-action access field refs
    for action in &r.actions {
        let action_path = resolve_action_path(&r.path, &action.run);
        if !action_path.is_file() {
            report.push(
                Diagnostic::error(
                    ErrorCode::E0207,
                    r.path.clone(),
                    action.run_span,
                    format!("action file not found: `{}`", action.run),
                )
                .with_label(action.run_span, "no such file")
                .with_suggestion(
                    format!("create {} or fix the path", action_path.display()),
                    Some(format!("run: \"./{}.action.ts\"", action.name)),
                )
                .with_root_id(format!("E0207:{}:{}", r.name, action.name)),
            );
        }
        if let Some(rule) = &action.access {
            match rule {
                RuleIr::Owner { field, span } if !field_names.contains(field.as_str()) => {
                    let suggestion = closest(field, field_names.iter().copied());
                    let mut d = Diagnostic::error(
                        ErrorCode::E0210,
                        r.path.clone(),
                        *span,
                        format!(
                            "action `{}` access rule references unknown field `{field}`",
                            action.name
                        ),
                    )
                    .with_root_id(format!(
                        "E0210:{}:action:{}:{}",
                        r.name, action.name, field
                    ));
                    if let Some(s) = suggestion {
                        d = d.with_suggestion(
                            format!("did you mean `{s}`?"),
                            Some(format!("access: rule.owner(\"{s}\")")),
                        );
                    }
                    report.push(d);
                }
                RuleIr::Member {
                    via,
                    on,
                    user_field,
                    span,
                } => {
                    validate_member_join(
                        r,
                        &format!("action:{}", action.name),
                        via,
                        on,
                        user_field,
                        *span,
                        by_name,
                        report,
                    );
                }
                _ => {}
            }
        }
    }

    // Option sanity: max only meaningful on text (warn as error for bad types already caught).
    // Sortable/filterable on id without declaration is fine; no extra check.
}

fn to_resource_ir(r: ParsedResource) -> ResourceIr {
    ResourceIr {
        name: r.name,
        export_name: r.export_name,
        path: r.path,
        span: r.span,
        fields: r.fields,
        constraints: r.constraints,
        access: r.access.expect("validated"),
        actions: r.actions,
    }
}

/// Fill empty `on` / `user_field` with conventions (ADR-047).
fn resolve_member_defaults(mut r: ResourceIr) -> ResourceIr {
    let resource_name = r.name.clone();
    let fix = |rule: &mut RuleIr| {
        if let RuleIr::Member {
            on, user_field, ..
        } = rule
        {
            if on.is_empty() {
                *on = resource_name.clone();
            }
            if user_field.is_empty() {
                *user_field = "user".into();
            }
        }
    };
    for op in &mut r.access.operations {
        fix(&mut op.rule);
    }
    for action in &mut r.actions {
        if let Some(rule) = &mut action.access {
            fix(rule);
        }
    }
    r
}

fn member_on_user<'a>(
    resource_name: &str,
    on: &'a str,
    user_field: &'a str,
) -> (String, String) {
    let on = if on.is_empty() {
        resource_name.to_string()
    } else {
        on.to_string()
    };
    let user_field = if user_field.is_empty() {
        "user".into()
    } else {
        user_field.to_string()
    };
    (on, user_field)
}

fn validate_member_join(
    r: &ParsedResource,
    op_label: &str,
    via: &str,
    on: &str,
    user_field: &str,
    span: Span,
    by_name: &BTreeMap<String, &ParsedResource>,
    report: &mut Report,
) {
    let Some(join) = by_name.get(via) else {
        let suggestion = closest(via, by_name.keys().map(|s| s.as_str()));
        let mut d = Diagnostic::error(
            ErrorCode::E0213,
            r.path.clone(),
            span,
            format!("`rule.member` join resource `{via}` is not declared"),
        )
        .with_root_id(format!("E0213:{}:{}:via:{}", r.name, op_label, via));
        if let Some(s) = suggestion {
            d = d.with_suggestion(
                format!("did you mean `{s}`?"),
                Some(format!("rule.member(\"{s}\")")),
            );
        } else {
            d = d.with_suggestion(
                "declare a join resource with refs to this resource and `user`",
                None,
            );
        }
        report.push(d);
        return;
    };

    let (on, user_field) = member_on_user(&r.name, on, user_field);
    let join_fields: BTreeMap<&str, &puddly_ir::FieldIr> =
        join.fields.iter().map(|f| (f.name.as_str(), f)).collect();

    match join_fields.get(on.as_str()) {
        None => {
            report.push(
                Diagnostic::error(
                    ErrorCode::E0213,
                    r.path.clone(),
                    span,
                    format!(
                        "`rule.member` column `{on}` not found on join resource `{via}`"
                    ),
                )
                .with_suggestion(
                    format!("add `{on}: field.ref(\"{}\")` on `{via}`", r.name),
                    None,
                )
                .with_root_id(format!("E0213:{}:{}:on:{}", r.name, op_label, on)),
            );
        }
        Some(f) => {
            let ok = match &f.kind {
                FieldKind::Ref { target } => target == &r.name,
                FieldKind::Id => false,
                _ => true, // uuid/text columns allowed but refs preferred
            };
            if let FieldKind::Ref { target } = &f.kind {
                if target != &r.name {
                    report.push(
                        Diagnostic::error(
                            ErrorCode::E0213,
                            r.path.clone(),
                            span,
                            format!(
                                "`rule.member` column `{on}` on `{via}` refs `{target}`, expected `{}`",
                                r.name
                            ),
                        )
                        .with_root_id(format!("E0213:{}:{}:on-target:{}", r.name, op_label, on)),
                    );
                }
            }
            let _ = ok;
        }
    }

    match join_fields.get(user_field.as_str()) {
        None => {
            report.push(
                Diagnostic::error(
                    ErrorCode::E0213,
                    r.path.clone(),
                    span,
                    format!(
                        "`rule.member` column `{user_field}` not found on join resource `{via}`"
                    ),
                )
                .with_suggestion(
                    format!("add `{user_field}: field.ref(\"user\")` on `{via}`"),
                    None,
                )
                .with_root_id(format!(
                    "E0213:{}:{}:user:{}",
                    r.name, op_label, user_field
                )),
            );
        }
        Some(f) => {
            if let FieldKind::Ref { target } = &f.kind {
                if target != "user" {
                    report.push(
                        Diagnostic::error(
                            ErrorCode::E0213,
                            r.path.clone(),
                            span,
                            format!(
                                "`rule.member` column `{user_field}` on `{via}` refs `{target}`, expected `user`"
                            ),
                        )
                        .with_root_id(format!(
                            "E0213:{}:{}:user-target:{}",
                            r.name, op_label, user_field
                        )),
                    );
                }
            }
        }
    }
}

fn resolve_action_path(resource_path: &Path, run: &str) -> PathBuf {
    let parent = resource_path.parent().unwrap_or_else(|| Path::new("."));
    parent.join(run)
}

fn closest<'a>(target: &str, candidates: impl Iterator<Item = &'a str>) -> Option<String> {
    let mut best: Option<(usize, &str)> = None;
    for c in candidates {
        let d = edit_distance(target, c);
        if d > 0 && d <= 2 {
            match best {
                None => best = Some((d, c)),
                Some((bd, _)) if d < bd => best = Some((d, c)),
                Some((bd, bc)) if d == bd && c < bc => best = Some((d, c)),
                _ => {}
            }
        }
    }
    best.map(|(_, s)| s.to_string())
}

fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0; b.len() + 1];
    for i in 1..=a.len() {
        cur[0] = i;
        for j in 1..=b.len() {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            cur[j] = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}
