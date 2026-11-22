// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

// Policy data model - canonical definition used across handlers and services
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Policy {
    pub id: String,
    pub name: String,
    pub namespace: String,
    pub created_at: String,
    pub status: String,
}

#[derive(Debug, Deserialize)]
pub struct CreatePolicyRequest {
    pub name: String,
    pub namespace: String,
    pub spec: Value,
}

/// Maximum serialized size for the spec field (100 KB).
const MAX_SPEC_SIZE: usize = 100 * 1024;
/// Maximum nesting depth for the spec JSON.
const MAX_SPEC_DEPTH: usize = 20;

impl CreatePolicyRequest {
    /// Validate the spec field: reject specs that are too large or too deeply nested.
    pub fn validate_spec(&self) -> Result<(), String> {
        let serialized = serde_json::to_string(&self.spec)
            .map_err(|e| format!("Failed to serialize spec: {}", e))?;
        if serialized.len() > MAX_SPEC_SIZE {
            return Err(format!(
                "Policy spec exceeds maximum size of {} bytes (actual: {} bytes)",
                MAX_SPEC_SIZE,
                serialized.len()
            ));
        }
        let depth = json_depth(&self.spec);
        if depth > MAX_SPEC_DEPTH {
            return Err(format!(
                "Policy spec exceeds maximum nesting depth of {} (actual: {})",
                MAX_SPEC_DEPTH, depth
            ));
        }
        Ok(())
    }
}

/// Compute the maximum nesting depth of a JSON value.
fn json_depth(value: &Value) -> usize {
    match value {
        Value::Array(arr) => 1 + arr.iter().map(json_depth).max().unwrap_or(0),
        Value::Object(map) => 1 + map.values().map(json_depth).max().unwrap_or(0),
        _ => 1,
    }
}

/// The four rule lists of a CiliumNetworkPolicy spec.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum RuleDirection {
    #[serde(rename = "ingress")]
    Ingress,
    #[serde(rename = "egress")]
    Egress,
    #[serde(rename = "ingressDeny")]
    IngressDeny,
    #[serde(rename = "egressDeny")]
    EgressDeny,
}

impl RuleDirection {
    pub const ALL: [RuleDirection; 4] = [
        RuleDirection::Ingress,
        RuleDirection::Egress,
        RuleDirection::IngressDeny,
        RuleDirection::EgressDeny,
    ];

    /// The spec key holding this direction's rules.
    pub fn key(self) -> &'static str {
        match self {
            RuleDirection::Ingress => "ingress",
            RuleDirection::Egress => "egress",
            RuleDirection::IngressDeny => "ingressDeny",
            RuleDirection::EgressDeny => "egressDeny",
        }
    }
}

/// Body of `POST /policies/{id}/rules`.
#[derive(Debug, Deserialize)]
pub struct AddRuleRequest {
    pub direction: RuleDirection,
    pub rule: Value,
    /// Reject with 409 if the policy changed since the caller read it.
    pub resource_version: Option<String>,
}

/// Body of `PUT /policies/{id}/rules`.
#[derive(Debug, Deserialize)]
pub struct EditRuleRequest {
    pub direction: RuleDirection,
    pub index: usize,
    pub rule: Value,
    pub resource_version: Option<String>,
}

/// Query of `DELETE /policies/{id}/rules`.
#[derive(Debug, Deserialize)]
pub struct DeleteRuleQuery {
    pub direction: RuleDirection,
    pub index: usize,
    pub resource_version: Option<String>,
}

/// Query shared by the rule endpoints.
#[derive(Debug, Default, Deserialize)]
pub struct DryRunQuery {
    #[serde(default)]
    pub dry_run: bool,
}

fn spec_object(spec: &mut Value) -> Result<&mut serde_json::Map<String, Value>, String> {
    let map = spec
        .as_object_mut()
        .ok_or_else(|| "policy spec must be an object".to_string())?;
    if map.contains_key("specs") {
        return Err(
            "policies using `specs` (a list of rules-with-selectors) cannot be edited rule by rule"
                .into(),
        );
    }
    Ok(map)
}

fn require_rule_object(rule: &Value) -> Result<(), String> {
    if rule.is_object() {
        Ok(())
    } else {
        Err("rule must be a JSON object".into())
    }
}

/// Total number of ingress/egress/deny rules in a spec.
pub fn rule_count(spec: &Value) -> usize {
    RuleDirection::ALL
        .iter()
        .filter_map(|d| spec.get(d.key()).and_then(Value::as_array))
        .map(Vec::len)
        .sum()
}

/// Per-direction rule counts, for API responses.
pub fn rule_counts(spec: &Value) -> Value {
    let mut out = serde_json::Map::new();
    for d in RuleDirection::ALL {
        let n = spec
            .get(d.key())
            .and_then(Value::as_array)
            .map_or(0, Vec::len);
        out.insert(d.key().to_string(), Value::from(n));
    }
    Value::Object(out)
}

