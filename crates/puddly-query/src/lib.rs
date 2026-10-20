//! Query library over a [`Manifest`] — CLI is a thin wrapper (ADR-005).
//!
//! - [`inspect`]: describe a resource, field, or action
//! - [`impact`]: list dependents of a resource or field

use puddly_manifest::{Manifest, ManifestField, ManifestResource};
use serde::{Deserialize, Serialize};

/// Target selector: `resource`, `resource.field`, or `resource.action:name`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryTarget {
    pub resource: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
}

impl QueryTarget {
    /// Parse `todo`, `todo.title`, or `todo.action:archive`.
    pub fn parse(s: &str) -> Result<Self, String> {
        let s = s.trim();
        if s.is_empty() {
            return Err("empty target".into());
        }
        if let Some((resource, rest)) = s.split_once('.') {
            if let Some(action) = rest.strip_prefix("action:") {
                if action.is_empty() {
                    return Err("action name required after `action:`".into());
                }
                return Ok(Self {
                    resource: resource.to_string(),
                    field: None,
                    action: Some(action.to_string()),
                });
            }
            if rest.is_empty() {
                return Err("field or action:name required after `.`".into());
            }
            Ok(Self {
                resource: resource.to_string(),
                field: Some(rest.to_string()),
                action: None,
            })
        } else {
            Ok(Self {
                resource: s.to_string(),
                field: None,
                action: None,
            })
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InspectResult {
    pub ok: bool,
    pub target: QueryTarget,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource: Option<ManifestResource>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field: Option<ManifestField>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<puddly_manifest::ManifestAction>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Describe a resource / field / action from the manifest.
pub fn inspect(manifest: &Manifest, target: &QueryTarget) -> InspectResult {
    let Some(resource) = manifest.resources.iter().find(|r| r.name == target.resource) else {
        return InspectResult {
            ok: false,
            target: target.clone(),
            resource: None,
            field: None,
            action: None,
            error: Some(format!("unknown resource `{}`", target.resource)),
        };
    };

    if let Some(field_name) = &target.field {
        let field = resource.fields.iter().find(|f| f.name == *field_name);
        return match field {
            Some(f) => InspectResult {
                ok: true,
                target: target.clone(),
                resource: Some(resource.clone()),
                field: Some(f.clone()),
                action: None,
                error: None,
            },
            None => InspectResult {
                ok: false,
                target: target.clone(),
                resource: Some(resource.clone()),
                field: None,
                action: None,
                error: Some(format!(
                    "resource `{}` has no field `{field_name}`",
                    target.resource
                )),
            },
        };
    }

    if let Some(action_name) = &target.action {
        let action = resource.actions.iter().find(|a| a.name == *action_name);
        return match action {
            Some(a) => InspectResult {
                ok: true,
                target: target.clone(),
                resource: Some(resource.clone()),
                field: None,
                action: Some(a.clone()),
                error: None,
            },
            None => InspectResult {
                ok: false,
                target: target.clone(),
                resource: Some(resource.clone()),
                field: None,
                action: None,
                error: Some(format!(
                    "resource `{}` has no action `{action_name}`",
                    target.resource
                )),
            },
        };
    }

    InspectResult {
        ok: true,
        target: target.clone(),
        resource: Some(resource.clone()),
        field: None,
        action: None,
        error: None,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImpactEdge {
    pub kind: String,
    pub from: String,
    pub to: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImpactResult {
    pub ok: bool,
    pub target: QueryTarget,
    pub edges: Vec<ImpactEdge>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// List what depends on a resource or field (inbound refs, access, constraints).
pub fn impact(manifest: &Manifest, target: &QueryTarget) -> ImpactResult {
    if target.action.is_some() {
        return ImpactResult {
            ok: false,
            target: target.clone(),
            edges: Vec::new(),
            error: Some("impact on actions is not supported yet; use a resource or field".into()),
        };
    }

    let Some(resource) = manifest.resources.iter().find(|r| r.name == target.resource) else {
        return ImpactResult {
            ok: false,
            target: target.clone(),
            edges: Vec::new(),
            error: Some(format!("unknown resource `{}`", target.resource)),
        };
    };

    let mut edges = Vec::new();

    if let Some(field_name) = &target.field {
        if !resource.fields.iter().any(|f| f.name == *field_name) {
            return ImpactResult {
                ok: false,
                target: target.clone(),
                edges: Vec::new(),
                error: Some(format!(
                    "resource `{}` has no field `{field_name}`",
                    target.resource
                )),
            };
        }
        // Access rules using this field
        for op in &resource.access {
            if op.rule.field.as_deref() == Some(field_name.as_str()) {
                edges.push(ImpactEdge {
                    kind: "access".into(),
                    from: format!("{}.{}", resource.name, field_name),
                    to: format!("{}.{}", resource.name, op.name),
                    detail: Some(format!("rule.{}", op.rule.kind)),
                });
            }
        }
        // Constraints using this field
        for c in &resource.constraints {
            if c.per.as_deref() == Some(field_name.as_str()) {
                edges.push(ImpactEdge {
                    kind: "constraint.per".into(),
                    from: format!("{}.{}", resource.name, field_name),
                    to: format!("{}.constraint:{}", resource.name, c.kind),
                    detail: None,
                });
            }
            for w in &c.where_fields {
                if w.field == *field_name {
                    edges.push(ImpactEdge {
                        kind: "constraint.where".into(),
                        from: format!("{}.{}", resource.name, field_name),
                        to: format!("{}.constraint:{}", resource.name, c.kind),
                        detail: None,
                    });
                }
            }
        }
        // Outbound ref from this field
        if let Some(f) = resource.fields.iter().find(|f| f.name == *field_name) {
            if let Some(t) = &f.target {
                edges.push(ImpactEdge {
                    kind: "ref".into(),
                    from: format!("{}.{}", resource.name, field_name),
                    to: t.clone(),
                    detail: None,
                });
            }
        }
    } else {
        // Resource-level: who references me?
        for name in &resource.referenced_by {
            edges.push(ImpactEdge {
                kind: "referencedBy".into(),
                from: name.clone(),
                to: resource.name.clone(),
                detail: None,
            });
        }
        // Outbound refs
        for t in &resource.refs {
            edges.push(ImpactEdge {
                kind: "ref".into(),
                from: resource.name.clone(),
                to: t.clone(),
                detail: None,
            });
        }
        // Actions on this resource
        for a in &resource.actions {
            edges.push(ImpactEdge {
                kind: "action".into(),
                from: resource.name.clone(),
                to: format!("{}.action:{}", resource.name, a.name),
                detail: Some(a.run.clone()),
            });
        }
    }

    edges.sort_by(|a, b| {
        (&a.kind, &a.from, &a.to, &a.detail).cmp(&(&b.kind, &b.from, &b.to, &b.detail))
    });

    ImpactResult {
        ok: true,
        target: target.clone(),
        edges,
        error: None,
    }
}

pub fn render_json<T: Serialize>(value: &T) -> String {
    let mut s = serde_json::to_string_pretty(value).expect("serialize");
    s.push('\n');
    s
}

pub fn format_inspect(result: &InspectResult, json: bool) -> String {
    if json {
        return render_json(result);
    }
    if !result.ok {
        return format!("error: {}\n", result.error.as_deref().unwrap_or("unknown"));
    }
    let mut out = String::new();
    if let Some(r) = &result.resource {
        out.push_str(&format!(
            "resource `{}` ({}) — {}\n",
            r.name, r.export_name, r.path
        ));
        if result.field.is_none() && result.action.is_none() {
            out.push_str(&format!("  fields: {}\n", field_names(r)));
            out.push_str(&format!("  refs → {:?}\n", r.refs));
            out.push_str(&format!("  ← referencedBy {:?}\n", r.referenced_by));
            out.push_str(&format!(
                "  actions: {:?}\n",
                r.actions.iter().map(|a| a.name.as_str()).collect::<Vec<_>>()
            ));
        }
    }
    if let Some(f) = &result.field {
        out.push_str(&format!("  field `{}`: kind={}\n", f.name, f.kind));
        if let Some(t) = &f.target {
            out.push_str(&format!("    ref → {t}\n"));
        }
    }
    if let Some(a) = &result.action {
        out.push_str(&format!("  action `{}`: run={}\n", a.name, a.run));
    }
    out
}

pub fn format_impact(result: &ImpactResult, json: bool) -> String {
    if json {
        return render_json(result);
    }
    if !result.ok {
        return format!("error: {}\n", result.error.as_deref().unwrap_or("unknown"));
    }
    if result.edges.is_empty() {
        return "no impact edges\n".into();
    }
    let mut out = String::new();
    for e in &result.edges {
        out.push_str(&format!("{}: {} → {}", e.kind, e.from, e.to));
        if let Some(d) = &e.detail {
            out.push_str(&format!(" ({d})"));
        }
        out.push('\n');
    }
    out
}

fn field_names(r: &ManifestResource) -> String {
    r.fields
        .iter()
        .map(|f| f.name.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}
