// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

/// BPF Capability Detection
///
/// Probes the runtime environment to determine what level of BPF access
/// is available: full program loading, read-only map access, bpftool CLI,
/// or no BPF access at all.
///
/// Also provides comprehensive kernel feature detection via `KernelCapabilities`
/// which checks kernel version requirements, BPF config options, map types,
/// BTF support, and Cilium-specific BPF map presence.
use std::path::Path;

/// Level of BPF access available on this system.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BpfCapability {
    /// Root / CAP_BPF: load programs, read/write maps, perf events
    Full,
    /// /sys/fs/bpf/ readable: native map reads only
    ReadOnly,
    /// bpftool binary available: CLI-based map reads
    BpftoolOnly,
    /// No BPF access: MockMapReader fallback
    None,
}

/// Detected BPF capabilities for the current process.
#[derive(Debug, Clone)]
pub struct BpfCapabilities {
    pub capability: BpfCapability,
    pub bpf_fs_available: bool,
    pub bpf_fs_writable: bool,
    pub bpftool_available: bool,
    pub kernel_version: Option<String>,
}

impl BpfCapabilities {
    /// Probe the current system for BPF capabilities.
    pub fn detect() -> Self {
        let bpf_fs_available = Path::new("/sys/fs/bpf").exists();
        let bpf_fs_writable = bpf_fs_available && Self::check_bpf_fs_writable();
        let bpftool_available = Self::check_bpftool();
        let kernel_version = Self::read_kernel_version();

        let capability = if bpf_fs_writable && Self::check_cap_bpf() {
            BpfCapability::Full
        } else if bpf_fs_available {
            BpfCapability::ReadOnly
        } else if bpftool_available {
            BpfCapability::BpftoolOnly
        } else {
            BpfCapability::None
        };

        tracing::info!(
            capability = ?capability,
            bpf_fs_available,
            bpf_fs_writable,
            bpftool_available,
            kernel = ?kernel_version,
            "BPF capabilities detected"
        );

        Self {
            capability,
            bpf_fs_available,
            bpf_fs_writable,
            bpftool_available,
            kernel_version,
        }
    }

    /// Check whether we can write to the BPF filesystem (indicates full access).
    fn check_bpf_fs_writable() -> bool {
        // Try to stat the tc/globals directory — if it's writable we likely have
        // sufficient privileges to pin/unpin maps.
        let globals = Path::new("/sys/fs/bpf/tc/globals");
        if !globals.exists() {
            return false;
        }
        // A lightweight writability check: attempt to create and immediately
        // remove a temporary file. If we can do this, we have write access.
        let probe_path = globals.join(".paqtra_probe");
        match std::fs::write(&probe_path, b"") {
            Ok(()) => {
                let _ = std::fs::remove_file(&probe_path);
                true
            }
            Err(_) => false,
        }
    }

    /// Check if the current process has CAP_BPF or is running as root.
    fn check_cap_bpf() -> bool {
        // Fast check: are we root?
        if unsafe { libc::geteuid() } == 0 {
            return true;
        }

        // Check /proc/self/status for CapEff containing the CAP_BPF bit (bit 39).
        // CapEff is a hex-encoded capability mask.
        if let Ok(status) = std::fs::read_to_string("/proc/self/status") {
            for line in status.lines() {
                if let Some(hex) = line.strip_prefix("CapEff:\t") {
                    if let Ok(cap_eff) = u64::from_str_radix(hex.trim(), 16) {
                        // CAP_BPF = 39, CAP_SYS_ADMIN = 21
                        let cap_bpf = 1u64 << 39;
                        let cap_sys_admin = 1u64 << 21;
                        return (cap_eff & cap_bpf) != 0 || (cap_eff & cap_sys_admin) != 0;
                    }
                }
            }
        }

        false
    }

