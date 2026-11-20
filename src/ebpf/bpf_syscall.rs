// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

/// BPF Map Access via syscalls and bpftool
///
/// Provides BPF map reading through multiple methods:
/// 1. Direct bpftool command execution
/// 2. Reading from pinned maps (read-only, via bpftool)
use anyhow::{Context, Result};
use serde_json::Value;
use std::collections::HashMap;
use std::net::IpAddr;
use std::process::Command;

use super::bpf_parser::{
    parse_ct6_entry, parse_ct_entry, parse_ipcache_entry, parse_lb_entry, parse_metrics_entry,
    parse_policy_entry,
};
use super::{ConntrackEntry, DropReasonType, IPCacheEntry, LoadBalancerEntry, PolicyDecision};

/// BPF map accessor using bpftool
pub struct BpfToolReader {
    bpftool_path: String,
}

impl BpfToolReader {
    /// Create a new bpftool reader
    pub fn new() -> Result<Self> {
        // Try to find bpftool
        let bpftool_path = Self::find_bpftool()?;

        Ok(Self { bpftool_path })
    }

    /// Find bpftool binary
    fn find_bpftool() -> Result<String> {
        // Check common locations
        let paths = vec![
            "/usr/sbin/bpftool",
            "/usr/local/bin/bpftool",
            "/bin/bpftool",
        ];

        for path in paths {
            if std::path::Path::new(path).exists() {
                return Ok(path.to_string());
            }
        }

        // Try which command
        if let Ok(output) = Command::new("which").arg("bpftool").output() {
            if output.status.success() {
                if let Ok(path) = String::from_utf8(output.stdout) {
                    return Ok(path.trim().to_string());
                }
            }
        }

        anyhow::bail!("bpftool not found. Please install bpftool package.")
    }

    /// List all BPF maps
    pub fn list_maps(&self) -> Result<Vec<BpfMapInfo>> {
        let output = Command::new(&self.bpftool_path)
            .args(["map", "list", "-j"])
            .output()
            .context("Failed to execute bpftool map list")?;

        if !output.status.success() {
            anyhow::bail!("bpftool map list failed");
        }

        let json_str =
            String::from_utf8(output.stdout).context("Failed to parse bpftool output")?;

        let maps: Vec<Value> =
            serde_json::from_str(&json_str).context("Failed to parse bpftool JSON")?;

        let mut result = Vec::new();
        for map in maps {
            if let Some(name) = map.get("name").and_then(|v| v.as_str()) {
                let id = map.get("id").and_then(|v| v.as_u64()).unwrap_or(0);
                result.push(BpfMapInfo {
                    id: u32::try_from(id).unwrap_or(0),
                    name: name.to_string(),
                    map_type: map
                        .get("type")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown")
                        .to_string(),
                });
            }
        }

        Ok(result)
    }

    /// Find map by name
    pub fn find_map_by_name(&self, name: &str) -> Result<Option<BpfMapInfo>> {
        let maps = self.list_maps()?;
        Ok(maps.into_iter().find(|m| m.name == name))
    }

    /// Dump map contents
    pub fn dump_map(&self, map_id: u32) -> Result<Vec<(Vec<u8>, Vec<u8>)>> {
        let output = Command::new(&self.bpftool_path)
            .args(["map", "dump", "id", &map_id.to_string(), "-j"])
            .output()
            .context("Failed to dump map")?;

        if !output.status.success() {
            return Ok(Vec::new());
        }

        let json_str = String::from_utf8(output.stdout).context("Failed to parse dump output")?;

        let entries: Vec<Value> = serde_json::from_str(&json_str).unwrap_or_default();

        let mut result = Vec::new();
        for entry in entries {
            if let (Some(key), Some(value)) = (entry.get("key"), entry.get("value")) {
                // Parse hex arrays to bytes
                if let (Ok(key_bytes), Ok(val_bytes)) =
                    (Self::parse_hex_array(key), Self::parse_hex_array(value))
                {
                    result.push((key_bytes, val_bytes));
                }
            }
        }

        Ok(result)
    }

    /// Parse hex array from JSON ["0x01", "0x02", ...]
    fn parse_hex_array(value: &Value) -> Result<Vec<u8>> {
        if let Some(arr) = value.as_array() {
            let mut bytes = Vec::new();
            for item in arr {
                if let Some(hex_str) = item.as_str() {
                    let cleaned = hex_str.trim_start_matches("0x");
                    if let Ok(byte) = u8::from_str_radix(cleaned, 16) {
                        bytes.push(byte);
                    }
                }
            }
            Ok(bytes)
        } else {
            Ok(Vec::new())
        }
    }