/// Append a rule; returns its index within the direction's list.
pub fn add_rule(spec: &mut Value, direction: RuleDirection, rule: Value) -> Result<usize, String> {
    require_rule_object(&rule)?;
    let map = spec_object(spec)?;
    let list = map
        .entry(direction.key())
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
        .ok_or_else(|| format!("spec.{} is not a list", direction.key()))?;
    list.push(rule);
    Ok(list.len() - 1)
}

/// Replace the rule at `index`.
pub fn edit_rule(
    spec: &mut Value,
    direction: RuleDirection,
    index: usize,
    rule: Value,
) -> Result<(), String> {
    require_rule_object(&rule)?;
    let map = spec_object(spec)?;
    let list = map
        .get_mut(direction.key())
        .and_then(Value::as_array_mut)
        .ok_or_else(|| format!("policy has no {} rules", direction.key()))?;
    let len = list.len();
    let slot = list.get_mut(index).ok_or_else(|| {
        format!(
            "{} rule index {} out of range (policy has {})",
            direction.key(),
            index,
            len
        )
    })?;
    *slot = rule;
    Ok(())
}

/// Remove and return the rule at `index`. A policy's last rule cannot be
/// removed: Cilium's CRD validation requires at least one rule, so the apply
/// would be rejected. An emptied list is dropped from the spec.
pub fn delete_rule(
    spec: &mut Value,
    direction: RuleDirection,
    index: usize,
) -> Result<Value, String> {
    if rule_count(spec) <= 1 {
        return Err(
            "refusing to delete the policy's last rule: Cilium rejects a policy with no rules; delete the policy instead"
                .into(),
        );
    }
    let map = spec_object(spec)?;
    let list = map
        .get_mut(direction.key())
        .and_then(Value::as_array_mut)
        .ok_or_else(|| format!("policy has no {} rules", direction.key()))?;
    if index >= list.len() {
        return Err(format!(
            "{} rule index {} out of range (policy has {})",
            direction.key(),
            index,
            list.len()
        ));
    }
    let removed = list.remove(index);
    if list.is_empty() {
        map.remove(direction.key());
    }
    Ok(removed)
}

#[cfg(test)]
mod rule_tests {
    use super::*;
    use serde_json::json;

    fn spec() -> Value {
        json!({
            "endpointSelector": {"matchLabels": {"app": "db"}},
            "ingress": [
                {"fromEndpoints": [{"matchLabels": {"app": "api"}}]},
                {"fromCIDR": ["10.0.0.0/8"]}
            ]
        })
    }

    #[test]
    fn add_appends_and_creates_list() {
        let mut s = spec();
        assert_eq!(
            add_rule(
                &mut s,
                RuleDirection::Ingress,
                json!({"fromEntities": ["world"]})
            )
            .unwrap(),
            2
        );
        assert_eq!(
            add_rule(
                &mut s,
                RuleDirection::EgressDeny,
                json!({"toCIDR": ["1.1.1.1/32"]})
            )
            .unwrap(),
            0
        );
        assert_eq!(rule_count(&s), 4);
        assert_eq!(rule_counts(&s)["egressDeny"], 1);
    }

    #[test]
    fn add_rejects_non_object() {
        let mut s = spec();
        assert!(add_rule(&mut s, RuleDirection::Ingress, json!("nope")).is_err());
    }

    #[test]
    fn edit_replaces_and_checks_bounds() {
        let mut s = spec();
        edit_rule(
            &mut s,
            RuleDirection::Ingress,
            1,
            json!({"fromCIDR": ["192.168.0.0/16"]}),
        )
        .unwrap();
        assert_eq!(s["ingress"][1]["fromCIDR"][0], "192.168.0.0/16");
        assert!(edit_rule(&mut s, RuleDirection::Ingress, 5, json!({})).is_err());
        assert!(edit_rule(&mut s, RuleDirection::Egress, 0, json!({})).is_err());
    }

    #[test]
    fn delete_removes_and_drops_empty_list() {
        let mut s = spec();
        add_rule(
            &mut s,
            RuleDirection::Egress,
            json!({"toEntities": ["dns"]}),
        )
        .unwrap();
        let gone = delete_rule(&mut s, RuleDirection::Egress, 0).unwrap();
        assert_eq!(gone["toEntities"][0], "dns");
        assert!(s.get("egress").is_none());
        assert_eq!(rule_count(&s), 2);
    }

    #[test]
    fn the_last_rule_cannot_be_deleted() {
        let mut s = json!({"endpointSelector": {}, "ingress": [{"fromEntities": ["world"]}]});
        let err = delete_rule(&mut s, RuleDirection::Ingress, 0).unwrap_err();
        assert!(err.contains("delete the policy instead"), "{err}");
        assert_eq!(rule_count(&s), 1, "spec untouched on refusal");
    }

    #[test]
    fn delete_out_of_range() {
        let mut s = spec();
        assert!(delete_rule(&mut s, RuleDirection::Ingress, 9).is_err());
        assert_eq!(rule_count(&s), 2);
    }

    #[test]
    fn specs_lists_are_rejected() {
        let mut s = json!({"specs": [{"ingress": [{}]}]});
        assert!(add_rule(&mut s, RuleDirection::Ingress, json!({})).is_err());
    }
}