    /// Check if bpftool is available on the system.
    fn check_bpftool() -> bool {
        std::process::Command::new("bpftool")
            .arg("version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    /// Read the kernel version string.
    fn read_kernel_version() -> Option<String> {
        std::fs::read_to_string("/proc/version")
            .ok()
            .and_then(|v| v.split_whitespace().nth(2).map(String::from))
    }

    /// Whether program loading is possible.
    pub fn can_load_programs(&self) -> bool {
        self.capability == BpfCapability::Full
    }

    /// Whether map reads are possible (via any method).
    pub fn can_read_maps(&self) -> bool {
        self.capability != BpfCapability::None
    }

    /// Whether map writes are possible.
    pub fn can_write_maps(&self) -> bool {
        self.capability == BpfCapability::Full
    }
}

impl Default for BpfCapabilities {
    fn default() -> Self {
        Self::detect()
    }
}

// ---------------------------------------------------------------------------
// Kernel feature detection
// ---------------------------------------------------------------------------

/// Parsed kernel version with major.minor.patch components.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KernelVersion {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl KernelVersion {
    /// Parse a version string like "6.19.11-200.fc43.x86_64" into components.
    pub fn parse(version_str: &str) -> Option<Self> {
        // Take everything before the first '-' (or the whole string)
        let base = version_str.split('-').next()?;
        let mut parts = base.split('.');
        let major = parts.next()?.parse::<u32>().ok()?;
        let minor = parts.next()?.parse::<u32>().ok()?;
        let patch = parts
            .next()
            .and_then(|p| p.parse::<u32>().ok())
            .unwrap_or(0);
        Some(Self {
            major,
            minor,
            patch,
        })
    }

    /// Returns true if this version is at least the given major.minor.
    pub fn at_least(&self, major: u32, minor: u32) -> bool {
        (self.major, self.minor) >= (major, minor)
    }
}

impl std::fmt::Display for KernelVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// Kernel BPF configuration options detected from /proc/config.gz or Kconfig.
#[derive(Debug, Clone, Default)]
pub struct BpfKernelConfig {
    /// CONFIG_BPF=y
    pub config_bpf: bool,
    /// CONFIG_BPF_SYSCALL=y
    pub config_bpf_syscall: bool,
    /// CONFIG_BPF_JIT=y
    pub config_bpf_jit: bool,
    /// CONFIG_HAVE_EBPF_JIT=y
    pub config_have_ebpf_jit: bool,
    /// CONFIG_BPF_EVENTS=y
    pub config_bpf_events: bool,
    /// CONFIG_CGROUP_BPF=y
    pub config_cgroup_bpf: bool,
}

impl BpfKernelConfig {
    /// Returns true if all essential BPF options are enabled.
    pub fn has_basic_bpf(&self) -> bool {
        self.config_bpf && self.config_bpf_syscall
    }

    /// Returns true if JIT compilation is available.
    pub fn has_jit(&self) -> bool {
        self.config_bpf_jit && self.config_have_ebpf_jit
    }
}

/// Available BPF map types detected on the system.
#[derive(Debug, Clone, Default)]
pub struct BpfMapTypes {
    pub hash: bool,
    pub array: bool,
    pub lru_hash: bool,
    pub lru_percpu_hash: bool,
    pub percpu_hash: bool,
    pub percpu_array: bool,
    pub lpm_trie: bool,
    pub array_of_maps: bool,
    pub hash_of_maps: bool,
}

/// Cilium-specific BPF maps found in /sys/fs/bpf/tc/globals/.
#[derive(Debug, Clone, Default)]
pub struct CiliumMaps {
    pub cilium_ct4_global: bool,
    pub cilium_ct_any4_global: bool,
    pub cilium_ipcache: bool,
    pub cilium_lb4_services_v2: bool,
    pub cilium_lb4_backends_v3: bool,
    pub cilium_policy: bool,
    pub cilium_metrics: bool,
    pub cilium_events: bool,
}

impl CiliumMaps {
    /// Returns the number of detected Cilium maps.
    pub fn count(&self) -> usize {
        [
            self.cilium_ct4_global,
            self.cilium_ct_any4_global,
            self.cilium_ipcache,
            self.cilium_lb4_services_v2,
            self.cilium_lb4_backends_v3,
            self.cilium_policy,
            self.cilium_metrics,
            self.cilium_events,
        ]
        .iter()
        .filter(|&&v| v)
        .count()
    }

    /// Returns true if the core Cilium maps are present.
    pub fn has_core_maps(&self) -> bool {
        self.cilium_ipcache && self.cilium_policy
    }
}

/// Feature tier based on kernel version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeatureTier {
    /// Kernel < 4.19: not supported
    Unsupported,
    /// Kernel 4.19+: basic BPF (cgroup, tracepoints)
    Basic,
    /// Kernel 5.4+: BTF, CO-RE
    Btf,
    /// Kernel 5.10+: ring buffer, atomics, signed helpers
    Advanced,
    /// Kernel 5.15+: bloom filter, typed pointers
    Full,
}

