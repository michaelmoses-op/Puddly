//! Lower Oxc TSX AST into restricted [`NodeIr`].

use crate::error::{ViewError, ViewResult};
use crate::ir::{Attr, AttrValue, NodeIr, PropPath};
use oxc_ast::ast::{
    ArrowFunctionBody, CallExpression, ExportDefaultDeclarationKind, Expression, FunctionBody,
    JSXAttributeItem, JSXAttributeName, JSXAttributeValue, JSXChild, JSXElement, JSXElementName,
    JSXExpression, Program, Statement,
};
use std::path::Path;

pub struct Lowered {
    pub root: NodeIr,
}

pub fn lower_default_export(program: &Program<'_>, path: &Path) -> ViewResult<Lowered> {
    let mut found = None;
    for stmt in &program.body {
        let Statement::ExportDefaultDeclaration(export) = stmt else {
            continue;
        };
        match &export.declaration {
            ExportDefaultDeclarationKind::FunctionDeclaration(func) => {
                let body = func
                    .body
                    .as_ref()
                    .ok_or_else(|| dialect(path, "default export function needs a body"))?;
                found = Some(lower_function_body(body, path)?);
            }
            ExportDefaultDeclarationKind::FunctionExpression(func) => {
                let body = func
                    .body
                    .as_ref()
                    .ok_or_else(|| dialect(path, "default export function needs a body"))?;
                found = Some(lower_function_body(body, path)?);
            }
            ExportDefaultDeclarationKind::ArrowFunctionExpression(arrow) => {
                found = Some(match &arrow.body {
                    ArrowFunctionBody::FunctionBody(body) => lower_function_body(body, path)?,
                    other => {
                        let expr = other.as_expression().ok_or_else(|| {
                            dialect(path, "arrow default export must return JSX")
                        })?;
                        lower_jsx_expr(expr, path)?
                    }
                });
            }
            _ => {
                return Err(dialect(
                    path,
                    "unsupported default export (expected `export default function`)",
                ));
            }
        }
    }
    let root = found.ok_or_else(|| dialect(path, "missing `export default function`"))?;
    Ok(Lowered { root })
}

fn lower_function_body(body: &FunctionBody<'_>, path: &Path) -> ViewResult<NodeIr> {
    let mut returned = None;
    for stmt in &body.statements {
        match stmt {
            Statement::ReturnStatement(ret) => {
                let Some(arg) = &ret.argument else {
                    return Err(dialect(path, "return must yield JSX"));
                };
                if returned.is_some() {
                    return Err(dialect(path, "only one return is allowed"));
                }
                returned = Some(lower_jsx_expr(arg, path)?);
            }
            Statement::EmptyStatement(_) => {}
            _ => {
                return Err(dialect(
                    path,
                    "only `return <jsx>` is allowed in view/island bodies",
                ));
            }
        }
    }
    returned.ok_or_else(|| dialect(path, "function body must `return` JSX"))
}

fn lower_jsx_expr(expr: &Expression<'_>, path: &Path) -> ViewResult<NodeIr> {
    match expr {
        Expression::JSXElement(el) => lower_element(el, path),
        Expression::JSXFragment(frag) => {
            let mut children = Vec::new();
            for child in &frag.children {
                if let Some(n) = lower_child(child, path)? {
                    children.push(n);
                }
            }
            Ok(NodeIr::Element {
                tag: "div".into(),
                attrs: Vec::new(),
                children,
                self_closing: false,
            })
        }
        Expression::ParenthesizedExpression(paren) => lower_jsx_expr(&paren.expression, path),
        _ => Err(dialect(path, "expected JSX element as return value")),
    }
}

fn lower_element(el: &JSXElement<'_>, path: &Path) -> ViewResult<NodeIr> {
    let tag = element_name(&el.opening_element.name, path)?;
    let attrs = lower_attrs(&el.opening_element.attributes, path)?;
    let self_closing = el.closing_element.is_none();

    if is_pascal_case(&tag) {
        return Ok(NodeIr::Island {
            name: tag,
            props: attrs,
        });
    }

    let mut children = Vec::new();
    for child in &el.children {
        if let Some(n) = lower_child(child, path)? {
            children.push(n);
        }
    }
    Ok(NodeIr::Element {
        tag,
        attrs,
        children,
        self_closing,
    })
}

fn lower_child(child: &JSXChild<'_>, path: &Path) -> ViewResult<Option<NodeIr>> {
    match child {
        JSXChild::Text(t) => {
            let raw = t.value.as_str();
            if raw.chars().all(|c| c.is_whitespace()) {
                if raw.contains('\n') || raw.is_empty() {
                    return Ok(None);
                }
            }
            Ok(Some(NodeIr::Text(raw.to_string())))
        }
        JSXChild::Element(el) => Ok(Some(lower_element(el, path)?)),
        JSXChild::ExpressionContainer(container) => match &container.expression {
            JSXExpression::EmptyExpression(_) => Ok(None),
            JSXExpression::StringLiteral(lit) => Ok(Some(NodeIr::Text(lit.value.to_string()))),
            other => {
                let expr = other
                    .as_expression()
                    .ok_or_else(|| dialect(path, "unsupported JSX expression"))?;
                let path_expr = expr_to_prop_path(expr, path)?;
                let puddly_id = path_expr
                    .segments
                    .last()
                    .cloned()
                    .unwrap_or_else(|| "expr".into());
                Ok(Some(NodeIr::Expr {
                    path: path_expr,
                    puddly_id,
                }))
            }
        },
        JSXChild::Fragment(frag) => {
            let mut children = Vec::new();
            for c in &frag.children {
                if let Some(n) = lower_child(c, path)? {
                    children.push(n);
                }
            }
            Ok(Some(NodeIr::Element {
                tag: "div".into(),
                attrs: Vec::new(),
                children,
                self_closing: false,
            }))
        }
        JSXChild::Spread(_) => Err(dialect(path, "JSX spread children are not supported")),
    }
}

