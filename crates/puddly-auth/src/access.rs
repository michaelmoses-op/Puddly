//! Evaluate declarative access rules against an optional user + row values.

use crate::user::AuthUser;
use puddly_ir::RuleIr;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use subtle::ConstantTimeEq;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AccessDecision {
    Allow,
    Deny,
}

/// Row field values used by `owner` / `member` rules (stringly for Step 5).
#[derive(Debug, Clone, Default)]
pub struct RowValues {
    pub fields: BTreeMap<String, String>,
}

impl RowValues {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.fields.insert(key.into(), value.into());
        self
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.fields.get(key).map(|s| s.as_str())
    }
}

/// Evaluate a single IR rule.
pub fn check_rule(
    rule: &RuleIr,
    user: Option<&AuthUser>,
    row: Option<&RowValues>,
) -> AccessDecision {
    match rule {
        RuleIr::Authenticated { .. } => {
            if user.is_some() {
                AccessDecision::Allow
            } else {
                AccessDecision::Deny
            }
        }
        RuleIr::Owner { field, .. } => {
            let Some(user) = user else {
                return AccessDecision::Deny;
            };
            let Some(row) = row else {
                return AccessDecision::Deny;
            };
            let Some(owner_id) = row.get(field) else {
                return AccessDecision::Deny;
            };
            // Constant-time compare on UTF-8 bytes (same length fast-path via subtle).
            if constant_eq(user.id.as_bytes(), owner_id.as_bytes()) {
                AccessDecision::Allow
            } else {
                AccessDecision::Deny
            }
        }
        RuleIr::Role { role, .. } => {
            let Some(user) = user else {
                return AccessDecision::Deny;
            };
            if user.has_role(role) {
                AccessDecision::Allow
            } else {
                AccessDecision::Deny
            }
        }
        RuleIr::Member { field, .. } => {
            // Step 5: treat member like owner equality on a single id field.
            // Multi-value membership (arrays / join tables) lands with RPC.
            let Some(user) = user else {
                return AccessDecision::Deny;
            };
            let Some(row) = row else {
                return AccessDecision::Deny;
            };
            let Some(member_id) = row.get(field) else {
                return AccessDecision::Deny;
            };
            if constant_eq(user.id.as_bytes(), member_id.as_bytes()) {
                AccessDecision::Allow
            } else {
                AccessDecision::Deny
            }
        }
    }
}

/// All rules must allow (AND). Empty → Deny (fail closed).
pub fn check_rules(
    rules: &[RuleIr],
    user: Option<&AuthUser>,
    row: Option<&RowValues>,
) -> AccessDecision {
    if rules.is_empty() {
        return AccessDecision::Deny;
    }
    for rule in rules {
        if check_rule(rule, user, row) == AccessDecision::Deny {
            return AccessDecision::Deny;
        }
    }
    AccessDecision::Allow
}

fn constant_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        // Still touch both sides roughly equally for common id lengths.
        let _ = a.ct_eq(a);
        let _ = b.ct_eq(b);
        return false;
    }
    bool::from(a.ct_eq(b))
}
