// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

//! Read-only BPF attachment inventory and brotherhood classification.
//!
//! Paqtra never attaches, detaches, or replaces programs. It only lists what
//! the kernel already exposes and labels owners: `cilium` (`cil_*`), `netra`
//! (`netra_*`), or `other`.

use serde::{Deserialize, Serialize};
use std::process::Command;

/// Owner class for a BPF program name (kernel may truncate to 15 chars).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BpfOwner {
    Cilium,
    Netra,
    Other,
}

impl BpfOwner {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Cilium => "cilium",
            Self::Netra => "netra",
            Self::Other => "other",
        }
    }
}

/// Classify by program name prefix (full or truncated to 15 chars).
pub fn classify_owner(name: &str) -> BpfOwner {
    let n = name.trim();
    // Cilium classic names: cil_*, bpf_*, and common truncated forms
    if n.starts_with("cil_")
        || n.starts_with("cilium")
        || n.starts_with("cil_from")
        || n.starts_with("cil_to")
        || n.starts_with("cil_over")
        || n.starts_with("tail_") && n.contains("cil")
    {
        return BpfOwner::Cilium;
    }
    if n.starts_with("netra_") || n.starts_with("netra") {
        return BpfOwner::Netra;
    }
    BpfOwner::Other
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Attachment {
    pub id: u32,
    pub name: String,
    pub owner: BpfOwner,
    pub prog_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttachmentInventory {
    pub node: String,
    pub source: String,
    pub attachments: Vec<Attachment>,
    pub total: usize,
    pub truncated: bool,
    pub cilium: usize,
    pub netra: usize,
    pub other: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriftFinding {
    pub kind: String,
    pub severity: String,
    pub message: String,
}

const MAX_ATTACHMENTS: usize = 200;

/// Build inventory from bpftool JSON or empty with unavailable source.
pub fn collect_inventory() -> AttachmentInventory {
    let node = std::env::var("NODE_NAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "local".into());

    if let Some(mut inv) = inventory_from_bpftool(&node) {
        summarize(&mut inv);
        return inv;
    }

    let mut inv = AttachmentInventory {
        node,
        source: "unavailable".into(),
        attachments: Vec::new(),
        total: 0,
        truncated: false,
        cilium: 0,
        netra: 0,
        other: 0,
    };
    summarize(&mut inv);
    inv
}

fn inventory_from_bpftool(node: &str) -> Option<AttachmentInventory> {
    let output = Command::new("bpftool")
        .args(["prog", "list", "-j"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let v: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;
    let arr = v.as_array()?;
    let mut attachments = Vec::new();
    for item in arr {
        let id = item.get("id").and_then(|x| x.as_u64()).unwrap_or(0) as u32;
        let name = item
            .get("name")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();
        if name.is_empty() {
            continue;
        }
        let prog_type = item
            .get("type")
            .and_then(|x| x.as_str())
            .unwrap_or("unknown")
            .to_string();
        let tag = item
            .get("tag")
            .and_then(|x| x.as_str())
            .map(|s| s.to_string());
        attachments.push(Attachment {
            id,
            owner: classify_owner(&name),
            name,
            prog_type,
            tag,
        });
    }
    let total = attachments.len();
    let truncated = total > MAX_ATTACHMENTS;
    if truncated {
        attachments.truncate(MAX_ATTACHMENTS);
    }
    Some(AttachmentInventory {
        node: node.to_string(),
        source: "bpftool".into(),
        attachments,
        total,
        truncated,
        cilium: 0,
        netra: 0,
        other: 0,
    })
}

fn summarize(inv: &mut AttachmentInventory) {
    inv.cilium = inv
        .attachments
        .iter()
        .filter(|a| a.owner == BpfOwner::Cilium)
        .count();
    inv.netra = inv
        .attachments
        .iter()
        .filter(|a| a.owner == BpfOwner::Netra)
        .count();
    inv.other = inv
        .attachments
        .iter()
        .filter(|a| a.owner == BpfOwner::Other)
        .count();
}

/// Drift findings from Paqtra's brotherhood perspective (warn-only, never mutate).
pub fn drift_findings(inv: &AttachmentInventory) -> Vec<DriftFinding> {
    let mut out = Vec::new();
    if inv.source == "unavailable" {
        out.push(DriftFinding {
            kind: "bpf-inventory-unavailable".into(),
            severity: "info".into(),
            message: "BPF program inventory unavailable (bpftool missing or failed). Drift cannot be judged."
                .into(),
        });
        return out;
    }
    if inv.cilium == 0 {
        out.push(DriftFinding {
            kind: "bpf-cilium-missing".into(),
            severity: "warning".into(),
            message: "No cil_* programs visible — Cilium datapath may be absent on this node, or inventory is incomplete."
                .into(),
        });
    }
    if inv.truncated {
        out.push(DriftFinding {
            kind: "bpf-inventory-truncated".into(),
            severity: "info".into(),
            message: format!(
                "Inventory truncated at {} of {} programs — drift may be incomplete.",
                MAX_ATTACHMENTS, inv.total
            ),
        });
    }
    out
}

/// Feature catalog row for `paqtra features`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureRow {
    pub id: String,
    pub name: String,
    pub state: String,
    pub note: String,
}

/// Build a discovery catalog from local capabilities (observe-only).
pub fn feature_catalog() -> Vec<FeatureRow> {
    use crate::ebpf::capabilities::{BpfCapabilities, BpfCapability};

    let caps = BpfCapabilities::detect();
    let bpf_state = match caps.capability {
        BpfCapability::Full => "full",
        BpfCapability::ReadOnly => "read-only",
        BpfCapability::BpftoolOnly => "bpftool",
        BpfCapability::None => "unavailable",
    };
    let inv = collect_inventory();

    vec![
        FeatureRow {
            id: "brotherhood".into(),
            name: "Cilium brotherhood".into(),
            state: "on".into(),
            note: "Paqtra observes; never attaches or writes Cilium maps".into(),
        },
        FeatureRow {
            id: "bpf-capability".into(),
            name: "BPF access tier".into(),
            state: bpf_state.into(),
            note: caps
                .kernel_version
                .clone()
                .unwrap_or_else(|| "kernel unknown".into()),
        },
        FeatureRow {
            id: "bpf-fs".into(),
            name: "BPF filesystem".into(),
            state: if caps.bpf_fs_available { "on" } else { "off" }.into(),
            note: "/sys/fs/bpf".into(),
        },
        FeatureRow {
            id: "bpftool".into(),
            name: "bpftool".into(),
            state: if caps.bpftool_available { "on" } else { "off" }.into(),
            note: "used for read-only prog inventory".into(),
        },
        FeatureRow {
            id: "attachments".into(),
            name: "Attachment inventory".into(),
            state: if inv.source != "unavailable" {
                "on"
            } else {
                "degraded"
            }
            .into(),
            note: format!(
                "{} via {} (cilium={} netra={} other={})",
                inv.total, inv.source, inv.cilium, inv.netra, inv.other
            ),
        },
        FeatureRow {
            id: "hubble".into(),
            name: "Hubble flows".into(),
            state: "optional".into(),
            note: "API/UI enrich when hubble-relay is reachable".into(),
        },
        FeatureRow {
            id: "enforce".into(),
            name: "Datapath enforce".into(),
            state: "off".into(),
            note: "Owned by Cilium CNP — Paqtra does not enforce".into(),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_cilium_and_netra() {
        assert_eq!(classify_owner("cil_from_container"), BpfOwner::Cilium);
        assert_eq!(classify_owner("cil_to_netdev"), BpfOwner::Cilium);
        assert_eq!(classify_owner("netra_tcx_ingress"), BpfOwner::Netra);
        assert_eq!(classify_owner("netra_edge_ingr"), BpfOwner::Netra);
        assert_eq!(classify_owner("custom_filter"), BpfOwner::Other);
    }

    #[test]
    fn drift_unavailable() {
        let inv = AttachmentInventory {
            node: "n".into(),
            source: "unavailable".into(),
            attachments: vec![],
            total: 0,
            truncated: false,
            cilium: 0,
            netra: 0,
            other: 0,
        };
        let f = drift_findings(&inv);
        assert_eq!(f[0].kind, "bpf-inventory-unavailable");
    }

    #[test]
    fn drift_cilium_missing() {
        let inv = AttachmentInventory {
            node: "n".into(),
            source: "bpftool".into(),
            attachments: vec![],
            total: 0,
            truncated: false,
            cilium: 0,
            netra: 0,
            other: 0,
        };
        let f = drift_findings(&inv);
        assert!(f.iter().any(|x| x.kind == "bpf-cilium-missing"));
    }

    #[test]
    fn feature_catalog_nonempty() {
        let rows = feature_catalog();
        assert!(rows.iter().any(|r| r.id == "brotherhood"));
        assert!(rows.iter().any(|r| r.id == "enforce" && r.state == "off"));
    }
}