    /// Read Cilium conntrack map (both IPv4 and IPv6)
    pub fn read_cilium_ct_map(&self) -> Result<Vec<ConntrackEntry>> {
        let mut ct_entries = Vec::new();

        // Read IPv4 conntrack map
        if let Some(ct4_map) = self.find_map_by_name("cilium_ct4_global")? {
            let entries = self.dump_map(ct4_map.id)?;
            for (key, value) in entries {
                if let Ok(ct) = parse_ct_entry(&key, &value) {
                    ct_entries.push(ct);
                }
            }
        }

        // Read IPv6 conntrack map
        if let Some(ct6_map) = self.find_map_by_name("cilium_ct6_global")? {
            let entries = self.dump_map(ct6_map.id)?;
            for (key, value) in entries {
                if let Ok(ct) = parse_ct6_entry(&key, &value) {
                    ct_entries.push(ct);
                }
            }
        }

        Ok(ct_entries)
    }

    /// Read Cilium IP cache
    pub fn read_cilium_ipcache(&self) -> Result<Vec<IPCacheEntry>> {
        let map = match self.find_map_by_name("cilium_ipcache")? {
            Some(m) => m,
            None => return Ok(Vec::new()),
        };

        let entries = self.dump_map(map.id)?;

        let mut ipcache_entries = Vec::new();

        for (key_bytes, value_bytes) in entries {
            if let Ok(entry) = parse_ipcache_entry(&key_bytes, &value_bytes) {
                ipcache_entries.push(entry);
            }
        }

        Ok(ipcache_entries)
    }

    /// Read Cilium load balancer map
    pub fn read_cilium_lb_map(&self) -> Result<Vec<LoadBalancerEntry>> {
        let map = match self.find_map_by_name("cilium_lb4_services")? {
            Some(m) => m,
            None => return Ok(Vec::new()),
        };

        let entries = self.dump_map(map.id)?;

        let mut lb_entries = Vec::new();

        for (key, value) in entries {
            if let Ok(entry) = parse_lb_entry(&key, &value) {
                lb_entries.push(entry);
            }
        }

        Ok(lb_entries)
    }

    /// Read Cilium policy map
    pub fn read_cilium_policy_map(&self) -> Result<Vec<PolicyDecision>> {
        let map = match self.find_map_by_name("cilium_policy")? {
            Some(m) => m,
            None => return Ok(Vec::new()),
        };

        let entries = self.dump_map(map.id)?;

        let mut policy_entries = Vec::new();

        for (key, value) in entries {
            if let Ok(entry) = parse_policy_entry(&key, &value) {
                policy_entries.push(entry);
            }
        }

        Ok(policy_entries)
    }

    /// Read Cilium metrics/drop counters
    pub fn read_cilium_metrics(&self) -> Result<HashMap<DropReasonType, u64>> {
        let map = match self.find_map_by_name("cilium_metrics")? {
            Some(m) => m,
            None => return Ok(HashMap::new()),
        };

        let entries = self.dump_map(map.id)?;

        let mut metrics = HashMap::new();

        for (key, value) in entries {
            if let Ok((reason, count)) = parse_metrics_entry(&key, &value) {
                *metrics.entry(reason).or_insert(0) += count;
            }
        }

        Ok(metrics)
    }

    /// Check if bpftool is available
    pub fn is_available(&self) -> bool {
        std::path::Path::new(&self.bpftool_path).exists()
    }
}

/// BPF Map Information
#[derive(Debug, Clone)]
pub struct BpfMapInfo {
    pub id: u32,
    pub name: String,
    #[allow(dead_code)]
    pub map_type: String,
}

/// Identity information
#[derive(Debug, Clone)]
pub struct IdentityInfo {
    pub identity: u32,
    pub labels: Vec<String>,
    pub namespace: String,
    pub pod_name: String,
}

/// Maximum number of cached identities before eviction
const MAX_IDENTITY_CACHE_SIZE: usize = 10_000;

/// Identity resolver - maps Cilium identities to K8s labels
#[allow(dead_code)]
pub struct IdentityResolver {
    identity_cache: HashMap<u32, IdentityInfo>,
}

impl Default for IdentityResolver {
    fn default() -> Self {
        Self::new()
    }
}

#[allow(dead_code)]
impl IdentityResolver {
    pub fn new() -> Self {
        Self {
            identity_cache: HashMap::new(),
        }
    }

