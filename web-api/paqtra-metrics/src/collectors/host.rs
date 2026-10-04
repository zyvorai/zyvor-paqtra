use super::{
    key_value_file, labels, pf, read_float, read_lines, read_trim, Chart, Collector, Emitter, Fsys,
    Get, Info,
};
use std::collections::{BTreeMap, HashMap};

const CPU_DIMS: [&str; 10] = [
    "user",
    "nice",
    "system",
    "idle",
    "iowait",
    "irq",
    "softirq",
    "steal",
    "guest",
    "guest_nice",
];

/// /proc/stat and /proc/softirqs.
pub struct Cpu {
    fs: Fsys,
    prev: HashMap<String, [f64; 10]>,
}

impl Cpu {
    pub fn new(fs: Fsys) -> Self {
        Self {
            fs,
            prev: HashMap::new(),
        }
    }
}

impl Collector for Cpu {
    fn info(&self) -> Info {
        Info::new("proc.stat", "cpu", 1)
    }

    fn collect(&mut self, _now: i64, e: &mut Emitter) -> Result<(), String> {
        let lines = read_lines(&self.fs.proc("stat"))?;
        let procs = Chart::new(
            "system.processes",
            "processes",
            "processes",
            "System processes",
        );
        for l in &lines {
            let f: Vec<&str> = l.split_whitespace().collect();
            if f.len() < 2 {
                continue;
            }
            match f[0] {
                name if name.starts_with("cpu") => {
                    let mut vals = [0.0; 10];
                    for (i, v) in vals.iter_mut().enumerate() {
                        if let Some(s) = f.get(i + 1) {
                            *v = pf(s);
                        }
                    }
                    // Guest time is already included in user and nice.
                    vals[0] -= vals[8];
                    vals[1] -= vals[9];
                    let Some(prev) = self.prev.insert(name.to_string(), vals) else {
                        continue;
                    };
                    let deltas: Vec<f64> = vals
                        .iter()
                        .zip(prev)
                        .map(|(v, p)| (v - p).max(0.0))
                        .collect();
                    let total: f64 = deltas.iter().sum();
                    if total <= 0.0 {
                        continue;
                    }
                    let ch = if name == "cpu" {
                        Chart::new("system.cpu", "cpu", "%", "Total CPU utilization").ty("stacked")
                    } else {
                        let core = &name[3..];
                        Chart::new("cpu.cpu", "cpu", "%", "Core utilization")
                            .id(format!("cpu.cpu{core}"))
                            .ty("stacked")
                            .labels(&labels([("cpu", core)]))
                    };
                    for (i, d) in CPU_DIMS.iter().enumerate() {
                        if *d != "idle" {
                            e.gauge(&ch, d, deltas[i] / total * 100.0);
                        }
                    }
                }
                "intr" => e.incremental(
                    &Chart::new("system.intr", "cpu", "interrupts/s", "CPU interrupts"),
                    "interrupts",
                    pf(f[1]),
                    1.0,
                ),
                "ctxt" => e.incremental(
                    &Chart::new(
                        "system.ctxt",
                        "cpu",
                        "context switches/s",
                        "CPU context switches",
                    ),
                    "switches",
                    pf(f[1]),
                    1.0,
                ),
                "processes" => e.incremental(
                    &Chart::new(
                        "system.forks",
                        "processes",
                        "processes/s",
                        "Started processes",
                    ),
                    "started",
                    pf(f[1]),
                    1.0,
                ),
                "procs_running" => e.gauge(&procs, "running", pf(f[1])),
                "procs_blocked" => e.gauge(&procs, "blocked", pf(f[1])),
                _ => {}
            }
        }
        if let Ok(sl) = read_lines(&self.fs.proc("softirqs")) {
            let ch =
                Chart::new("system.softirqs", "cpu", "softirqs/s", "System softirqs").ty("stacked");
            for l in sl.iter().skip(1) {
                let f: Vec<&str> = l.split_whitespace().collect();
                if f.len() < 2 {
                    continue;
                }
                let sum: f64 = f[1..].iter().map(|v| pf(v)).sum();
                e.incremental(&ch, &f[0].trim_end_matches(':').to_lowercase(), sum, 1.0);
            }
        }
        Ok(())
    }
}

/// /proc/meminfo and /proc/vmstat.
pub struct Memory {
    fs: Fsys,
}

impl Memory {
    pub fn new(fs: Fsys) -> Self {
        Self { fs }
    }
}

impl Collector for Memory {
    fn info(&self) -> Info {
        Info::new("proc.meminfo", "mem", 1)
    }

