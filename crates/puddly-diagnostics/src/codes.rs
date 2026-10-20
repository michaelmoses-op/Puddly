use serde::{Deserialize, Serialize};
use std::fmt;

/// Stable diagnostic codes. Never reuse a retired code number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
#[serde(rename_all = "UPPERCASE")]
pub enum ErrorCode {
    /// Syntax / parse failure from the underlying TS parser.
    E0001,
    /// Disallowed construct in a `*.resource.ts` file.
    E0101,
    /// Missing required `access` block.
    E0201,
    /// Duplicate resource name across the app.
    E0202,
    /// Duplicate field name within a resource.
    E0203,
    /// `field.ref` target does not name a known resource.
    E0204,
    /// Field option has the wrong type or is invalid for the field kind.
    E0205,
    /// Constraint / where clause references an unknown or non-filterable field.
    E0206,
    /// Action `run` path does not resolve to an existing file.
    E0207,
    /// Unknown field builder or rule helper.
    E0208,
    /// Duplicate resource export / multiple resources in one file.
    E0209,
    /// Access rule references an unknown field.
    E0210,
    /// Reserved resource name (`auth` is built-in RPC).
    E0211,
    /// Documentation: `rule.member` equality placeholder (not a hard error by itself).
    E0212,
    /// I/O or project layout problem (not a resource-rule error).
    E0900,
}

impl ErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::E0001 => "E0001",
            Self::E0101 => "E0101",
            Self::E0201 => "E0201",
            Self::E0202 => "E0202",
            Self::E0203 => "E0203",
            Self::E0204 => "E0204",
            Self::E0205 => "E0205",
            Self::E0206 => "E0206",
            Self::E0207 => "E0207",
            Self::E0208 => "E0208",
            Self::E0209 => "E0209",
            Self::E0210 => "E0210",
            Self::E0211 => "E0211",
            Self::E0212 => "E0212",
            Self::E0900 => "E0900",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim().trim_start_matches("error[").trim_end_matches(']');
        ERROR_REGISTRY
            .iter()
            .find(|e| e.code.as_str().eq_ignore_ascii_case(s))
            .map(|e| e.code)
    }

    pub fn title(self) -> &'static str {
        explain(self).map(|e| e.title).unwrap_or("unknown error")
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Entry in the explain registry.
#[derive(Debug, Clone, Copy)]
pub struct ErrorDoc {
    pub code: ErrorCode,
    pub title: &'static str,
    pub explanation: &'static str,
    pub example_bad: &'static str,
    pub example_good: &'static str,
}

