// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

/// eBPF Map Reader Implementation
///
/// Reads Cilium eBPF maps from /sys/fs/bpf/ using bpftool when available,
/// falling back to empty results when BPF filesystem is not accessible.
use anyhow::Result;
use std::path::Path;

use super::bpf_syscall::BpfToolReader;
use super::*;

pub struct BasicMapReader {
    map_root: String,
    bpftool: Option<BpfToolReader>,
}

impl BasicMapReader {
    pub fn new() -> Self {
        let bpftool = BpfToolReader::new().ok();
        Self {
            map_root: "/sys/fs/bpf/tc/globals".to_string(),
            bpftool,
        }
    }

    pub fn with_root(map_root: String) -> Self {
        let bpftool = BpfToolReader::new().ok();
        Self { map_root, bpftool }
    }

    pub fn is_available(&self) -> bool {
        Path::new(&self.map_root).exists() || self.bpftool.is_some()
    }

    /// Check if specific Cilium map exists
    pub fn map_exists(&self, map_name: &str) -> bool {
        let path = format!("{}/{}", self.map_root, map_name);
        if Path::new(&path).exists() {
            return true;
        }
        if let Some(ref tool) = self.bpftool {
            return tool.find_map_by_name(map_name).ok().flatten().is_some();
        }
        false
    }

    /// List all available Cilium maps
    pub fn list_maps(&self) -> Result<Vec<String>> {
        let path = Path::new(&self.map_root);
        if path.exists() {
            let mut maps = vec![];
            for entry in std::fs::read_dir(path)? {
                let entry = entry?;
                if let Some(name) = entry.file_name().to_str() {
                    if name.starts_with("cilium_") {
                        maps.push(name.to_string());
                    }
                }
            }
            if !maps.is_empty() {
                return Ok(maps);
            }
        }

        // Fall back to bpftool
        if let Some(ref tool) = self.bpftool {
            return Ok(tool
                .list_maps()
                .unwrap_or_default()
                .into_iter()
                .map(|m| m.name)
                .filter(|n| n.starts_with("cilium_"))
                .collect());
        }

        Ok(vec![])
    }
}

impl Default for BasicMapReader {
    fn default() -> Self {
        Self::new()
    }
}

impl MapReader for BasicMapReader {
    fn read_policy_map(&self) -> Result<Vec<PolicyDecision>> {
        if let Some(ref tool) = self.bpftool {
            return tool.read_cilium_policy_map();
        }
        Ok(vec![])
    }

    fn read_conntrack_map(&self) -> Result<Vec<ConntrackEntry>> {
        if let Some(ref tool) = self.bpftool {
            return tool.read_cilium_ct_map();
        }
        Ok(vec![])
    }

    fn read_lb_map(&self) -> Result<Vec<LoadBalancerEntry>> {
        if let Some(ref tool) = self.bpftool {
            return tool.read_cilium_lb_map();
        }
        Ok(vec![])
    }

    fn read_ipcache_map(&self) -> Result<Vec<IPCacheEntry>> {
        if let Some(ref tool) = self.bpftool {
            return tool.read_cilium_ipcache();
        }
        Ok(vec![])
    }

    fn read_drop_map(&self) -> Result<Vec<DropReason>> {
        if let Some(ref tool) = self.bpftool {
            let metrics = tool.read_cilium_metrics()?;
            let mut drops = Vec::new();
            for (reason, count) in metrics {
                if count > 0 {
                    drops.push(DropReason {
                        src_ip: "0.0.0.0".to_string(),
                        dst_ip: "0.0.0.0".to_string(),
                        port: 0,
                        protocol: 0,
                        reason,
                        timestamp: 0,
                    });
                }
            }
            return Ok(drops);
        }
        Ok(vec![])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_map_reader_creation() {
        let reader = BasicMapReader::new();
        // Map root should be set
        assert!(!reader.map_root.is_empty());
    }

    #[test]
    fn test_mock_reader() {
        let reader = MockMapReader;
        let policies = reader.read_policy_map().unwrap();
        // Mock should return at least one entry
        assert!(!policies.is_empty());
    }
}
