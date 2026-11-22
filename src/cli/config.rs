// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

//! `paqtra config view|get|set`: the release's Helm values.

use anyhow::{bail, Result};
use regex::Regex;
use std::sync::LazyLock;

use super::global::Global;
use super::helm;
use super::install::{cmd_upgrade, InstallOpts};

static ASSIGNMENT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[A-Za-z0-9_.\[\]-]+=.*$").unwrap());

/// `api.env.hubbleMode` in a values document.
pub fn lookup_key<'a>(root: &'a serde_json::Value, key: &str) -> Option<&'a serde_json::Value> {
    key.split('.').try_fold(root, |v, part| v.get(part))
}

/// Each `set` argument must be `key=value`, like Helm's `--set`.
pub fn validate_assignments(items: &[String]) -> Result<()> {
    if items.is_empty() {
        bail!("give at least one key=value, e.g. `paqtra config set api.env.hubbleMode=auto`");
    }
    for i in items {
        if !ASSIGNMENT_RE.is_match(i) {
            bail!("{i:?} is not key=value (keys use letters, digits, `.`, `-`, `_`)");
        }
    }
    Ok(())
}

pub async fn cmd_view(g: &Global, all: bool) -> Result<()> {
    let helm = helm::ensure(g).await?;
    let mut args: Vec<String> = vec![
        "get".into(),
        "values".into(),
        g.release.clone(),
        "--namespace".into(),
        g.namespace.clone(),
        "--output".into(),
        "yaml".into(),
    ];
    if all {
        args.push("--all".into());
    }
    print!("{}", helm.run_ok(&args, "helm get values").await?);
    Ok(())
}

pub async fn cmd_get(g: &Global, key: &str) -> Result<()> {
    let helm = helm::ensure(g).await?;
    let args: Vec<String> = vec![
        "get".into(),
        "values".into(),
        g.release.clone(),
        "--namespace".into(),
        g.namespace.clone(),
        "--all".into(),
        "--output".into(),
        "json".into(),
    ];
    let json = helm.run_ok(&args, "helm get values").await?;
    let root: serde_json::Value = serde_json::from_str(&json)?;
    match lookup_key(&root, key) {
        Some(serde_json::Value::String(s)) => println!("{s}"),
        Some(v) => println!("{v}"),
        None => bail!("no value for {key:?} (see `paqtra config view --all`)"),
    }
    Ok(())
}

pub async fn cmd_set(g: &Global, assignments: Vec<String>, no_wait: bool) -> Result<()> {
    validate_assignments(&assignments)?;
    // A values change is an upgrade that keeps everything else (and the chart
    // this CLI carries).
    cmd_upgrade(
        g,
        InstallOpts {
            set: assignments,
            wait: !no_wait,
            ..InstallOpts::default()
        },
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn looks_up_dotted_keys() {
        let v = json!({"api": {"env": {"hubbleMode": "grpc"}, "replicas": 2}, "x": null});
        assert_eq!(lookup_key(&v, "api.env.hubbleMode"), Some(&json!("grpc")));
        assert_eq!(lookup_key(&v, "api.replicas"), Some(&json!(2)));
        assert_eq!(
            lookup_key(&v, "api.env"),
            Some(&json!({"hubbleMode": "grpc"}))
        );
        assert_eq!(lookup_key(&v, "api.nope"), None);
        assert_eq!(lookup_key(&v, "nope.deeper"), None);
        assert_eq!(lookup_key(&v, "x"), Some(&serde_json::Value::Null));
    }

    #[test]
    fn assignments_must_be_key_value() {
        assert!(
            validate_assignments(&["api.env.hubbleMode=auto".into(), "ui.replicas=2".into()])
                .is_ok()
        );
        assert!(validate_assignments(&["a.b[0].c=v=with=equals".into()]).is_ok());
        assert!(validate_assignments(&[]).is_err());
        for bad in ["novalue", "=x", "a b=c", "--set x=y", "a;b=c"] {
            assert!(validate_assignments(&[bad.to_string()]).is_err(), "{bad:?}");
        }
    }
}
