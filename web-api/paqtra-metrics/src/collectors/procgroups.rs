use super::{key_value_file, labels, pf, Chart, Collector, Emitter, Fsys, Get, Info};
use std::collections::{HashMap, HashSet};

/// USER_HZ, 100 on every mainstream Linux build.
const CLOCK_TICKS: f64 = 100.0;

struct ProcPrev {
    start: String,
    at: i64,
    utime: f64,
    stime: f64,
    minflt: f64,
    majflt: f64,
    read_bytes: f64,
    write_bytes: f64,
}

#[derive(Default, Clone)]
struct Group {
    user: f64,
    sys: f64,
    rss_mib: f64,
    procs: f64,
    threads: f64,
    read_kib: f64,
    write_kib: f64,
    minflt: f64,
    majflt: f64,
}

impl Group {
    fn add(&mut self, g: &Group) {
        self.user += g.user;
        self.sys += g.sys;
        self.rss_mib += g.rss_mib;
        self.procs += g.procs;
        self.threads += g.threads;
        self.read_kib += g.read_kib;
        self.write_kib += g.write_kib;
        self.minflt += g.minflt;
        self.majflt += g.majflt;
    }
}

/// Per-process CPU, memory, threads, I/O and page faults aggregated by comm,
/// the 15-byte kernel task name. Reads `/proc/<pid>/stat` and `io` only;
/// never `cmdline`, `environ` or anything holding arguments or environment.
/// `io` is skipped for processes the agent may not read.
pub struct ProcessGroups {
    fs: Fsys,
    max: usize,
    prev: HashMap<u32, ProcPrev>,
    keep: HashSet<String>,
    page_mib: f64,
}

impl ProcessGroups {
    pub fn new(fs: Fsys, max: usize) -> Self {
        // SAFETY: sysconf has no preconditions.
        let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
        Self {
            fs,
            max: if max == 0 { 40 } else { max },
            prev: HashMap::new(),
            keep: HashSet::new(),
            page_mib: if page > 0 { page as f64 } else { 4096.0 } / (1u64 << 20) as f64,
        }
    }
}

/// comm and the fields after it from /proc/<pid>/stat.
fn parse_stat(s: &str) -> Option<(&str, Vec<&str>)> {
    let open = s.find('(')?;
    let end = s.rfind(')')?;
    (end > open).then(|| (&s[open + 1..end], s[end + 1..].split_whitespace().collect()))
}

impl Collector for ProcessGroups {
    fn info(&self) -> Info {
        Info::new("apps.groups", "apps", 2)
    }

    fn collect(&mut self, now: i64, e: &mut Emitter) -> Result<(), String> {
        let rd = std::fs::read_dir(&self.fs.proc).map_err(|e| e.to_string())?;
        let mut groups: HashMap<String, Group> = HashMap::new();
        let mut seen = HashSet::new();
        for ent in rd.flatten() {
            let name = ent.file_name();
            let Some(pid) = name.to_str().and_then(|n| n.parse::<u32>().ok()) else {
                continue;
            };
            let Ok(raw) = std::fs::read_to_string(ent.path().join("stat")) else {
                continue;
            };
            // f[0] is field 3 (state); field N is f[N-3].
            let Some((comm, f)) = parse_stat(&raw) else {
                continue;
            };
            if f.len() < 22 || f[0] == "Z" {
                continue;
            }
            let g = groups.entry(comm.to_string()).or_default();
            g.procs += 1.0;
            g.threads += pf(f[17]);
            g.rss_mib += pf(f[21]) * self.page_mib;
            let mut cur = ProcPrev {
                start: f[19].to_string(),
                at: now,
                utime: pf(f[11]),
                stime: pf(f[12]),
                minflt: pf(f[7]),
                majflt: pf(f[9]),
                read_bytes: 0.0,
                write_bytes: 0.0,
            };
            if let Ok(io) = key_value_file(&ent.path().join("io")) {
                cur.read_bytes = io.g("read_bytes");
                cur.write_bytes = io.g("write_bytes");
            }
            seen.insert(pid);
            if let Some(old) = self.prev.get(&pid).filter(|o| o.start == cur.start) {
                let dt = (now - old.at) as f64;
                if dt > 0.0 {
                    let d = |a: f64, b: f64| (a - b).max(0.0) / dt;
                    g.user += d(cur.utime, old.utime) / CLOCK_TICKS * 100.0;
                    g.sys += d(cur.stime, old.stime) / CLOCK_TICKS * 100.0;
                    g.minflt += d(cur.minflt, old.minflt);
                    g.majflt += d(cur.majflt, old.majflt);
                    g.read_kib += d(cur.read_bytes, old.read_bytes) / 1024.0;
                    g.write_kib += d(cur.write_bytes, old.write_bytes) / 1024.0;
                }
            }
            self.prev.insert(pid, cur);
        }
        self.prev.retain(|pid, _| seen.contains(pid));
        // Keep separately charted groups sticky so series do not churn.
        self.keep.retain(|n| groups.contains_key(n));
        let mut names: Vec<&String> = groups.keys().collect();
        names.sort_by(|a, b| {
            let (ga, gb) = (&groups[*a], &groups[*b]);
            let sa = ga.user + ga.sys + ga.rss_mib / 64.0;
            let sb = gb.user + gb.sys + gb.rss_mib / 64.0;
            sb.total_cmp(&sa)
                .then(gb.procs.total_cmp(&ga.procs))
                .then(a.cmp(b))
        });
        for n in names {
            if self.keep.len() >= self.max {
                break;
            }
            self.keep.insert(n.clone());
        }
        let mut other = Group::default();
        for (name, g) in &groups {
            if self.keep.contains(name) {
                emit_group(e, name, g);
            } else {
                other.add(g);
            }
        }
        if other.procs > 0.0 {
            emit_group(e, "other", &other);
        }
        Ok(())
    }
}