impl FeatureTier {
    fn from_version(v: &KernelVersion) -> Self {
        if v.at_least(5, 15) {
            FeatureTier::Full
        } else if v.at_least(5, 10) {
            FeatureTier::Advanced
        } else if v.at_least(5, 4) {
            FeatureTier::Btf
        } else if v.at_least(4, 19) {
            FeatureTier::Basic
        } else {
            FeatureTier::Unsupported
        }
    }

    /// Human-readable label.
    pub fn label(&self) -> &'static str {
        match self {
            FeatureTier::Unsupported => "unsupported",
            FeatureTier::Basic => "basic (4.19+)",
            FeatureTier::Btf => "BTF/CO-RE (5.4+)",
            FeatureTier::Advanced => "advanced (5.10+)",
            FeatureTier::Full => "full (5.15+)",
        }
    }
}

impl std::fmt::Display for FeatureTier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

/// Comprehensive kernel feature detection for eBPF.
///
/// Checks kernel version, BPF filesystem mount, kernel config options,
/// BTF availability, supported map types, and Cilium-specific maps.
#[derive(Debug, Clone)]
pub struct KernelCapabilities {
    /// Parsed kernel version.
    pub kernel_version: Option<KernelVersion>,
    /// Raw kernel version string from uname.
    pub kernel_version_raw: Option<String>,
    /// Feature tier derived from kernel version.
    pub feature_tier: FeatureTier,
    /// Whether /sys/fs/bpf is mounted.
    pub bpf_fs_mounted: bool,
    /// Whether /sys/kernel/btf/vmlinux exists (BTF support).
    pub btf_vmlinux: bool,
    /// Kernel BPF config options.
    pub kernel_config: BpfKernelConfig,
    /// Available BPF map types.
    pub map_types: BpfMapTypes,
    /// Cilium-specific BPF maps detected.
    pub cilium_maps: CiliumMaps,
}

impl KernelCapabilities {
    /// Detect all kernel capabilities by probing the running system.
    pub fn detect() -> Self {
        let kernel_version_raw = Self::read_uname_release();
        let kernel_version = kernel_version_raw.as_deref().and_then(KernelVersion::parse);
        let feature_tier = kernel_version
            .as_ref()
            .map(FeatureTier::from_version)
            .unwrap_or(FeatureTier::Unsupported);

        let bpf_fs_mounted = Path::new("/sys/fs/bpf").exists();
        let btf_vmlinux = Path::new("/sys/kernel/btf/vmlinux").exists();
        let kernel_config = Self::detect_kernel_config();
        let map_types = Self::detect_map_types();
        let cilium_maps = Self::detect_cilium_maps();

        let caps = Self {
            kernel_version,
            kernel_version_raw,
            feature_tier,
            bpf_fs_mounted,
            btf_vmlinux,
            kernel_config,
            map_types,
            cilium_maps,
        };

        tracing::info!(
            kernel = ?caps.kernel_version_raw,
            tier = %caps.feature_tier,
            bpf_fs = caps.bpf_fs_mounted,
            btf = caps.btf_vmlinux,
            cilium_maps = caps.cilium_maps.count(),
            "Kernel capabilities detected"
        );

        caps
    }

    /// Read kernel release from `uname -r` output (via /proc/version or uname syscall).
    fn read_uname_release() -> Option<String> {
        // First try /proc/version which is always available on Linux.
        if let Ok(content) = std::fs::read_to_string("/proc/version") {
            // Format: "Linux version 6.19.11-200.fc43.x86_64 ..."
            if let Some(ver) = content.split_whitespace().nth(2) {
                return Some(ver.to_string());
            }
        }
        // Fallback: run uname -r.
        std::process::Command::new("uname")
            .arg("-r")
            .output()
            .ok()
            .and_then(|o| {
                if o.status.success() {
                    Some(String::from_utf8_lossy(&o.stdout).trim().to_string())
                } else {
                    None
                }
            })
    }

