use super::host::emit_pressure;
use super::{
    key_value_file, pf, read_float, read_lines, read_trim, Chart, Collector, Emitter, Get, Info,
};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

/// One pod or container cgroup with its Kubernetes identity.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Workload {
    pub namespace: String,
    pub pod: String,
    /// Container ID; shortened in labels. Empty for a pod cgroup.
    pub container: String,
    pub workload_kind: String,
    pub workload_name: String,
    /// Absolute cgroup v2 directory.
    pub cgroup_path: PathBuf,
}

/// A pod cgroup found under the kubelet's cgroup tree.
#[derive(Debug, Clone, PartialEq)]
pub struct PodCgroup {
    pub uid: String,
    pub path: PathBuf,
    pub containers: Vec<(String, PathBuf)>,
}

/// Pod UID from a pod cgroup directory name, for both cgroup drivers:
/// `kubepods-burstable-pod<uid_with_underscores>.slice` (systemd) and
/// `pod<uid>` (cgroupfs).
fn pod_uid(name: &str) -> Option<String> {
    let name = name.strip_suffix(".slice").unwrap_or(name);
    let i = name.rfind("pod")?;
    let uid = &name[i + 3..];
    (uid.len() >= 32
        && uid
            .chars()
            .all(|c| c.is_ascii_hexdigit() || c == '_' || c == '-'))
    .then(|| uid.replace('_', "-"))
}

/// Container ID from a container cgroup directory name such as
/// `cri-containerd-<id>.scope`, `crio-<id>.scope`, `docker-<id>.scope` or a
/// bare `<id>`.
fn container_id(name: &str) -> Option<String> {
    let name = name.strip_suffix(".scope").unwrap_or(name);
    let id = name.rsplit('-').next()?;
    (id.len() >= 32 && id.chars().all(|c| c.is_ascii_hexdigit())).then(|| id.to_string())
}

/// Every pod cgroup under `root` (the cgroup v2 mount), at most three levels
/// below `kubepods.slice` or `kubepods`.
pub fn discover_pod_cgroups(root: &Path) -> Vec<PodCgroup> {
    fn walk(dir: &Path, depth: usize, out: &mut Vec<PodCgroup>) {
        let Ok(rd) = std::fs::read_dir(dir) else {
            return;
        };
        for e in rd.flatten() {
            if !e.file_type().is_ok_and(|t| t.is_dir()) {
                continue;
            }
            let name = e.file_name().to_string_lossy().into_owned();
            if let Some(uid) = pod_uid(&name) {
                let mut containers: Vec<(String, PathBuf)> = std::fs::read_dir(e.path())
                    .into_iter()
                    .flatten()
                    .flatten()
                    .filter(|c| c.file_type().is_ok_and(|t| t.is_dir()))
                    .filter_map(|c| {
                        Some((container_id(&c.file_name().to_string_lossy())?, c.path()))
                    })
                    .collect();
                containers.sort();
                out.push(PodCgroup {
                    uid,
                    path: e.path(),
                    containers,
                });
            } else if depth < 3 && name.starts_with("kubepods") {
                walk(&e.path(), depth + 1, out);
            }
        }
    }
    let mut out = Vec::new();
    for top in ["kubepods.slice", "kubepods"] {
        let p = root.join(top);
        if p.is_dir() {
            walk(&p, 0, &mut out);
        }
    }
    out.sort_by(|a, b| a.uid.cmp(&b.uid));
    out
}

/// Keeps one entry per pod: the pod cgroup when listed, otherwise the parent
/// of a container cgroup when that parent is a pod cgroup.
pub fn pod_level(ws: Vec<Workload>) -> Vec<Workload> {
    let mut out: Vec<Workload> = Vec::with_capacity(ws.len());
    let mut idx: HashMap<(String, String), usize> = HashMap::new();
    for mut w in ws {
        if w.pod.is_empty() {
            out.push(w);
            continue;
        }
        if !w.container.is_empty() {
            if let Some(parent) = w.cgroup_path.parent() {
                if parent
                    .file_name()
                    .is_some_and(|n| n.to_string_lossy().contains("pod"))
                {
                    w.cgroup_path = parent.to_path_buf();
                    w.container.clear();
                }
            }
        }
        let k = (w.namespace.clone(), w.pod.clone());
        match idx.get(&k) {
            None => {
                idx.insert(k, out.len());
                out.push(w);
            }
            Some(&i) if !out[i].container.is_empty() && w.container.is_empty() => out[i] = w,
            _ => {}
        }
    }
    out
}

fn short_id(id: &str) -> &str {
    let id = id.rsplit("://").next().unwrap_or(id);
    &id[..id.len().min(12)]
}

struct CgPrev {
    at: i64,
    user: f64,
    sys: f64,
}

pub type WorkloadList = Box<dyn Fn() -> Vec<Workload> + Send>;

