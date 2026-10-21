//! Generate OpenAPI 3.0 JSON for the opt-in REST surface (ADR-008 / ADR-045 / Step 27).

use crate::rest::REST_API_PREFIX;
use puddly_core::DISPLAY_NAME;
use puddly_ir::{AppIr, FieldKind, ResourceIr};
use serde_json::{json, Map, Value};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Relative path under the app root (gitignored via `.puddly/generated/`).
pub const GENERATED_OPENAPI_JSON: &str = ".puddly/generated/openapi.json";

/// OpenAPI 3 document as JSON for `ir`.
pub fn generate_openapi(ir: &AppIr) -> Value {
    let mut paths = Map::new();

    // Auth (always documented — built-in RPC ops).
    paths.insert(
        format!("{REST_API_PREFIX}/auth/login"),
        json!({
            "post": {
                "operationId": "auth.login",
                "summary": "Login",
                "tags": ["auth"],
                "requestBody": json_body_schema(json!({
                    "type": "object",
                    "properties": {
                        "email": { "type": "string" },
                        "password": { "type": "string" }
                    }
                })),
                "responses": { "200": { "description": "RPC envelope" } }
            }
        }),
    );
    paths.insert(
        format!("{REST_API_PREFIX}/auth/register"),
        json!({
            "post": {
                "operationId": "auth.register",
                "summary": "Register",
                "tags": ["auth"],
                "requestBody": json_body_schema(json!({
                    "type": "object",
                    "properties": {
                        "email": { "type": "string" },
                        "password": { "type": "string" },
                        "userId": { "type": "string" }
                    }
                })),
                "responses": { "200": { "description": "RPC envelope" } }
            }
        }),
    );
    paths.insert(
        format!("{REST_API_PREFIX}/auth/logout"),
        json!({
            "post": {
                "operationId": "auth.logout",
                "summary": "Logout",
                "tags": ["auth"],
                "responses": { "200": { "description": "RPC envelope" } }
            }
        }),
    );

    let mut schemas = Map::new();
    for r in &ir.resources {
        schemas.insert(format!("{}Fields", pascal(&r.name)), resource_schema(r));
        insert_resource_paths(&mut paths, r);
    }

    json!({
        "openapi": "3.0.3",
        "info": {
            "title": format!("{DISPLAY_NAME} REST API"),
            "version": "0.1.0",
            "description": "Opt-in REST surface beside primary RPC (POST /rpc/<resource>.<operation>). Mutations require header X-Puddly-CSRF; session cookie puddly_session."
        },
        "paths": Value::Object(paths),
        "components": {
            "schemas": Value::Object(schemas),
            "parameters": {
                "XPuddlyCsrf": {
                    "name": "X-Puddly-CSRF",
                    "in": "header",
                    "required": true,
                    "schema": { "type": "string" },
                    "description": "Presence CSRF header (any non-empty value)"
                }
            },
            "securitySchemes": {
                "cookieAuth": {
                    "type": "apiKey",
                    "in": "cookie",
                    "name": "puddly_session"
                }
            }
        }
    })
}

fn insert_resource_paths(paths: &mut Map<String, Value>, r: &ResourceIr) {
    let base = format!("{REST_API_PREFIX}/{}", r.name);
    let item = format!("{base}/{{id}}");
    let tag = r.name.as_str();
    let schema_ref = format!("#/components/schemas/{}Fields", pascal(&r.name));

    paths.insert(
        base.clone(),
        json!({
            "get": {
                "operationId": format!("{}.list", r.name),
                "summary": format!("List {}", r.name),
                "tags": [tag],
                "responses": { "200": { "description": "RPC envelope with items" } }
            },
            "post": {
                "operationId": format!("{}.create", r.name),
                "summary": format!("Create {}", r.name),
                "tags": [tag],
                "parameters": [{ "$ref": "#/components/parameters/XPuddlyCsrf" }],
                "requestBody": json_body_schema(json!({ "$ref": schema_ref })),
                "responses": { "200": { "description": "RPC envelope" } }
            }
        }),
    );

    paths.insert(
        item,
        json!({
            "get": {
                "operationId": format!("{}.get", r.name),
                "summary": format!("Get {}", r.name),
                "tags": [tag],
                "parameters": [id_param()],
                "responses": { "200": { "description": "RPC envelope" } }
            },
            "patch": {
                "operationId": format!("{}.update", r.name),
                "summary": format!("Update {}", r.name),
                "tags": [tag],
                "parameters": [
                    id_param(),
                    { "$ref": "#/components/parameters/XPuddlyCsrf" }
                ],
                "requestBody": json_body_schema(json!({ "$ref": schema_ref })),
                "responses": { "200": { "description": "RPC envelope" } }
            },
            "delete": {
                "operationId": format!("{}.delete", r.name),
                "summary": format!("Delete {}", r.name),
                "tags": [tag],
                "parameters": [
                    id_param(),
                    { "$ref": "#/components/parameters/XPuddlyCsrf" }
                ],
                "responses": { "200": { "description": "RPC envelope" } }
            }
        }),
    );

    for action in &r.actions {
        let p = format!("{base}/{}", action.name);
        paths.insert(
            p,
            json!({
                "post": {
                    "operationId": format!("{}.{}", r.name, action.name),
                    "summary": format!("{} {}", r.name, action.name),
                    "tags": [tag],
                    "parameters": [{ "$ref": "#/components/parameters/XPuddlyCsrf" }],
                    "requestBody": json_body_schema(json!({
                        "type": "object",
                        "properties": { "id": { "type": "string", "format": "uuid" } }
                    })),
                    "responses": { "200": { "description": "RPC envelope" } }
                }
            }),
        );
    }
}

fn id_param() -> Value {
    json!({
        "name": "id",
        "in": "path",
        "required": true,
        "schema": { "type": "string", "format": "uuid" }
    })
}

fn json_body_schema(schema: Value) -> Value {
    json!({
        "required": true,
        "content": {
            "application/json": {
                "schema": schema
            }
        }
    })
}

fn resource_schema(r: &ResourceIr) -> Value {
    let mut props = Map::new();
    for f in &r.fields {
        if matches!(f.kind, FieldKind::Computed { .. }) {
            continue;
        }
        props.insert(f.name.clone(), field_schema(&f.kind));
    }
    json!({
        "type": "object",
        "properties": Value::Object(props)
    })
}

fn field_schema(kind: &FieldKind) -> Value {
    match kind {
        FieldKind::Bool => json!({ "type": "boolean" }),
        FieldKind::Id | FieldKind::Ref { .. } => {
            json!({ "type": "string", "format": "uuid" })
        }
        FieldKind::Date => json!({ "type": "string", "format": "date" }),
        FieldKind::Timestamp => json!({ "type": "string", "format": "date-time" }),
        FieldKind::Text => json!({ "type": "string" }),
        FieldKind::Computed { .. } => json!({}),
    }
}

fn pascal(name: &str) -> String {
    let mut chars = name.chars();
    match chars.next() {
        None => "Resource".into(),
        Some(c) => format!("{}{}", c.to_ascii_uppercase(), chars.as_str()),
    }
}

/// Write [`GENERATED_OPENAPI_JSON`] under `app_root`.
pub fn write_openapi(app_root: &Path, ir: &AppIr) -> io::Result<PathBuf> {
    let dest = app_root.join(GENERATED_OPENAPI_JSON);
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    let doc = generate_openapi(ir);
    let text = serde_json::to_string_pretty(&doc)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
    fs::write(&dest, text)?;
    Ok(dest)
}