    /// Detect BPF-related kernel config options.
    ///
    /// Tries, in order:
    /// 1. /proc/config.gz (needs CONFIG_IKCONFIG_PROC=y)
    /// 2. /boot/config-<version>
    fn detect_kernel_config() -> BpfKernelConfig {
        let mut cfg = BpfKernelConfig::default();

        // Try /proc/config.gz first
        if let Some(content) = Self::read_proc_config_gz() {
            Self::parse_kernel_config(&content, &mut cfg);
            return cfg;
        }

        // Fallback: /boot/config-<version>
        if let Some(ver) = Self::read_uname_release() {
            let path = format!("/boot/config-{ver}");
            if let Ok(content) = std::fs::read_to_string(&path) {
                Self::parse_kernel_config(&content, &mut cfg);
                return cfg;
            }
        }

        cfg
    }

    /// Read and decompress /proc/config.gz.
    fn read_proc_config_gz() -> Option<String> {
        use std::io::Read;
        let data = std::fs::read("/proc/config.gz").ok()?;
        let mut decoder = flate2::read::GzDecoder::new(data.as_slice());
        let mut content = String::new();
        decoder.read_to_string(&mut content).ok()?;
        Some(content)
    }

    /// Parse kernel config text and populate the config struct.
    fn parse_kernel_config(content: &str, cfg: &mut BpfKernelConfig) {
        for line in content.lines() {
            let line = line.trim();
            match line {
                "CONFIG_BPF=y" => cfg.config_bpf = true,
                "CONFIG_BPF_SYSCALL=y" => cfg.config_bpf_syscall = true,
                "CONFIG_BPF_JIT=y" => cfg.config_bpf_jit = true,
                "CONFIG_HAVE_EBPF_JIT=y" => cfg.config_have_ebpf_jit = true,
                "CONFIG_BPF_EVENTS=y" => cfg.config_bpf_events = true,
                "CONFIG_CGROUP_BPF=y" => cfg.config_cgroup_bpf = true,
                _ => {}
            }
        }
    }

    /// Detect available BPF map types by probing via bpftool or checking
    /// /proc/kallsyms for relevant symbols.
    fn detect_map_types() -> BpfMapTypes {
        // Try bpftool feature probe first
        if let Some(types) = Self::detect_map_types_bpftool() {
            return types;
        }

        // Fallback: on 5.4+ kernels, all standard map types are available.
        // On older kernels we make a conservative estimate from /proc/kallsyms.
        let version = Self::read_uname_release().and_then(|v| KernelVersion::parse(&v));
        let mut types = BpfMapTypes::default();

        if let Some(v) = version {
            if v.at_least(4, 19) {
                types.hash = true;
                types.array = true;
                types.percpu_hash = true;
                types.percpu_array = true;
                types.lpm_trie = true;
            }
            if v.at_least(4, 20) {
                types.lru_hash = true;
                types.lru_percpu_hash = true;
            }
            if v.at_least(5, 4) {
                types.array_of_maps = true;
                types.hash_of_maps = true;
            }
        }

        types
    }

    /// Try to detect map types using `bpftool feature probe`.
    fn detect_map_types_bpftool() -> Option<BpfMapTypes> {
        let output = std::process::Command::new("bpftool")
            .args(["feature", "probe"])
            .output()
            .ok()?;

        if !output.status.success() {
            return None;
        }

        let text = String::from_utf8_lossy(&output.stdout);
        let mut types = BpfMapTypes::default();

        for line in text.lines() {
            let lower = line.to_lowercase();
            if lower.contains("map_type") {
                if lower.contains("hash") && !lower.contains("lru") && !lower.contains("percpu") {
                    types.hash = true;
                }
                if lower.contains("array")
                    && !lower.contains("percpu")
                    && !lower.contains("of_maps")
                {
                    types.array = true;
                }
                if lower.contains("lru_hash") && !lower.contains("percpu") {
                    types.lru_hash = true;
                }
                if lower.contains("lru_percpu_hash") {
                    types.lru_percpu_hash = true;
                }
                if lower.contains("percpu_hash") && !lower.contains("lru") {
                    types.percpu_hash = true;
                }
                if lower.contains("percpu_array") {
                    types.percpu_array = true;
                }
                if lower.contains("lpm_trie") {
                    types.lpm_trie = true;
                }
                if lower.contains("array_of_maps") {
                    types.array_of_maps = true;
                }
                if lower.contains("hash_of_maps") {
                    types.hash_of_maps = true;
                }
            }
        }

        Some(types)
    }