/// cgroup v2 CPU, memory, I/O and pressure per workload. By default each pod
/// is charted once from its pod cgroup, whose v2 stats already include every
/// container; `containers` charts each container cgroup as well.
pub struct Cgroups {
    list: WorkloadList,
    containers: bool,
    prev: HashMap<String, CgPrev>,
}

impl Cgroups {
    pub fn new(list: WorkloadList, containers: bool) -> Self {
        Self {
            list,
            containers,
            prev: HashMap::new(),
        }
    }
}

impl Collector for Cgroups {
    fn info(&self) -> Info {
        Info::new("cgroups", "cgroups", 1)
    }

    fn collect(&mut self, now: i64, e: &mut Emitter) -> Result<(), String> {
        let mut ws = (self.list)();
        if !self.containers {
            ws = pod_level(ws);
        }
        let mut seen = HashSet::new();
        let mut errs = 0;
        for w in &ws {
            if w.cgroup_path.as_os_str().is_empty() {
                continue;
            }
            let p = |f: &str| w.cgroup_path.join(f);
            let Ok(cpu) = key_value_file(&p("cpu.stat")) else {
                errs += 1;
                continue;
            };
            let mut id = format!("{}_{}", w.namespace, w.pod);
            let mut lbl = BTreeMap::from([
                ("namespace".to_string(), w.namespace.clone()),
                ("pod".to_string(), w.pod.clone()),
            ]);
            if !w.container.is_empty() {
                id = format!("{id}_{}", short_id(&w.container));
                lbl.insert("container_id".into(), short_id(&w.container).into());
            }
            if !w.workload_kind.is_empty() {
                lbl.insert("workload_kind".into(), w.workload_kind.clone());
                lbl.insert("workload".into(), w.workload_name.clone());
            }
            seen.insert(id.clone());
            let mk = |ctx: &str, fam: &str, units: &str, title: &str| {
                Chart::new(ctx, fam, units, title)
                    .id(format!("cgroup_{id}.{}", &ctx["cgroup.".len()..]))
                    .labels(&lbl)
            };
            let (user, sys) = (cpu.g("user_usec"), cpu.g("system_usec"));
            if let Some(pv) = self.prev.get(&id) {
                let dt = (now - pv.at) as f64;
                if dt > 0.0 && user >= pv.user && sys >= pv.sys {
                    let ch = mk(
                        "cgroup.cpu",
                        "cpu",
                        "%",
                        "Workload CPU usage (100% = 1 core)",
                    )
                    .ty("stacked");
                    e.gauge(&ch, "user", (user - pv.user) / 1e6 / dt * 100.0);
                    e.gauge(&ch, "system", (sys - pv.sys) / 1e6 / dt * 100.0);
                }
            }
            self.prev.insert(id.clone(), CgPrev { at: now, user, sys });
            if cpu.g("nr_periods") > 0.0 {
                e.incremental(
                    &mk(
                        "cgroup.throttled",
                        "cpu",
                        "periods/s",
                        "Workload CPU throttled periods",
                    ),
                    "throttled_periods",
                    cpu.g("nr_throttled"),
                    1.0,
                );
            }
            e.incremental(
                &mk(
                    "cgroup.throttled_duration",
                    "cpu",
                    "ms",
                    "Workload CPU throttled time",
                ),
                "duration",
                cpu.g("throttled_usec"),
                0.001,
            );
            const MIB: f64 = (1u64 << 20) as f64;
            if let Some(cur) = read_float(&p("memory.current")) {
                e.gauge(
                    &mk("cgroup.mem_usage", "mem", "MiB", "Workload memory usage"),
                    "ram",
                    cur / MIB,
                );
                if let Some(mx) = read_trim(&p("memory.max")).filter(|m| m != "max") {
                    let lim = pf(&mx);
                    if lim > 0.0 {
                        e.gauge(
                            &mk(
                                "cgroup.mem_utilization",
                                "mem",
                                "%",
                                "Workload memory utilization of limit",
                            ),
                            "utilization",
                            cur / lim * 100.0,
                        );
                    }
                }
                if let Ok(ms) = key_value_file(&p("memory.stat")) {
                    let m =
                        mk("cgroup.mem", "mem", "MiB", "Workload memory breakdown").ty("stacked");
                    for k in ["anon", "file", "kernel", "sock"] {
                        e.gauge(&m, k, ms.g(k) / MIB);
                    }
                    let pg = mk("cgroup.pgfaults", "mem", "faults/s", "Workload page faults");
                    e.incremental(&pg, "faults", ms.g("pgfault"), 1.0);
                    e.incremental(&pg, "major", ms.g("pgmajfault"), 1.0);
                }
            }
            if let Ok(ev) = key_value_file(&p("memory.events")) {
                let oe = mk(
                    "cgroup.mem_events",
                    "mem",
                    "events/s",
                    "Workload memory events",
                );
                for k in ["oom", "oom_kill", "max"] {
                    e.incremental(&oe, k, ev.g(k), 1.0);
                }
            }
            if let Ok(lines) = read_lines(&p("io.stat")) {
                let mut t = HashMap::<&str, f64>::new();
                for l in &lines {
                    for kv in l.split_whitespace().skip(1) {
                        if let Some((k, v)) = kv.split_once('=') {
                            if let Some(slot) = ["rbytes", "wbytes", "rios", "wios"]
                                .iter()
                                .find(|x| **x == k)
                            {
                                *t.entry(slot).or_default() += pf(v);
                            }
                        }
                    }
                }
                let g = |k: &str| t.get(k).copied().unwrap_or(0.0);
                let io = mk("cgroup.io", "disk", "KiB/s", "Workload I/O bandwidth");
                e.incremental(&io, "read", g("rbytes"), 1.0 / 1024.0);
                e.incremental(&io, "write", g("wbytes"), 1.0 / 1024.0);
                let ops = mk(
                    "cgroup.serviced_ops",
                    "disk",
                    "operations/s",
                    "Workload I/O operations",
                );
                e.incremental(&ops, "read", g("rios"), 1.0);
                e.incremental(&ops, "write", g("wios"), 1.0);
            }
            for res in ["cpu", "memory", "io"] {
                if let Ok(lines) = read_lines(&p(&format!("{res}.pressure"))) {
                    emit_pressure(e, "cgroup", res, Some((&id, &lbl)), &lines);
                }
            }
        }
        self.prev.retain(|id, _| seen.contains(id));
        if seen.is_empty() && errs > 0 {
            return Err("no workload cgroup was readable".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::testutil::*;
    use super::*;

    #[test]
    fn discovers_pods_for_both_drivers() {
        let d = tempfile::tempdir().unwrap();
        let uid = "0f1e2d3c_4b5a_6978_8796_a5b4c3d2e1f0";
        let ctr = "a".repeat(64);
        let sd = d.path().join(format!(
            "kubepods.slice/kubepods-burstable.slice/kubepods-burstable-pod{uid}.slice/cri-containerd-{ctr}.scope"
        ));
        std::fs::create_dir_all(&sd).unwrap();
        let cg = d
            .path()
            .join("kubepods/pod11111111-2222-3333-4444-555555555555");
        std::fs::create_dir_all(cg.join("b".repeat(64))).unwrap();
        std::fs::create_dir_all(d.path().join("system.slice/podman.service")).unwrap();
        let pods = discover_pod_cgroups(d.path());
        assert_eq!(pods.len(), 2, "{pods:?}");
        assert_eq!(pods[0].uid, "0f1e2d3c-4b5a-6978-8796-a5b4c3d2e1f0");
        assert_eq!(pods[0].containers[0].0, ctr);
        assert_eq!(pods[1].uid, "11111111-2222-3333-4444-555555555555");
        assert_eq!(pods[1].containers.len(), 1);
    }

    #[test]
    fn pod_level_collapses_containers() {
        let ws = vec![
            Workload {
                namespace: "a".into(),
                pod: "p".into(),
                container: "c1".into(),
                cgroup_path: "/x/pod1/c1".into(),
                ..Default::default()
            },
            Workload {
                namespace: "a".into(),
                pod: "p".into(),
                container: "c2".into(),
                cgroup_path: "/x/pod1/c2".into(),
                ..Default::default()
            },
            Workload {
                namespace: "a".into(),
                pod: "q".into(),
                container: "c3".into(),
                cgroup_path: "/x/other/c3".into(),
                ..Default::default()
            },
        ];
        let got = pod_level(ws);
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].cgroup_path, Path::new("/x/pod1"));
        assert!(got[0].container.is_empty());
        assert_eq!(got[1].container, "c3");
    }

    #[test]
    fn cgroup_cpu_memory_and_pressure() {
        let fx = Fixture::new("cgroup");
        let path = fx.path("pod1");
        let mut c = Cgroups::new(
            Box::new(move || {
                vec![Workload {
                    namespace: "default".into(),
                    pod: "web-1".into(),
                    workload_kind: "Deployment".into(),
                    workload_name: "web".into(),
                    cgroup_path: path.clone(),
                    ..Default::default()
                }]
            }),
            false,
        );
        let mut r = Runs::new();
        r.run(&mut c, 1000);
        let cur = std::fs::read_to_string(fx.path("pod1/cpu.stat")).unwrap();
        let user: f64 = cur
            .lines()
            .find(|l| l.starts_with("user_usec"))
            .unwrap()
            .split_whitespace()
            .nth(1)
            .unwrap()
            .parse()
            .unwrap();
        fx.rewrite(
            "pod1/cpu.stat",
            &format!("user_usec {user}"),
            &format!("user_usec {}", user + 500_000.0),
        );
        let got = r.run(&mut c, 1001);
        got.want("cgroup_default_web-1.cpu/user", 50.0);
        assert!(got.has("cgroup_default_web-1.mem_usage/ram"));
        assert!(
            got.has("cgroup.cpu_some_pressure_default_web-1/avg10"),
            "{:?}",
            got.keys().collect::<Vec<_>>()
        );
    }
}