fn lower_attrs(attrs: &[JSXAttributeItem<'_>], path: &Path) -> ViewResult<Vec<Attr>> {
    let mut out = Vec::new();
    for item in attrs {
        match item {
            JSXAttributeItem::Attribute(attr) => {
                let name = match &attr.name {
                    JSXAttributeName::Identifier(id) => id.name.to_string(),
                    JSXAttributeName::NamespacedName(ns) => {
                        format!("{}:{}", ns.namespace.name, ns.name.name)
                    }
                };
                let name = if name == "className" {
                    "class".into()
                } else {
                    name
                };
                let value = match &attr.value {
                    None => AttrValue::String(String::new()),
                    Some(JSXAttributeValue::StringLiteral(lit)) => {
                        AttrValue::String(lit.value.to_string())
                    }
                    Some(JSXAttributeValue::ExpressionContainer(container)) => {
                        match &container.expression {
                            JSXExpression::EmptyExpression(_) => {
                                return Err(dialect(path, "empty attribute expression"));
                            }
                            other => {
                                let expr = other.as_expression().ok_or_else(|| {
                                    dialect(path, "unsupported attribute expression")
                                })?;
                                AttrValue::Expr(expr_to_prop_path(expr, path)?)
                            }
                        }
                    }
                    Some(JSXAttributeValue::Element(_))
                    | Some(JSXAttributeValue::Fragment(_)) => {
                        return Err(dialect(
                            path,
                            "JSX element attribute values are not supported",
                        ));
                    }
                };
                out.push(Attr { name, value });
            }
            JSXAttributeItem::SpreadAttribute(_) => {
                return Err(dialect(path, "spread attributes are not supported"));
            }
        }
    }
    Ok(out)
}

fn element_name(name: &JSXElementName<'_>, path: &Path) -> ViewResult<String> {
    match name {
        JSXElementName::Identifier(id) => Ok(id.name.to_string()),
        JSXElementName::IdentifierReference(id) => Ok(id.name.to_string()),
        JSXElementName::NamespacedName(ns) => Ok(format!("{}:{}", ns.namespace.name, ns.name.name)),
        JSXElementName::MemberExpression(_) => {
            Err(dialect(path, "member JSX tags are not supported"))
        }
        JSXElementName::ThisExpression(_) => Err(dialect(path, "`<this>` is not supported")),
    }
}

fn expr_to_prop_path(expr: &Expression<'_>, path: &Path) -> ViewResult<PropPath> {
    match expr {
        Expression::Identifier(id) => Ok(PropPath::single(id.name.as_str())),
        Expression::StaticMemberExpression(mem) => {
            let mut segments = Vec::new();
            collect_member(&mem.object, &mut segments, path)?;
            segments.push(mem.property.name.to_string());
            if segments.first().map(|s| s.as_str()) == Some("props") {
                segments.remove(0);
            }
            if segments.is_empty() {
                return Err(dialect(path, "empty props path"));
            }
            Ok(PropPath { segments })
        }
        Expression::ParenthesizedExpression(p) => expr_to_prop_path(&p.expression, path),
        Expression::CallExpression(call) => {
            if is_ident_call(call, "String") {
                if let Some(arg) = call.arguments.first().and_then(|a| a.as_expression()) {
                    return expr_to_prop_path(arg, path);
                }
            }
            Err(dialect(
                path,
                "only `{ident}`, `{props.field}`, or `{String(props.field)}` expressions are supported",
            ))
        }
        _ => Err(dialect(
            path,
            "only `{ident}` / `{props.field}` expressions are supported",
        )),
    }
}

fn collect_member(expr: &Expression<'_>, out: &mut Vec<String>, path: &Path) -> ViewResult<()> {
    match expr {
        Expression::Identifier(id) => {
            out.push(id.name.to_string());
            Ok(())
        }
        Expression::StaticMemberExpression(mem) => {
            collect_member(&mem.object, out, path)?;
            out.push(mem.property.name.to_string());
            Ok(())
        }
        _ => Err(dialect(path, "unsupported member expression")),
    }
}

fn is_ident_call(call: &CallExpression<'_>, name: &str) -> bool {
    matches!(&call.callee, Expression::Identifier(id) if id.name == name)
}

fn is_pascal_case(tag: &str) -> bool {
    tag.chars().next().is_some_and(|c| c.is_ascii_uppercase())
}

fn dialect(path: &Path, message: impl Into<String>) -> ViewError {
    ViewError::Dialect {
        path: path.to_path_buf(),
        message: message.into(),
    }
}
