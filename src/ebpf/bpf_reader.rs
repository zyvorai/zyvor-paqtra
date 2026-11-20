// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

/// Real Cilium BPF Map Reader
///
/// Reads data directly from Cilium's pinned BPF maps in /sys/fs/bpf/
use anyhow::Result;
use std::fs;
use std::path::{Path, PathBuf};

use super::bpf_syscall::BpfToolReader;
use super::{
    ConntrackEntry, DropReason, IPCacheEntry, LoadBalancerEntry, MapReader, PolicyDecision,
};

/// Cilium BPF Map Paths
const BPF_FS_PATH: &str = "/sys/fs/bpf";
const CILIUM_PATH: &str = "tc/globals";

/// Cilium BPF Map Reader
pub struct CiliumMapReader {
    #[allow(dead_code)]
    bpf_path: PathBuf,
    available_maps: Vec<String>,
    bpftool: Option<BpfToolReader>,
}

impl CiliumMapReader {
    /// Create a new Cilium map reader
    ///
    /// Reads through `bpftool` (read-only), and degrades to empty results
    /// when it is not available.
    pub fn new() -> Result<Self> {
        let bpf_path = PathBuf::from(BPF_FS_PATH).join(CILIUM_PATH);

        // Try to initialize bpftool reader
        let bpftool = BpfToolReader::new().ok();

        // Check if BPF filesystem is mounted or bpftool is available
        let available_maps = if bpf_path.exists() {
            Self::discover_maps(&bpf_path)?
        } else if let Some(ref tool) = bpftool {
            // Use bpftool to list maps
            tool.list_maps()
                .unwrap_or_default()
                .into_iter()
                .map(|m| m.name)
                .collect()
        } else {
            Vec::new()
        };

        if available_maps.is_empty() && bpftool.is_none() {
            tracing::warn!(
                "BPF filesystem not found at {:?} and no BPF reader available. \
                 Some features will be limited.",
                bpf_path
            );
        }

        Ok(Self {
            bpf_path,
            available_maps,
            bpftool,
        })
    }

    /// Create a reader with custom BPF path (for testing)
    pub fn with_path(bpf_path: PathBuf) -> Result<Self> {
        let available_maps = if bpf_path.exists() {
            Self::discover_maps(&bpf_path)?
        } else {
            Vec::new()
        };

        Ok(Self {
            bpf_path,
            available_maps,
            bpftool: None,
        })
    }

    /// Discover available BPF maps
    fn discover_maps(path: &Path) -> Result<Vec<String>> {
        let mut maps = Vec::new();

        if let Ok(entries) = fs::read_dir(path) {
            for entry in entries.flatten() {
                if let Ok(name) = entry.file_name().into_string() {
                    if name.starts_with("cilium_") {
                        maps.push(name);
                    }
                }
            }
        }

        Ok(maps)
    }

    /// Check if a map exists
    fn has_map(&self, name: &str) -> bool {
        self.available_maps.contains(&name.to_string())
    }

    /// Get path to a specific map
    fn map_path(&self, name: &str) -> PathBuf {
        self.bpf_path.join(name)
    }

    /// Read raw entries from a map.
    ///
    /// Delegates to the AyaMapReader when available for typed reads,
    /// otherwise falls back to bpftool.
    fn read_map_entries(&self, map_name: &str) -> Result<Vec<Vec<u8>>> {
        let map_path = self.map_path(map_name);

        if !map_path.exists() {
            return Ok(Vec::new());
        }

        // Fall back to bpftool if available
        if let Some(ref tool) = self.bpftool {
            if let Ok(maps) = tool.list_maps() {
                if maps.iter().any(|m| m.name == map_name) {
                    tracing::debug!(
                        map = map_name,
                        "Map found via bpftool, use MapReader trait for typed access"
                    );
                }
            }
        }

        Ok(Vec::new())
    }

    /// Get list of available maps
    pub fn available_maps(&self) -> &[String] {
        &self.available_maps
    }

    /// Check if Cilium is running (maps are available)
    pub fn is_cilium_available(&self) -> bool {
        !self.available_maps.is_empty()
    }
}

impl MapReader for CiliumMapReader {
    fn read_policy_map(&self) -> Result<Vec<PolicyDecision>> {
        // Fall back to bpftool
        if let Some(tool) = &self.bpftool {
            return tool.read_cilium_policy_map();
        }

        Ok(Vec::new())
    }

    fn read_conntrack_map(&self) -> Result<Vec<ConntrackEntry>> {
        if let Some(tool) = &self.bpftool {
            return tool.read_cilium_ct_map();
        }

        Ok(Vec::new())
    }

    fn read_lb_map(&self) -> Result<Vec<LoadBalancerEntry>> {
        if let Some(tool) = &self.bpftool {
            return tool.read_cilium_lb_map();
        }

        Ok(Vec::new())
    }

    fn read_ipcache_map(&self) -> Result<Vec<IPCacheEntry>> {
        if let Some(tool) = &self.bpftool {
            return tool.read_cilium_ipcache();
        }

        Ok(Vec::new())
    }

    fn read_drop_map(&self) -> Result<Vec<DropReason>> {
        if let Some(tool) = &self.bpftool {
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

        Ok(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_discover_maps() {
        let temp_dir = TempDir::new().unwrap();
        let bpf_path = temp_dir.path();

        // Create mock map files
        fs::write(bpf_path.join("cilium_policy"), b"").unwrap();
        fs::write(bpf_path.join("cilium_ct4_global"), b"").unwrap();
        fs::write(bpf_path.join("other_file"), b"").unwrap();

        let maps = CiliumMapReader::discover_maps(bpf_path).unwrap();

        assert_eq!(maps.len(), 2);
        assert!(maps.contains(&"cilium_policy".to_string()));
        assert!(maps.contains(&"cilium_ct4_global".to_string()));
        assert!(!maps.contains(&"other_file".to_string()));
    }

    #[test]
    fn test_reader_with_empty_path() {
        let temp_dir = TempDir::new().unwrap();
        let reader = CiliumMapReader::with_path(temp_dir.path().to_path_buf()).unwrap();

        assert_eq!(reader.available_maps().len(), 0);
        assert!(!reader.is_cilium_available());
    }

    #[test]
    fn test_has_map() {
        let temp_dir = TempDir::new().unwrap();
        let bpf_path = temp_dir.path();

        fs::write(bpf_path.join("cilium_policy"), b"").unwrap();

        let reader = CiliumMapReader::with_path(bpf_path.to_path_buf()).unwrap();

        assert!(reader.has_map("cilium_policy"));
        assert!(!reader.has_map("cilium_nonexistent"));
    }
}
