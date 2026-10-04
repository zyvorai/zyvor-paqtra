use super::{labels, read_lines, Chart, Collector, Emitter, Fsys, Info};
use std::collections::HashSet;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct FsUsage {
    pub block_size: u64,
    pub blocks: u64,
    pub free: u64,
    pub avail: u64,
    pub inodes: u64,
    pub inodes_free: u64,
}

fn statvfs(path: &std::path::Path) -> Result<FsUsage, String> {
    use std::os::unix::ffi::OsStrExt;
    let c = std::ffi::CString::new(path.as_os_str().as_bytes()).map_err(|e| e.to_string())?;
    let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: `c` is a valid NUL-terminated path and `st` is writable.
    if unsafe { libc::statvfs(c.as_ptr(), &mut st) } != 0 {
        return Err(format!(
            "statvfs {}: {}",
            path.display(),
            std::io::Error::last_os_error()
        ));
    }
    #[allow(clippy::unnecessary_cast)]
    Ok(FsUsage {
        block_size: st.f_frsize as u64,
        blocks: st.f_blocks as u64,
        free: st.f_bfree as u64,
        avail: st.f_bavail as u64,
        inodes: st.f_files as u64,
        inodes_free: st.f_ffree as u64,
    })
}

const REAL_FS: [&str; 18] = [
    "ext2",
    "ext3",
    "ext4",
    "xfs",
    "btrfs",
    "zfs",
    "vfat",
    "exfat",
    "f2fs",
    "jfs",
    "reiserfs",
    "ntfs",
    "nfs",
    "nfs4",
    "cifs",
    "smb3",
    "ceph",
    "fuse.glusterfs",
];

#[derive(Debug, PartialEq)]
pub(crate) struct Mount {
    pub point: String,
    pub fstype: String,
    pub device: String,
}

pub(crate) fn parse_mountinfo(lines: &[String]) -> Vec<Mount> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for l in lines {
        let Some((pre, post)) = l.split_once(" - ") else {
            continue;
        };
        let a: Vec<&str> = pre.split_whitespace().collect();
        let b: Vec<&str> = post.split_whitespace().collect();
        if a.len() < 5 || b.len() < 2 {
            continue;
        }
        let point = unescape_mount(a[4]);
        if !seen.insert(point.clone()) {
            continue;
        }
        out.push(Mount {
            point,
            fstype: b[0].into(),
            device: b[1].into(),
        });
    }
    out
}

fn unescape_mount(s: &str) -> String {
    s.replace("\\040", " ")
        .replace("\\011", "\t")
        .replace("\\012", "\n")
        .replace("\\134", "\\")
}

/// Space and inode usage from mountinfo plus statvfs.
pub struct Filesystems {
    fs: Fsys,
    root: Option<PathBuf>,
    statfs: fn(&std::path::Path) -> Result<FsUsage, String>,
}

impl Filesystems {
    pub fn new(fs: Fsys, root: Option<PathBuf>) -> Self {
        Self {
            fs,
            root,
            statfs: statvfs,
        }
    }
}

impl Collector for Filesystems {
    fn info(&self) -> Info {
        Info::new("diskspace", "disk", 10)
    }

    fn collect(&mut self, _now: i64, e: &mut Emitter) -> Result<(), String> {
        let lines = read_lines(&self.fs.proc("1/mountinfo"))
            .or_else(|_| read_lines(&self.fs.proc("self/mountinfo")))?;
        let mut errs = Vec::new();
        let start = e.samples().len();
        for m in parse_mountinfo(&lines) {
            if !REAL_FS.contains(&m.fstype.as_str())
                || m.point.starts_with("/var/lib/kubelet/pods/")
                || m.point.contains("/containers/storage/overlay")
            {
                continue;
            }
            let path = match &self.root {
                Some(r) => r.join(m.point.trim_start_matches('/')),
                None => PathBuf::from(&m.point),
            };
            let u = match (self.statfs)(&path) {
                Ok(u) => u,
                Err(err) => {
                    errs.push(err);
                    continue;
                }
            };
            if u.blocks == 0 {
                continue;
            }
            let bs = u.block_size as f64;
            const GIB: f64 = (1u64 << 30) as f64;
            let lbl = labels([
                ("mount_point", m.point.as_str()),
                ("filesystem", m.fstype.as_str()),
                ("device", m.device.as_str()),
            ]);
            let mk = |ctx: &str, id: &str, units: &str, title: &str| {
                Chart::new(ctx, "disk", units, title)
                    .id(format!("{id}{}", m.point))
                    .labels(&lbl)
            };
            let sp = mk("disk.space", "disk_space_", "GiB", "Disk space usage").ty("stacked");
            e.gauge(&sp, "avail", u.avail as f64 * bs / GIB);
            e.gauge(
                &sp,
                "used",
                u.blocks.saturating_sub(u.free) as f64 * bs / GIB,
            );
            e.gauge(
                &sp,
                "reserved_for_root",
                u.free.saturating_sub(u.avail) as f64 * bs / GIB,
            );
            let usable = (u.blocks.saturating_sub(u.free) + u.avail) as f64;
            if usable > 0.0 {
                e.gauge(
                    &mk(
                        "disk.space_utilization",
                        "disk_space_utilization_",
                        "%",
                        "Disk space utilization",
                    ),
                    "used",
                    u.blocks.saturating_sub(u.free) as f64 / usable * 100.0,
                );
            }
            if u.inodes > 0 {
                let used = u.inodes.saturating_sub(u.inodes_free) as f64;
                let inodes =
                    mk("disk.inodes", "disk_inodes_", "inodes", "Disk inode usage").ty("stacked");
                e.gauge(&inodes, "avail", u.inodes_free as f64);
                e.gauge(&inodes, "used", used);
                e.gauge(
                    &mk(
                        "disk.inodes_utilization",
                        "disk_inodes_utilization_",
                        "%",
                        "Disk inode utilization",
                    ),
                    "used",
                    used / u.inodes as f64 * 100.0,
                );
            }
        }
        if e.samples().len() == start && !errs.is_empty() {
            return Err(errs.join("; "));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::testutil::*;
    use super::*;

    #[test]
    fn mountinfo_unescapes_and_dedups() {
        let lines: Vec<String> = [
            "22 1 8:1 / / rw - ext4 /dev/sda1 rw",
            "25 22 8:2 / /var/lib\\040data rw - xfs /dev/sda2 rw",
            "26 22 8:2 / /var/lib\\040data rw - xfs /dev/sda2 rw",
        ]
        .map(String::from)
        .to_vec();
        let m = parse_mountinfo(&lines);
        assert_eq!(m.len(), 2);
        assert_eq!(m[1].point, "/var/lib data");
    }

    #[test]
    fn filesystems_report_real_mounts_only() {
        let fx = Fixture::new("host");
        let mut f = Filesystems::new(fx.fs(), Some("/host".into()));
        f.statfs = |p| {
            assert!(p.starts_with("/host"), "{}", p.display());
            Ok(FsUsage {
                block_size: 4096,
                blocks: 1000,
                free: 400,
                avail: 300,
                inodes: 100,
                inodes_free: 25,
            })
        };
        let got = Runs::new().run(&mut f, 1000);
        got.want("disk_space_utilization_//used", 600.0 / 900.0 * 100.0);
        got.want("disk_inodes_utilization_/var/lib data/used", 75.0);
        assert!(got
            .keys()
            .all(|k| !k.contains("/proc") && !k.contains("/run")));
    }
}