    fn collect(&mut self, _now: i64, e: &mut Emitter) -> Result<(), String> {
        let mi = key_value_file(&self.fs.proc("meminfo"))?;
        const MIB: f64 = 1.0 / 1024.0; // meminfo is KiB
        let (total, free) = (mi.g("MemTotal"), mi.g("MemFree"));
        let buffers = mi.g("Buffers");
        let cached = mi.g("Cached") + mi.g("SReclaimable") - mi.g("Shmem");
        let used = total - free - buffers - cached;
        let ram = Chart::new("system.ram", "ram", "MiB", "System RAM").ty("stacked");
        e.gauge(&ram, "free", free * MIB);
        e.gauge(&ram, "used", used.max(0.0) * MIB);
        e.gauge(&ram, "cached", cached.max(0.0) * MIB);
        e.gauge(&ram, "buffers", buffers * MIB);
        if let Some(&avail) = mi.get("MemAvailable") {
            e.gauge(
                &Chart::new("mem.available", "ram", "MiB", "Available RAM"),
                "avail",
                avail * MIB,
            );
            if total > 0.0 {
                e.gauge(
                    &Chart::new(
                        "mem.used_percent",
                        "ram",
                        "%",
                        "RAM used (excluding reclaimable)",
                    ),
                    "used",
                    (total - avail) / total * 100.0,
                );
            }
        }
        let st = mi.g("SwapTotal");
        if st > 0.0 {
            let sw = Chart::new("mem.swap", "swap", "MiB", "System swap").ty("stacked");
            e.gauge(&sw, "free", mi.g("SwapFree") * MIB);
            e.gauge(&sw, "used", (st - mi.g("SwapFree")) * MIB);
        }
        e.gauge(
            &Chart::new("mem.committed", "ram", "MiB", "Committed memory"),
            "committed_as",
            mi.g("Committed_AS") * MIB,
        );
        let k =
            Chart::new("mem.kernel", "kernel", "MiB", "Memory used by the kernel").ty("stacked");
        e.gauge(&k, "slab", mi.g("Slab") * MIB);
        e.gauge(&k, "kernel_stack", mi.g("KernelStack") * MIB);
        e.gauge(&k, "page_tables", mi.g("PageTables") * MIB);
        e.gauge(&k, "vmalloc_used", mi.g("VmallocUsed") * MIB);
        let wb = Chart::new("mem.writeback", "kernel", "MiB", "Writeback memory");
        e.gauge(&wb, "dirty", mi.g("Dirty") * MIB);
        e.gauge(&wb, "writeback", mi.g("Writeback") * MIB);
        let hp = mi.g("HugePages_Total");
        if hp > 0.0 {
            let h = Chart::new("mem.hugepages", "hugepages", "pages", "Huge pages").ty("stacked");
            e.gauge(&h, "free", mi.g("HugePages_Free"));
            e.gauge(&h, "used", hp - mi.g("HugePages_Free"));
        }
        let Ok(vm) = key_value_file(&self.fs.proc("vmstat")) else {
            return Ok(());
        };
        let pg = Chart::new("mem.pgfaults", "ram", "faults/s", "Memory page faults");
        e.incremental(&pg, "minor", vm.g("pgfault") - vm.g("pgmajfault"), 1.0);
        e.incremental(&pg, "major", vm.g("pgmajfault"), 1.0);
        let io = Chart::new(
            "system.pgpgio",
            "disk",
            "KiB/s",
            "Memory paged from/to disk",
        );
        e.incremental(&io, "in", vm.g("pgpgin"), 1.0);
        e.incremental(&io, "out", vm.g("pgpgout"), 1.0);
        let sio = Chart::new("mem.swapio", "swap", "KiB/s", "Swap I/O");
        e.incremental(&sio, "in", vm.g("pswpin"), 4.0);
        e.incremental(&sio, "out", vm.g("pswpout"), 4.0);
        if let Some(&v) = vm.get("oom_kill") {
            e.incremental(
                &Chart::new("mem.oom_kill", "ram", "kills/s", "Out of memory kills"),
                "kills",
                v,
                1.0,
            );
        }
        Ok(())
    }
}

/// Pressure stall information from /proc/pressure.
pub struct Pressure {
    fs: Fsys,
}

impl Pressure {
    pub fn new(fs: Fsys) -> Self {
        Self { fs }
    }
}

impl Collector for Pressure {
    fn info(&self) -> Info {
        Info::new("proc.pressure", "pressure", 1)
    }

    fn collect(&mut self, _now: i64, e: &mut Emitter) -> Result<(), String> {
        let mut found = false;
        for res in ["cpu", "memory", "io"] {
            if let Ok(lines) = read_lines(&self.fs.proc(&format!("pressure/{res}"))) {
                found = true;
                emit_pressure(e, "system", res, None, &lines);
            }
        }
        if found {
            Ok(())
        } else {
            Err("pressure stall information not available".into())
        }
    }
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    c.next()
        .map(|f| f.to_uppercase().chain(c).collect())
        .unwrap_or_default()
}