/// Full registry powering `puddly explain <code>`.
pub static ERROR_REGISTRY: &[ErrorDoc] = &[
    ErrorDoc {
        code: ErrorCode::E0001,
        title: "syntax error in resource file",
        explanation: "The TypeScript parser could not read this file. Fix the syntax, then re-run `puddly check`.",
        example_bad: "export const Todo = resource(\"todo\" {",
        example_good: "export const Todo = resource(\"todo\", {",
    },
    ErrorDoc {
        code: ErrorCode::E0101,
        title: "disallowed construct in resource file",
        explanation: "`*.resource.ts` files may only declare resources with the declarative vocabulary (resource, field, rule, limit). Move logic to an action file.",
        example_bad: "const x = 1;\nexport const Todo = resource(\"todo\", { ... })",
        example_good: "export const Todo = resource(\"todo\", {\n  fields: { id: field.id() },\n  access: { list: rule.authenticated(), create: rule.authenticated(), update: rule.authenticated(), delete: rule.authenticated() },\n})",
    },
    ErrorDoc {
        code: ErrorCode::E0201,
        title: "missing access block",
        explanation: "Every resource must declare an `access` object. Omitting it is a compile error.",
        example_bad: "export const Todo = resource(\"todo\", { fields: { id: field.id() } })",
        example_good: "export const Todo = resource(\"todo\", {\n  fields: { id: field.id() },\n  access: { list: rule.authenticated(), create: rule.authenticated(), update: rule.authenticated(), delete: rule.authenticated() },\n})",
    },
    ErrorDoc {
        code: ErrorCode::E0202,
        title: "duplicate resource name",
        explanation: "Resource names must be unique across the app. Rename one of the conflicting resources.",
        example_bad: "// a.resource.ts\nresource(\"todo\", { ... })\n// b.resource.ts\nresource(\"todo\", { ... })",
        example_good: "resource(\"todo\", { ... })\nresource(\"todoItem\", { ... })",
    },
    ErrorDoc {
        code: ErrorCode::E0203,
        title: "duplicate field name",
        explanation: "Field names must be unique within a resource.",
        example_bad: "fields: { title: field.text(), title: field.text() }",
        example_good: "fields: { title: field.text(), subtitle: field.text() }",
    },
    ErrorDoc {
        code: ErrorCode::E0204,
        title: "unknown reference target",
        explanation: "`field.ref(\"...\")` must name another resource declared in the app.",
        example_bad: "owner: field.ref(\"usr\")",
        example_good: "owner: field.ref(\"user\")",
    },
    ErrorDoc {
        code: ErrorCode::E0205,
        title: "invalid field option",
        explanation: "A field option has the wrong type or is not allowed on this field kind.",
        example_bad: "title: field.text({ max: \"200\" })",
        example_good: "title: field.text({ max: 200 })",
    },
    ErrorDoc {
        code: ErrorCode::E0206,
        title: "unknown or non-filterable field in constraint",
        explanation: "`limit` `where` / `per` keys must name fields on the resource. Filter predicates require `filterable: true`.",
        example_bad: "limit({ per: \"owner\", where: { missing: false }, max: 10 })",
        example_good: "limit({ per: \"owner\", where: { done: false }, max: 10 })",
    },
    ErrorDoc {
        code: ErrorCode::E0207,
        title: "action file not found",
        explanation: "Action `run` paths are resolved relative to the resource file and must exist.",
        example_bad: "archive: { input: { id: field.id() }, run: \"./missing.action.ts\" }",
        example_good: "archive: { input: { id: field.id() }, run: \"./archive.action.ts\" }",
    },
    ErrorDoc {
        code: ErrorCode::E0208,
        title: "unknown helper in resource file",
        explanation: "Only known field builders and rule helpers are allowed.",
        example_bad: "title: field.string()",
        example_good: "title: field.text()",
    },
    ErrorDoc {
        code: ErrorCode::E0209,
        title: "invalid resource file shape",
        explanation: "Each `*.resource.ts` must export exactly one `resource(...)` binding.",
        example_bad: "export const A = resource(\"a\", { ... })\nexport const B = resource(\"b\", { ... })",
        example_good: "export const Todo = resource(\"todo\", { ... })",
    },
    ErrorDoc {
        code: ErrorCode::E0210,
        title: "access rule references unknown field",
        explanation: "`rule.owner` / `rule.member` must name a field on the same resource.",
        example_bad: "update: rule.owner(\"owne\")",
        example_good: "update: rule.owner(\"owner\")",
    },
    ErrorDoc {
        code: ErrorCode::E0211,
        title: "reserved resource name",
        explanation: "The resource name `auth` is reserved for built-in RPC (`auth.login`, `auth.register`, `auth.logout`). Rename your resource.",
        example_bad: "export const Auth = resource(\"auth\", { ... })",
        example_good: "export const Account = resource(\"account\", { ... })",
    },
    ErrorDoc {
        code: ErrorCode::E0212,
        title: "rule.member is an equality placeholder",
        explanation: "`rule.member(field)` currently means the same as `rule.owner(field)`: the signed-in user's id must equal the row's field value. Multi-value membership via join tables is planned later. Prefer `rule.owner` when you mean single-owner equality; use `member` only when you intend to migrate to join-table membership.",
        example_bad: "// expecting array/org membership today\nlist: rule.member(\"members\")",
        example_good: "// single id field (current semantics)\nlist: rule.member(\"orgId\")\n// or explicit owner\nlist: rule.owner(\"owner\")",
    },
    ErrorDoc {
        code: ErrorCode::E0900,
        title: "project or I/O error",
        explanation: "Puddly could not read the project path or resource files.",
        example_bad: "puddly check ./does-not-exist",
        example_good: "puddly check ./examples/todo-slice",
    },
];

pub fn explain(code: ErrorCode) -> Option<&'static ErrorDoc> {
    ERROR_REGISTRY.iter().find(|e| e.code == code)
}

pub fn format_explain(code: ErrorCode) -> String {
    match explain(code) {
        None => format!("unknown error code: {code}\n"),
        Some(doc) => {
            let cli = puddly_core::CLI_NAME;
            format!(
                "{code}: {title}\n\n{explanation}\n\nIncorrect:\n```\n{bad}\n```\n\nCorrect:\n```\n{good}\n```\n\nSee also: `{cli} explain {code}`\n",
                code = doc.code,
                title = doc.title,
                explanation = doc.explanation,
                bad = doc.example_bad,
                good = doc.example_good,
            )
        }
    }
}