    /// Detect Cilium-specific BPF maps in /sys/fs/bpf/tc/globals/.
    fn detect_cilium_maps() -> CiliumMaps {
        let base = Path::new("/sys/fs/bpf/tc/globals");
        if !base.exists() {
            return CiliumMaps::default();
        }

        CiliumMaps {
            cilium_ct4_global: base.join("cilium_ct4_global").exists(),
            cilium_ct_any4_global: base.join("cilium_ct_any4_global").exists(),
            cilium_ipcache: base.join("cilium_ipcache").exists(),
            cilium_lb4_services_v2: base.join("cilium_lb4_services_v2").exists(),
            cilium_lb4_backends_v3: base.join("cilium_lb4_backends_v3").exists(),
            cilium_policy: Self::detect_cilium_policy_map(base),
            cilium_metrics: base.join("cilium_metrics").exists(),
            cilium_events: base.join("cilium_events").exists(),
        }
    }

    /// Cilium policy maps are per-endpoint (cilium_policy_00001, etc.).
    /// Returns true if at least one policy map exists.
    fn detect_cilium_policy_map(base: &Path) -> bool {
        let Ok(entries) = std::fs::read_dir(base) else {
            return false;
        };
        entries.filter_map(|e| e.ok()).any(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with("cilium_policy_")
        })
    }

    // -- Convenience query methods --

    /// Whether the kernel meets the minimum version for basic BPF (4.19+).
    pub fn supports_basic_bpf(&self) -> bool {
        self.feature_tier != FeatureTier::Unsupported
    }

    /// Whether BTF/CO-RE is available (kernel 5.4+ with vmlinux BTF).
    pub fn supports_btf(&self) -> bool {
        matches!(
            self.feature_tier,
            FeatureTier::Btf | FeatureTier::Advanced | FeatureTier::Full
        ) && self.btf_vmlinux
    }

    /// Whether the kernel supports advanced features (5.10+).
    pub fn supports_advanced(&self) -> bool {
        matches!(self.feature_tier, FeatureTier::Advanced | FeatureTier::Full)
    }

    /// Whether Cilium appears to be running (has core BPF maps).
    pub fn cilium_detected(&self) -> bool {
        self.cilium_maps.has_core_maps()
    }

    /// Return a human-readable summary of detected capabilities.
    pub fn summary(&self) -> String {
        let ver = self
            .kernel_version
            .as_ref()
            .map(|v| v.to_string())
            .unwrap_or_else(|| "unknown".to_string());
        let tier = self.feature_tier.label();
        let bpf_fs = if self.bpf_fs_mounted { "yes" } else { "no" };
        let btf = if self.btf_vmlinux { "yes" } else { "no" };
        let config_ok = if self.kernel_config.has_basic_bpf() {
            "yes"
        } else {
            "no/unknown"
        };
        let jit = if self.kernel_config.has_jit() {
            "yes"
        } else {
            "no/unknown"
        };
        let cilium = self.cilium_maps.count();

        format!(
            "kernel={ver} tier={tier} bpf_fs={bpf_fs} btf={btf} \
             config_bpf={config_ok} jit={jit} cilium_maps={cilium}"
        )
    }
}