    /// Resolve identity to labels via the local cache, falling back to
    /// `cilium identity get <id> -o json` when the identity is unknown.
    pub async fn resolve_identity(&mut self, identity: u32) -> Option<&IdentityInfo> {
        if self.identity_cache.contains_key(&identity) {
            return self.identity_cache.get(&identity);
        }

        // Query Cilium CLI for the identity (with 5s timeout)
        let cmd = tokio::process::Command::new("cilium")
            .args(["identity", "get", &identity.to_string(), "-o", "json"])
            .output();
        if let Ok(Ok(output)) = tokio::time::timeout(std::time::Duration::from_secs(5), cmd).await {
            if output.status.success() {
                if let Ok(json_str) = String::from_utf8(output.stdout) {
                    if let Ok(val) = serde_json::from_str::<Value>(&json_str) {
                        let labels: Vec<String> = val
                            .get("labels")
                            .and_then(|v| v.as_array())
                            .map(|arr| {
                                arr.iter()
                                    .filter_map(|l| l.as_str().map(String::from))
                                    .collect()
                            })
                            .unwrap_or_default();

                        // Extract namespace and pod from labels (k8s:io.kubernetes.pod.namespace=X)
                        let namespace = labels
                            .iter()
                            .find_map(|l| l.strip_prefix("k8s:io.kubernetes.pod.namespace="))
                            .unwrap_or("unknown")
                            .to_string();
                        let pod_name = labels
                            .iter()
                            .find_map(|l| l.strip_prefix("k8s:io.cilium.k8s.policy.name="))
                            .unwrap_or("unknown")
                            .to_string();

                        // Evict oldest entries if cache is full
                        if self.identity_cache.len() >= MAX_IDENTITY_CACHE_SIZE {
                            let keys: Vec<u32> = self
                                .identity_cache
                                .keys()
                                .take(MAX_IDENTITY_CACHE_SIZE / 4)
                                .copied()
                                .collect();
                            for k in keys {
                                self.identity_cache.remove(&k);
                            }
                        }

                        self.identity_cache.insert(
                            identity,
                            IdentityInfo {
                                identity,
                                labels,
                                namespace,
                                pod_name,
                            },
                        );

                        return self.identity_cache.get(&identity);
                    }
                }
            }
        }

        tracing::debug!(identity, "Could not resolve Cilium identity");
        None
    }

    /// Resolve IP to identity by scanning the ipcache BPF map via bpftool.
    ///
    /// Parses `cilium bpf ipcache list -o json` output, matching the
    /// requested IP to its assigned security identity.
    pub async fn resolve_ip(&self, ip: &IpAddr) -> Option<u32> {
        let ip_str = ip.to_string();

        // Try `cilium bpf ipcache list -o json` (with 5s timeout)
        let cmd = tokio::process::Command::new("cilium")
            .args(["bpf", "ipcache", "list", "-o", "json"])
            .output();
        if let Ok(Ok(output)) = tokio::time::timeout(std::time::Duration::from_secs(5), cmd).await {
            if output.status.success() {
                if let Ok(json_str) = String::from_utf8(output.stdout) {
                    if let Ok(entries) = serde_json::from_str::<Vec<Value>>(&json_str) {
                        for entry in &entries {
                            let cidr = entry.get("cidr").and_then(|v| v.as_str()).unwrap_or("");
                            // Match exact IP or CIDR prefix (e.g. "10.0.0.1/32")
                            // Use exact match on the IP portion before the '/'
                            let cidr_ip = cidr.split('/').next().unwrap_or("");
                            if cidr_ip == ip_str {
                                if let Some(id) = entry.get("identity").and_then(|v| v.as_u64()) {
                                    return Some(id as u32);
                                }
                            }
                        }
                    }
                }
            }
        }

        None
    }

    /// Add identity to cache
    pub fn add_identity(&mut self, info: IdentityInfo) {
        self.identity_cache.insert(info.identity, info);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_bpftool() {
        // This test will fail if bpftool is not installed
        // which is expected in CI environments
        match BpfToolReader::find_bpftool() {
            Ok(path) => {
                assert!(!path.is_empty());
                assert!(path.contains("bpftool"));
            }
            Err(_) => {
                // bpftool not available, skip test
                println!("bpftool not available, skipping test");
            }
        }
    }

    #[test]
    fn test_parse_hex_array() {
        let json_val = serde_json::json!(["0x01", "0x02", "0x0a", "0xff"]);
        let bytes = BpfToolReader::parse_hex_array(&json_val).unwrap();
        assert_eq!(bytes, vec![0x01, 0x02, 0x0a, 0xff]);
    }

    #[test]
    fn test_identity_resolver_add() {
        let mut resolver = IdentityResolver::new();

        resolver.add_identity(IdentityInfo {
            identity: 100,
            labels: vec!["app=web".to_string()],
            namespace: "default".to_string(),
            pod_name: "web-pod-123".to_string(),
        });

        assert_eq!(resolver.identity_cache.len(), 1);
    }

    #[tokio::test]
    async fn test_resolve_identity_from_cache() {
        let mut resolver = IdentityResolver::new();

        resolver.add_identity(IdentityInfo {
            identity: 200,
            labels: vec!["app=api".to_string()],
            namespace: "prod".to_string(),
            pod_name: "api-pod-456".to_string(),
        });

        let info = resolver.resolve_identity(200).await;
        assert!(info.is_some());
        let info = info.unwrap();
        assert_eq!(info.identity, 200);
        assert_eq!(info.namespace, "prod");
    }

    #[tokio::test]
    async fn test_resolve_identity_unknown_returns_none() {
        let mut resolver = IdentityResolver::new();
        // Identity not in cache and cilium CLI likely not available
        let info = resolver.resolve_identity(99999).await;
        // Without cilium CLI, the identity won't be found
        assert!(info.is_none());
    }

    #[tokio::test]
    async fn test_resolve_ip_without_cilium() {
        let resolver = IdentityResolver::new();
        let ip: IpAddr = "10.0.0.1".parse().unwrap();
        // Without cilium CLI, returns None gracefully
        let result = resolver.resolve_ip(&ip).await;
        assert!(result.is_none());
    }
}
