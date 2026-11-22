// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

//! Listing released chart versions from the OCI registry (`--list-versions`),
//! like `cilium install --list-versions`. ghcr.io hands out an anonymous pull
//! token, so no login is needed for public charts.

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::cmp::Ordering;
use std::time::Duration;

const REGISTRY_HOST: &str = "ghcr.io";
/// Repository path of the published chart (see chart::OCI_CHART).
const REPOSITORY: &str = "zyvorai/charts/paqtra";

#[derive(Deserialize)]
struct Token {
    token: String,
}

#[derive(Deserialize)]
struct Tags {
    tags: Option<Vec<String>>,
}

fn get_json<T: for<'de> Deserialize<'de>>(url: &str, bearer: Option<&str>) -> Result<T> {
    let mut req = ureq::get(url)
        .config()
        .timeout_global(Some(Duration::from_secs(20)))
        .build();
    if let Some(t) = bearer {
        req = req.header("Authorization", format!("Bearer {t}"));
    }
    let mut resp = req
        .call()
        .with_context(|| format!("request failed: {url}"))?;
    let body = resp.body_mut().read_to_string().context("bad response")?;
    serde_json::from_str(&body).with_context(|| format!("unexpected response from {url}"))
}

/// All version tags of the published chart, newest first.
pub async fn list_versions() -> Result<Vec<String>> {
    tokio::task::spawn_blocking(|| -> Result<Vec<String>> {
        let token: Token = get_json(
            &format!("https://{REGISTRY_HOST}/token?service={REGISTRY_HOST}&scope=repository:{REPOSITORY}:pull"),
            None,
        )
        .context("cannot get a registry token")?;
        let tags: Tags = get_json(
            &format!("https://{REGISTRY_HOST}/v2/{REPOSITORY}/tags/list?n=1000"),
            Some(&token.token),
        )
        .context("cannot list chart versions (has a release been published, and is the package public?)")?;
        let versions = sort_versions(tags.tags.unwrap_or_default());
        if versions.is_empty() {
            bail!("no released chart versions found in oci://{REGISTRY_HOST}/{REPOSITORY}");
        }
        Ok(versions)
    })
    .await?
}

/// `1.2.3` or `1.2.3-rc.1` -> (major, minor, patch, prerelease).
fn parse(v: &str) -> Option<(u64, u64, u64, Option<String>)> {
    let (core, pre) = match v.split_once('-') {
        Some((c, p)) => (c, Some(p.to_string())),
        None => (v, None),
    };
    let mut it = core.split('.');
    let major = it.next()?.parse().ok()?;
    let minor = it.next()?.parse().ok()?;
    let patch = it.next()?.parse().ok()?;
    if it.next().is_some() {
        return None;
    }
    Some((major, minor, patch, pre))
}

/// Keep only version tags (registries also hold `latest`, digests, signatures)
/// and sort newest first; a release outranks its own release candidates.
pub fn sort_versions(tags: Vec<String>) -> Vec<String> {
    let mut parsed: Vec<_> = tags
        .into_iter()
        .filter_map(|t| parse(&t).map(|p| (p, t)))
        .collect();
    parsed.sort_by(|(a, _), (b, _)| {
        (a.0, a.1, a.2)
            .cmp(&(b.0, b.1, b.2))
            .then_with(|| match (&a.3, &b.3) {
                (None, None) => Ordering::Equal,
                (None, Some(_)) => Ordering::Greater,
                (Some(_), None) => Ordering::Less,
                (Some(x), Some(y)) => x.cmp(y),
            })
            .reverse()
    });
    parsed.into_iter().map(|(_, t)| t).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn sorts_newest_first_and_drops_non_versions() {
        let out = sort_versions(s(&[
            "2.0.0",
            "latest",
            "2.10.0",
            "sha256-abc.sig",
            "2.9.1",
            "1.0.0",
        ]));
        assert_eq!(out, s(&["2.10.0", "2.9.1", "2.0.0", "1.0.0"]));
    }

    #[test]
    fn a_release_outranks_its_release_candidates() {
        let out = sort_versions(s(&["2.2.0-rc.1", "2.2.0", "2.2.0-rc.2", "2.1.9"]));
        assert_eq!(out, s(&["2.2.0", "2.2.0-rc.2", "2.2.0-rc.1", "2.1.9"]));
    }

    #[test]
    fn rejects_malformed_versions() {
        assert!(parse("1.2").is_none());
        assert!(parse("1.2.3.4").is_none());
        assert!(parse("v1.2.3").is_none());
        assert!(sort_versions(s(&["", "x.y.z", "1.2"])).is_empty());
    }
}