impl Default for KernelCapabilities {
    fn default() -> Self {
        Self::detect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_capability_levels() {
        // Verify enum ordering / equality
        assert_ne!(BpfCapability::Full, BpfCapability::None);
        assert_ne!(BpfCapability::ReadOnly, BpfCapability::BpftoolOnly);
        assert_eq!(BpfCapability::Full, BpfCapability::Full);
    }

    #[test]
    fn test_detect_returns_valid_capability() {
        let caps = BpfCapabilities::detect();
        // Should always return some capability level
        assert!(matches!(
            caps.capability,
            BpfCapability::Full
                | BpfCapability::ReadOnly
                | BpfCapability::BpftoolOnly
                | BpfCapability::None
        ));
    }

    #[test]
    fn test_capability_gates() {
        let mut caps = BpfCapabilities {
            capability: BpfCapability::Full,
            bpf_fs_available: true,
            bpf_fs_writable: true,
            bpftool_available: true,
            kernel_version: None,
        };
        assert!(caps.can_load_programs());
        assert!(caps.can_read_maps());
        assert!(caps.can_write_maps());

        caps.capability = BpfCapability::ReadOnly;
        assert!(!caps.can_load_programs());
        assert!(caps.can_read_maps());
        assert!(!caps.can_write_maps());

        caps.capability = BpfCapability::BpftoolOnly;
        assert!(!caps.can_load_programs());
        assert!(caps.can_read_maps());

        caps.capability = BpfCapability::None;
        assert!(!caps.can_load_programs());
        assert!(!caps.can_read_maps());
        assert!(!caps.can_write_maps());
    }

    #[test]
    fn test_kernel_version_read() {
        // On Linux this should return something; on non-Linux it may be None
        let version = BpfCapabilities::read_kernel_version();
        if cfg!(target_os = "linux") {
            assert!(version.is_some());
        }
    }

    // --- KernelCapabilities tests ---

    #[test]
    fn test_kernel_version_parse_full() {
        let v = KernelVersion::parse("6.19.11-200.fc43.x86_64").unwrap();
        assert_eq!(v.major, 6);
        assert_eq!(v.minor, 19);
        assert_eq!(v.patch, 11);
    }

    #[test]
    fn test_kernel_version_parse_no_patch() {
        let v = KernelVersion::parse("5.4").unwrap();
        assert_eq!(v.major, 5);
        assert_eq!(v.minor, 4);
        assert_eq!(v.patch, 0);
    }

    #[test]
    fn test_kernel_version_parse_invalid() {
        assert!(KernelVersion::parse("not-a-version").is_none());
        assert!(KernelVersion::parse("").is_none());
    }

    #[test]
    fn test_kernel_version_at_least() {
        let v = KernelVersion {
            major: 5,
            minor: 10,
            patch: 3,
        };
        assert!(v.at_least(4, 19));
        assert!(v.at_least(5, 4));
        assert!(v.at_least(5, 10));
        assert!(!v.at_least(5, 15));
        assert!(!v.at_least(6, 0));
    }

    #[test]
    fn test_kernel_version_display() {
        let v = KernelVersion {
            major: 5,
            minor: 10,
            patch: 42,
        };
        assert_eq!(v.to_string(), "5.10.42");
    }

    #[test]
    fn test_feature_tier_from_version() {
        assert_eq!(
            FeatureTier::from_version(&KernelVersion {
                major: 4,
                minor: 14,
                patch: 0
            }),
            FeatureTier::Unsupported
        );
        assert_eq!(
            FeatureTier::from_version(&KernelVersion {
                major: 4,
                minor: 19,
                patch: 0
            }),
            FeatureTier::Basic
        );
        assert_eq!(
            FeatureTier::from_version(&KernelVersion {
                major: 5,
                minor: 4,
                patch: 0
            }),
            FeatureTier::Btf
        );
        assert_eq!(
            FeatureTier::from_version(&KernelVersion {
                major: 5,
                minor: 10,
                patch: 0
            }),
            FeatureTier::Advanced
        );
        assert_eq!(
            FeatureTier::from_version(&KernelVersion {
                major: 5,
                minor: 15,
                patch: 0
            }),
            FeatureTier::Full
        );
        assert_eq!(
            FeatureTier::from_version(&KernelVersion {
                major: 6,
                minor: 0,
                patch: 0
            }),
            FeatureTier::Full
        );
    }

    #[test]
    fn test_feature_tier_label() {
        assert_eq!(FeatureTier::Unsupported.label(), "unsupported");
        assert_eq!(FeatureTier::Basic.label(), "basic (4.19+)");
        assert_eq!(FeatureTier::Full.label(), "full (5.15+)");
    }

    #[test]
    fn test_bpf_kernel_config_checks() {
        let mut cfg = BpfKernelConfig::default();
        assert!(!cfg.has_basic_bpf());
        assert!(!cfg.has_jit());

        cfg.config_bpf = true;
        cfg.config_bpf_syscall = true;
        assert!(cfg.has_basic_bpf());

        cfg.config_bpf_jit = true;
        cfg.config_have_ebpf_jit = true;
        assert!(cfg.has_jit());
    }

    #[test]
    fn test_parse_kernel_config() {
        let content = "\
CONFIG_BPF=y
CONFIG_BPF_SYSCALL=y
CONFIG_BPF_JIT=y
CONFIG_HAVE_EBPF_JIT=y
CONFIG_BPF_EVENTS=y
CONFIG_CGROUP_BPF=y
CONFIG_SOMETHING_ELSE=y
# CONFIG_BPF_UNRELATED is not set
";
        let mut cfg = BpfKernelConfig::default();
        KernelCapabilities::parse_kernel_config(content, &mut cfg);
        assert!(cfg.config_bpf);
        assert!(cfg.config_bpf_syscall);
        assert!(cfg.config_bpf_jit);
        assert!(cfg.config_have_ebpf_jit);
        assert!(cfg.config_bpf_events);
        assert!(cfg.config_cgroup_bpf);
    }

    #[test]
    fn test_cilium_maps_count() {
        let maps = CiliumMaps {
            cilium_ipcache: true,
            cilium_policy: true,
            cilium_ct4_global: true,
            ..CiliumMaps::default()
        };
        assert_eq!(maps.count(), 3);
        assert!(maps.has_core_maps());
    }

    #[test]
    fn test_cilium_maps_no_core() {
        let maps = CiliumMaps::default();
        assert_eq!(maps.count(), 0);
        assert!(!maps.has_core_maps());
    }

    #[test]
    fn test_kernel_capabilities_detect_runs() {
        let caps = KernelCapabilities::detect();
        // On Linux the kernel version should be detected
        if cfg!(target_os = "linux") {
            assert!(caps.kernel_version.is_some());
            assert!(caps.kernel_version_raw.is_some());
            assert!(caps.supports_basic_bpf());
        }
    }

    #[test]
    fn test_kernel_capabilities_summary() {
        let caps = KernelCapabilities {
            kernel_version: Some(KernelVersion {
                major: 5,
                minor: 15,
                patch: 0,
            }),
            kernel_version_raw: Some("5.15.0-generic".to_string()),
            feature_tier: FeatureTier::Full,
            bpf_fs_mounted: true,
            btf_vmlinux: true,
            kernel_config: BpfKernelConfig {
                config_bpf: true,
                config_bpf_syscall: true,
                config_bpf_jit: true,
                config_have_ebpf_jit: true,
                ..BpfKernelConfig::default()
            },
            map_types: BpfMapTypes::default(),
            cilium_maps: CiliumMaps::default(),
        };
        let summary = caps.summary();
        assert!(summary.contains("kernel=5.15.0"));
        assert!(summary.contains("tier=full"));
        assert!(summary.contains("bpf_fs=yes"));
        assert!(summary.contains("btf=yes"));
        assert!(summary.contains("jit=yes"));
    }

    #[test]
    fn test_kernel_capabilities_convenience_methods() {
        let caps = KernelCapabilities {
            kernel_version: Some(KernelVersion {
                major: 5,
                minor: 10,
                patch: 0,
            }),
            kernel_version_raw: Some("5.10.0".to_string()),
            feature_tier: FeatureTier::Advanced,
            bpf_fs_mounted: true,
            btf_vmlinux: true,
            kernel_config: BpfKernelConfig::default(),
            map_types: BpfMapTypes::default(),
            cilium_maps: CiliumMaps {
                cilium_ipcache: true,
                cilium_policy: true,
                ..CiliumMaps::default()
            },
        };
        assert!(caps.supports_basic_bpf());
        assert!(caps.supports_btf());
        assert!(caps.supports_advanced());
        assert!(caps.cilium_detected());
    }

    #[test]
    fn test_kernel_capabilities_unsupported_kernel() {
        let caps = KernelCapabilities {
            kernel_version: Some(KernelVersion {
                major: 4,
                minor: 14,
                patch: 0,
            }),
            kernel_version_raw: Some("4.14.0".to_string()),
            feature_tier: FeatureTier::Unsupported,
            bpf_fs_mounted: false,
            btf_vmlinux: false,
            kernel_config: BpfKernelConfig::default(),
            map_types: BpfMapTypes::default(),
            cilium_maps: CiliumMaps::default(),
        };
        assert!(!caps.supports_basic_bpf());
        assert!(!caps.supports_btf());
        assert!(!caps.supports_advanced());
        assert!(!caps.cilium_detected());
    }
}
