// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use anyhow::Result;
use owo_colors::OwoColorize;

use crate::ebpf::attachments::{collect_inventory, drift_findings, feature_catalog};

use super::banner::print_banner;

/// `paqtra features` — discovery catalog (observe tiers, brotherhood).
pub fn cmd_features(output: &str) -> Result<()> {
    let rows = feature_catalog();
    if output == "json" {
        println!("{}", serde_json::to_string_pretty(&rows)?);
        return Ok(());
    }
    print_banner();
    println!("{}", "Features (observe-only)".bold());
    println!();
    for r in &rows {
        let state = format!("{:12}", r.state);
        let colored = if r.state == "on" || r.state == "full" {
            state.green().to_string()
        } else if r.state == "off" {
            state.dimmed().to_string()
        } else if r.state == "degraded" || r.state == "unavailable" {
            state.yellow().to_string()
        } else {
            state.cyan().to_string()
        };
        println!("  {:18} {} {}", r.name, colored, r.note.dimmed());
    }
    println!();
    println!(
        "{} See {}",
        "→".cyan(),
        "docs/cilium-brotherhood.md".dimmed()
    );
    Ok(())
}

/// `paqtra ebpf attachments` — read-only program inventory.
pub fn cmd_ebpf_attachments(output: &str) -> Result<()> {
    let inv = collect_inventory();
    if output == "json" {
        println!("{}", serde_json::to_string_pretty(&inv)?);
        return Ok(());
    }
    print_banner();
    println!(
        "{} node={} source={} total={} (cilium={} netra={} other={})",
        "Attachments".bold(),
        inv.node,
        inv.source,
        inv.total,
        inv.cilium,
        inv.netra,
        inv.other
    );
    if inv.truncated {
        println!(
            "{} inventory truncated at {}",
            "⚠".yellow(),
            inv.attachments.len()
        );
    }
    println!();
    for a in &inv.attachments {
        println!(
            "  {:>6}  {:8}  {:20}  {}",
            a.id,
            a.owner.as_str(),
            a.name,
            a.prog_type.dimmed()
        );
    }
    if inv.attachments.is_empty() {
        println!(
            "  {}",
            "(none — bpftool unavailable or no programs)".dimmed()
        );
    }
    Ok(())
}

/// `paqtra ebpf drift` — warn-only brotherhood drift findings.
pub fn cmd_ebpf_drift(output: &str) -> Result<()> {
    let inv = collect_inventory();
    let findings = drift_findings(&inv);
    if output == "json" {
        println!("{}", serde_json::to_string_pretty(&findings)?);
        return Ok(());
    }
    print_banner();
    println!("{}", "BPF drift (read-only)".bold());
    println!();
    if findings.is_empty() {
        println!("  {} No drift findings", "✔".green());
    } else {
        for f in &findings {
            let sev = format!("{:8}", f.severity);
            let colored = if f.severity == "warning" {
                sev.yellow().to_string()
            } else {
                sev.dimmed().to_string()
            };
            println!("  {} {} {}", colored, f.kind, f.message.dimmed());
        }
    }
    Ok(())
}
