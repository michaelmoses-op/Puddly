use serde::{Deserialize, Serialize};

/// Schema version for the committed manifest document.
pub const MANIFEST_VERSION: u32 = 1;

/// Committed app map — pure function of IR; stable field order via serde + sorted vecs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub version: u32,
    pub framework: String,
    /// Sorted by `name`.
    pub resources: Vec<ManifestResource>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManifestResource {
    pub name: String,
    pub export_name: String,
    /// Path relative to app root, forward slashes.
    pub path: String,
    pub fields: Vec<ManifestField>,
    pub constraints: Vec<ManifestConstraint>,
    pub access: Vec<ManifestAccessOp>,
    pub actions: Vec<ManifestAction>,
    /// Outbound `field.ref` targets, sorted unique.
    pub refs: Vec<String>,
    /// Resources that ref this one, sorted unique.
    pub referenced_by: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManifestField {
    pub name: String,
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub computed: Option<ComputedMeta>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub optional: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filterable: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sortable: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub renamed_from: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComputedMeta {
    /// Owner decision: computed expressions stay opaque in the manifest.
    pub opaque: bool,
    pub source_shape: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManifestConstraint {
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub per: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub where_fields: Vec<ManifestWhereEntry>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManifestWhereEntry {
    pub field: String,
    pub value: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManifestAccessOp {
    pub name: String,
    pub rule: ManifestRule,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManifestRule {
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    /// Join resource for `member` (ADR-047).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub via: Option<String>,
    /// Join column → protected resource id.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on: Option<String>,
    /// Join column → user id (`user` key in TS; `userField` in JSON).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_field: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManifestAction {
    pub name: String,
    pub run: String,
    pub input: Vec<ManifestField>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub access: Option<ManifestRule>,
}