/// The some/full avg10/60/300 gauges and stall time rate for one resource.
/// `scope` is a (chart id suffix, labels) pair for cgroups.
pub(crate) fn emit_pressure(
    e: &mut Emitter,
    prefix: &str,
    res: &str,
    scope: Option<(&str, &BTreeMap<String, String>)>,
    lines: &[String],
) {
    let (suffix, lbl) = match scope {
        Some((id, l)) => (format!("_{id}"), l.clone()),
        None => (String::new(), BTreeMap::new()),
    };
    for l in lines {
        let f: Vec<&str> = l.split_whitespace().collect();
        if f.len() < 5 {
            continue;
        }
        let kind = f[0];
        let ctx = format!("{prefix}.{res}_{kind}_pressure");
        let ch = Chart::new(
            &ctx,
            "pressure",
            "%",
            &format!("{} {kind} pressure", capitalize(res)),
        )
        .id(format!("{ctx}{suffix}"))
        .labels(&lbl);
        for kv in &f[1..] {
            let Some((k, v)) = kv.split_once('=') else {
                continue;
            };
            match k {
                "avg10" | "avg60" | "avg300" => e.gauge(&ch, k, pf(v)),
                "total" => {
                    let sctx = format!("{ctx}_stall_time");
                    let st = Chart::new(
                        &sctx,
                        "pressure",
                        "ms",
                        &format!("{} {kind} stall time", capitalize(res)),
                    )
                    .id(format!("{sctx}{suffix}"))
                    .labels(&lbl);
                    e.incremental(&st, "time", pf(v), 0.001); // microseconds
                }
                _ => {}
            }
        }
    }
}

struct DiskPrev {
    at: i64,
    ios: f64,
    ticks: f64,
    weighted: f64,
}

/// /proc/diskstats for whole block devices.
pub struct Disks {
    fs: Fsys,
    prev: HashMap<String, DiskPrev>,
}

impl Disks {
    pub fn new(fs: Fsys) -> Self {
        Self {
            fs,
            prev: HashMap::new(),
        }
    }
}

impl Collector for Disks {
    fn info(&self) -> Info {
        Info::new("proc.diskstats", "disk", 1)
    }

    fn collect(&mut self, now: i64, e: &mut Emitter) -> Result<(), String> {
        let lines = read_lines(&self.fs.proc("diskstats"))?;
        let (mut total_r, mut total_w) = (0.0, 0.0);
        for l in &lines {
            let f: Vec<&str> = l.split_whitespace().collect();
            if f.len() < 14 {
                continue;
            }
            let dev = f[2];
            if ["ram", "loop", "zram"].iter().any(|p| dev.starts_with(p)) {
                continue;
            }
            if !self.fs.sys(&format!("block/{dev}")).exists() {
                continue; // partition or not a block device on this host
            }
            let (reads, read_sect) = (pf(f[3]), pf(f[5]));
            let (writes, write_sect) = (pf(f[7]), pf(f[9]));
            let (inflight, io_ms, weighted_ms) = (pf(f[11]), pf(f[12]), pf(f[13]));
            let lbl = labels([("device", dev)]);
            let mk = |ctx: &str, units: &str, title: &str| {
                Chart::new(ctx, "disk", units, title)
                    .id(format!("{ctx}_{dev}"))
                    .labels(&lbl)
            };
            let io = mk("disk.io", "KiB/s", "Disk I/O bandwidth");
            e.incremental(&io, "reads", read_sect, 0.5);
            e.incremental(&io, "writes", write_sect, 0.5);
            let ops = mk("disk.ops", "operations/s", "Disk completed I/O operations");
            e.incremental(&ops, "reads", reads, 1.0);
            e.incremental(&ops, "writes", writes, 1.0);
            e.gauge(
                &mk("disk.qops", "operations", "Disk current I/O operations"),
                "operations",
                inflight,
            );
            let ios = reads + writes;
            if let Some(p) = self.prev.get(dev) {
                let dt = (now - p.at) as f64 * 1000.0;
                if dt > 0.0 && io_ms >= p.ticks {
                    e.gauge(
                        &mk("disk.util", "%", "Disk utilization time"),
                        "utilization",
                        ((io_ms - p.ticks) / dt * 100.0).min(100.0),
                    );
                    e.gauge(
                        &mk("disk.backlog", "milliseconds", "Disk backlog"),
                        "backlog",
                        (weighted_ms - p.weighted).max(0.0) / dt * 1000.0,
                    );
                }
                let dio = ios - p.ios;
                if dio > 0.0 {
                    e.gauge(
                        &mk(
                            "disk.await",
                            "milliseconds/operation",
                            "Average completed I/O operation time",
                        ),
                        "await",
                        (weighted_ms - p.weighted).max(0.0) / dio,
                    );
                }
            }
            self.prev.insert(
                dev.to_string(),
                DiskPrev {
                    at: now,
                    ios,
                    ticks: io_ms,
                    weighted: weighted_ms,
                },
            );
            total_r += read_sect;
            total_w += write_sect;
        }
        let sys = Chart::new("system.io", "disk", "KiB/s", "Disk I/O");
        e.incremental(&sys, "in", total_r, 0.5);
        e.incremental(&sys, "out", total_w, 0.5);
        Ok(())
    }
}