fn emit_group(e: &mut Emitter, name: &str, g: &Group) {
    let lbl = labels([("app_group", name)]);
    let mk = |ctx: &str, fam: &str, units: &str, title: &str| {
        Chart::new(ctx, fam, units, title)
            .id(format!("app.{name}_{}", &ctx["app.".len()..]))
            .labels(&lbl)
    };
    let cpu = mk(
        "app.cpu_utilization",
        "cpu",
        "%",
        "Process group CPU (100% = 1 core)",
    )
    .ty("stacked");
    e.gauge(&cpu, "user", g.user);
    e.gauge(&cpu, "system", g.sys);
    e.gauge(
        &mk(
            "app.mem_usage",
            "mem",
            "MiB",
            "Process group resident memory",
        ),
        "rss",
        g.rss_mib,
    );
    e.gauge(
        &mk(
            "app.processes",
            "processes",
            "processes",
            "Process group processes",
        ),
        "processes",
        g.procs,
    );
    e.gauge(
        &mk(
            "app.threads",
            "processes",
            "threads",
            "Process group threads",
        ),
        "threads",
        g.threads,
    );
    let io = mk("app.disk_io", "disk", "KiB/s", "Process group disk I/O");
    e.gauge(&io, "reads", g.read_kib);
    e.gauge(&io, "writes", g.write_kib);
    let pg = mk(
        "app.page_faults",
        "mem",
        "faults/s",
        "Process group page faults",
    );
    e.gauge(&pg, "minor", g.minflt);
    e.gauge(&pg, "major", g.majflt);
}

#[cfg(test)]
mod tests {
    use super::super::testutil::*;
    use super::*;

    #[test]
    fn groups_by_comm_and_folds_overflow() {
        let fx = Fixture::new("procs");
        let fs = Fsys {
            proc: fx.path(""),
            sys: fx.path(""),
        };
        let mut p = ProcessGroups::new(fs, 1);
        let mut r = Runs::new();
        let first = r.run(&mut p, 1000);
        assert!(
            first.has("app.other_processes/processes"),
            "{:?}",
            first.keys().collect::<Vec<_>>()
        );
        fx.rewrite("100/stat", " 300 100 ", " 400 100 ");
        let got = r.run(&mut p, 1001);
        // 100 ticks in 1s at USER_HZ 100 => 100% of one core
        got.want("app.nginx: worker_cpu_utilization/user", 100.0);
        assert_eq!(
            got.keys()
                .filter(|k| k.ends_with("_processes/processes"))
                .count(),
            2
        );
    }

    #[test]
    fn never_reads_cmdline() {
        let src = include_str!("procgroups.rs");
        let needle = ["\"cmd", "line\""].concat();
        assert!(!src.contains(&needle));
        assert!(!src.contains(&["\"env", "iron\""].concat()));
    }
}