/// Load average, uptime, entropy and file handles.
pub struct System {
    fs: Fsys,
}

impl System {
    pub fn new(fs: Fsys) -> Self {
        Self { fs }
    }
}

impl Collector for System {
    fn info(&self) -> Info {
        Info::new("proc.system", "system", 1)
    }

    fn collect(&mut self, _now: i64, e: &mut Emitter) -> Result<(), String> {
        let la = read_trim(&self.fs.proc("loadavg")).ok_or("loadavg not readable")?;
        let f: Vec<&str> = la.split_whitespace().collect();
        if f.len() >= 3 {
            let ch = Chart::new("system.load", "load", "load", "System load average");
            e.gauge(&ch, "load1", pf(f[0]));
            e.gauge(&ch, "load5", pf(f[1]));
            e.gauge(&ch, "load15", pf(f[2]));
        }
        if let Some(up) = read_trim(&self.fs.proc("uptime")) {
            if let Some(v) = up.split_whitespace().next() {
                e.gauge(
                    &Chart::new("system.uptime", "uptime", "seconds", "System uptime"),
                    "uptime",
                    pf(v),
                );
            }
        }
        if let Some(v) = read_float(&self.fs.proc("sys/kernel/random/entropy_avail")) {
            e.gauge(
                &Chart::new("system.entropy", "entropy", "entropy", "Available entropy"),
                "entropy",
                v,
            );
        }
        if let Some(fnr) = read_trim(&self.fs.proc("sys/fs/file-nr")) {
            let f: Vec<f64> = fnr.split_whitespace().map(pf).collect();
            if f.len() >= 3 {
                let used = f[0] - f[1];
                e.gauge(
                    &Chart::new("system.file_nr_used", "files", "files", "File descriptors"),
                    "used",
                    used,
                );
                if f[2] > 0.0 {
                    e.gauge(
                        &Chart::new(
                            "system.file_nr_utilization",
                            "files",
                            "%",
                            "File descriptor utilization",
                        ),
                        "used",
                        used / f[2] * 100.0,
                    );
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::testutil::*;
    use super::*;

    #[test]
    fn cpu_utilization_from_deltas() {
        let fx = Fixture::new("host");
        let mut c = Cpu::new(fx.fs());
        let mut r = Runs::new();
        r.run(&mut c, 1000);
        fx.rewrite(
            "proc/stat",
            "cpu  1000 10 500 8000",
            "cpu  1100 10 550 8350",
        );
        let got = r.run(&mut c, 1001);
        // deltas user 100, system 50, idle 350 => 500 total
        got.want("system.cpu/user", 20.0);
        got.want("system.cpu/system", 10.0);
        assert!(!got.has("system.cpu/idle"));
    }

    #[test]
    fn memory_gauges() {
        let fx = Fixture::new("host");
        let got = Runs::new().run(&mut Memory::new(fx.fs()), 1000);
        assert!(got.has("system.ram/used"));
        assert!(got.has("mem.available/avail"));
    }

    #[test]
    fn pressure_and_system() {
        let fx = Fixture::new("host");
        let got = Runs::new().run(&mut Pressure::new(fx.fs()), 1000);
        assert!(got.has("system.cpu_some_pressure/avg10"));
        let got = Runs::new().run(&mut System::new(fx.fs()), 1000);
        assert!(got.has("system.load/load1"));
        assert!(got.has("system.file_nr_utilization/used"));
    }

    #[test]
    fn disks_only_whole_devices() {
        let fx = Fixture::new("host");
        let mut d = Disks::new(fx.fs());
        let mut r = Runs::new();
        let first = r.run(&mut d, 1000);
        assert!(first.has("disk.qops_sda/operations"));
        assert!(first.keys().all(|k| !k.contains("sda1")), "{first:?}");
    }
}
