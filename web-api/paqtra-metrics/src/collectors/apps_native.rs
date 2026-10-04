//! Native application collectors: nginx stub_status, Apache mod_status,
//! HAProxy CSV, Redis INFO (RESP) and memcached stats.

use super::apps::{sanitize_id, App, AppConfig, HttpGet};
use super::{pf, Emitter};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpStream, ToSocketAddrs};

pub(crate) struct Nginx(pub AppConfig);

pub(crate) fn parse_nginx_status(s: &str) -> Result<HashMap<String, f64>, String> {
    let lines: Vec<&str> = s.trim().lines().collect();
    if lines.len() < 4 || !lines[0].starts_with("Active connections:") {
        return Err("not an nginx stub_status page".into());
    }
    let mut out = HashMap::new();
    out.insert(
        "active".into(),
        pf(lines[0]["Active connections:".len()..].trim()),
    );
    let f: Vec<&str> = lines[2].split_whitespace().collect();
    if f.len() < 3 {
        return Err("malformed nginx stub_status counters".into());
    }
    out.insert("accepts".into(), pf(f[0]));
    out.insert("handled".into(), pf(f[1]));
    out.insert("requests".into(), pf(f[2]));
    let f: Vec<&str> = lines[3].split_whitespace().collect();
    for kv in f.chunks(2).filter(|c| c.len() == 2) {
        out.insert(kv[0].trim_end_matches(':').to_lowercase(), pf(kv[1]));
    }
    Ok(out)
}

impl App for Nginx {
    fn collect(&mut self, http: &dyn HttpGet, e: &mut Emitter) -> Result<(), String> {
        let c = &self.0;
        let st = parse_nginx_status(&String::from_utf8_lossy(&c.get(http, &c.url)?))?;
        let g = |k: &str| st.get(k).copied().unwrap_or(0.0);
        e.gauge(
            &c.chart(
                "connections",
                "connections",
                "connections",
                "nginx active connections",
                "line",
            ),
            "active",
            g("active"),
        );
        let cs = c.chart(
            "connections_status",
            "connections",
            "connections",
            "nginx connections by state",
            "stacked",
        );
        e.gauge(&cs, "reading", g("reading"));
        e.gauge(&cs, "writing", g("writing"));
        e.gauge(&cs, "idle", g("waiting"));
        let ca = c.chart(
            "connections_accepted_handled",
            "connections",
            "connections/s",
            "nginx accepted and handled connections",
            "line",
        );
        e.incremental(&ca, "accepted", g("accepts"), 1.0);
        e.incremental(&ca, "handled", g("handled"), 1.0);
        e.incremental(
            &c.chart(
                "requests",
                "requests",
                "requests/s",
                "nginx client requests",
                "line",
            ),
            "requests",
            g("requests"),
            1.0,
        );
        Ok(())
    }
}

pub(crate) struct Apache(pub AppConfig);

pub(crate) fn parse_apache_status(s: &str) -> (HashMap<String, f64>, HashMap<&'static str, f64>) {
    let mut st = HashMap::new();
    let mut board = HashMap::new();
    for l in s.lines() {
        let Some((k, v)) = l.split_once(':') else {
            continue;
        };
        let (k, v) = (k.trim(), v.trim());
        if k == "Scoreboard" {
            for c in v.chars() {
                let name = match c {
                    '_' => "waiting",
                    'S' => "starting",
                    'R' => "reading",
                    'W' => "sending",
                    'K' => "keepalive",
                    'D' => "dns_lookup",
                    'C' => "closing",
                    'L' => "logging",
                    'G' => "finishing",
                    'I' => "idle_cleanup",
                    '.' => "open",
                    _ => continue,
                };
                *board.entry(name).or_default() += 1.0;
            }
            continue;
        }
        if let Ok(f) = v.parse::<f64>() {
            st.insert(k.to_string(), f);
        }
    }
    (st, board)
}

impl App for Apache {
    fn collect(&mut self, http: &dyn HttpGet, e: &mut Emitter) -> Result<(), String> {
        let c = &self.0;
        let mut url = c.url.clone();
        if !url.contains("auto") {
            url.push_str(if url.contains('?') { "&auto" } else { "?auto" });
        }
        let (st, board) = parse_apache_status(&String::from_utf8_lossy(&c.get(http, &url)?));
        if !st.contains_key("BusyWorkers") {
            return Err("not an Apache mod_status ?auto page".into());
        }
        if let Some(&v) = st.get("Total Accesses") {
            e.incremental(
                &c.chart(
                    "requests",
                    "requests",
                    "requests/s",
                    "Apache requests",
                    "line",
                ),
                "requests",
                v,
                1.0,
            );
        }
        if let Some(&v) = st.get("Total kBytes") {
            e.incremental(
                &c.chart("net", "bandwidth", "kilobits/s", "Apache bandwidth", "area"),
                "sent",
                v,
                8.0,
            );
        }
        let w = c.chart("workers", "workers", "workers", "Apache workers", "stacked");
        e.gauge(&w, "busy", st["BusyWorkers"]);
        e.gauge(&w, "idle", st.get("IdleWorkers").copied().unwrap_or(0.0));
        if let Some(&v) = st.get("ConnsTotal") {
            e.gauge(
                &c.chart(
                    "connections",
                    "connections",
                    "connections",
                    "Apache connections",
                    "line",
                ),
                "connections",
                v,
            );
        }
        if let Some(&v) = st.get("Uptime") {
            e.gauge(
                &c.chart("uptime", "availability", "seconds", "Apache uptime", "line"),
                "uptime",
                v,
            );
        }
        if !board.is_empty() {
            let sb = c.chart(
                "scoreboard",
                "workers",
                "workers",
                "Apache scoreboard",
                "stacked",
            );
            for k in [
                "waiting",
                "starting",
                "reading",
                "sending",
                "keepalive",
                "dns_lookup",
                "closing",
                "logging",
                "finishing",
                "idle_cleanup",
                "open",
            ] {
                e.gauge(&sb, k, board.get(k).copied().unwrap_or(0.0));
            }
        }
        Ok(())
    }
}

pub(crate) struct Haproxy(pub AppConfig);

pub(crate) fn parse_haproxy_csv(s: &str) -> Result<Vec<HashMap<String, String>>, String> {
    let lines: Vec<&str> = s.trim().lines().collect();
    if lines.is_empty() || !lines[0].starts_with("# pxname,svname") {
        return Err("not an HAProxy stats CSV".into());
    }
    let hdr: Vec<&str> = lines[0].trim_start_matches("# ").split(',').collect();
    Ok(lines[1..]
        .iter()
        .map(|l| {
            l.split(',')
                .zip(&hdr)
                .filter(|(_, h)| !h.is_empty())
                .map(|(v, h)| (h.to_string(), v.to_string()))
                .collect()
        })
        .collect())
}

impl App for Haproxy {
    fn collect(&mut self, http: &dyn HttpGet, e: &mut Emitter) -> Result<(), String> {
        let c = &self.0;
        let url = if c.url.contains("csv") {
            c.url.clone()
        } else {
            format!("{}/;csv;norefresh", c.url.trim_end_matches('/'))
        };
        let rows = parse_haproxy_csv(&String::from_utf8_lossy(&c.get(http, &url)?))?;
        for r in &rows {
            let get = |k: &str| r.get(k).map(String::as_str).unwrap_or("");
            let sv = get("svname");
            if sv != "FRONTEND" && sv != "BACKEND" {
                continue;
            }
            let kind = sv.to_lowercase();
            let px = get("pxname");
            let ch = |metric: &str, units: &str, title: &str, ty: &str| {
                let mut x = c.chart(
                    &format!("{kind}_{metric}"),
                    &kind,
                    units,
                    &format!("HAProxy {kind} {title}"),
                    ty,
                );
                x.id = format!("{}_{}", x.id, sanitize_id(px));
                x.labels.insert("proxy".into(), px.into());
                x
            };
            e.gauge(
                &ch("sessions", "sessions", "current sessions", "line"),
                "current",
                pf(get("scur")),
            );
            e.incremental(
                &ch("session_rate", "sessions/s", "new sessions", "line"),
                "sessions",
                pf(get("stot")),
                1.0,
            );
            let bw = ch("bandwidth", "kilobits/s", "bandwidth", "area");
            e.incremental(&bw, "in", pf(get("bin")), 8.0 / 1000.0);
            e.incremental(&bw, "out", pf(get("bout")), 8.0 / 1000.0);
            let hr = ch("http_responses", "responses/s", "HTTP responses", "stacked");
            for k in ["1xx", "2xx", "3xx", "4xx", "5xx", "other"] {
                let v = get(&format!("hrsp_{k}"));
                if !v.is_empty() {
                    e.incremental(&hr, k, pf(v), 1.0);
                }
            }
            let er = ch("errors", "errors/s", "errors", "line");
            for k in ["ereq", "econ", "eresp", "dreq", "dresp"] {
                if !get(k).is_empty() {
                    e.incremental(&er, k, pf(get(k)), 1.0);
                }
            }
            if kind == "backend" {
                let up = if get("status").starts_with("UP") {
                    1.0
                } else {
                    0.0
                };
                e.gauge(&ch("up", "boolean", "status (1 up)", "line"), "up", up);
                if !get("act").is_empty() {
                    e.gauge(
                        &ch("servers", "servers", "active servers", "line"),
                        "active",
                        pf(get("act")),
                    );
                }
                if !get("qcur").is_empty() {
                    e.gauge(
                        &ch("queue", "requests", "queued requests", "line"),
                        "queued",
                        pf(get("qcur")),
                    );
                }
            }
        }
        Ok(())
    }
}

/// Sends each command over TCP. Replies to all but the last command must be
/// one non-error line; the last command's reply is read until `done`.
fn line_conn(
    c: &AppConfig,
    send: &[String],
    mut done: impl FnMut(&str) -> bool,
) -> Result<Vec<String>, String> {
    let addr = c
        .address
        .to_socket_addrs()
        .map_err(|e| e.to_string())?
        .next()
        .ok_or("address did not resolve")?;
    let conn = TcpStream::connect_timeout(&addr, c.timeout()).map_err(|e| e.to_string())?;
    conn.set_read_timeout(Some(c.timeout()))
        .map_err(|e| e.to_string())?;
    conn.set_write_timeout(Some(c.timeout()))
        .map_err(|e| e.to_string())?;
    let mut w = conn.try_clone().map_err(|e| e.to_string())?;
    let mut r = BufReader::with_capacity(64 * 1024, conn);
    let mut lines = Vec::new();
    for (i, cmd) in send.iter().enumerate() {
        w.write_all(cmd.as_bytes()).map_err(|e| e.to_string())?;
        loop {
            let mut l = String::new();
            if r.read_line(&mut l).map_err(|e| e.to_string())? == 0 {
                return Err("connection closed".into());
            }
            let l = l.trim_end_matches(['\r', '\n']).to_string();
            if i < send.len() - 1 {
                if let Some(err) = l.strip_prefix('-') {
                    return Err(err.to_string());
                }
                break;
            }
            let stop = done(&l);
            lines.push(l);
            if stop || lines.len() > 100_000 {
                return Ok(lines);
            }
        }
    }
    Ok(lines)
}

fn resp_command(args: &[&str]) -> String {
    let mut s = format!("*{}\r\n", args.len());
    for a in args {
        s.push_str(&format!("${}\r\n{a}\r\n", a.len()));
    }
    s
}

pub(crate) struct Redis(pub AppConfig);

pub(crate) fn parse_redis_info(lines: &[String]) -> (HashMap<String, f64>, HashMap<String, f64>) {
    let mut info = HashMap::new();
    let mut keyspace = HashMap::new();
    for l in lines {
        if l.is_empty() || l.starts_with('#') || l.starts_with('$') {
            continue;
        }
        let Some((k, v)) = l.split_once(':') else {
            continue;
        };
        if k.starts_with("db") && v.contains("keys=") {
            if let Some(n) = v.split(',').find_map(|kv| kv.strip_prefix("keys=")) {
                keyspace.insert(k.to_string(), pf(n));
            }
            continue;
        }
        if k == "master_link_status" {
            info.insert("master_link_up".into(), if v == "up" { 1.0 } else { 0.0 });
            continue;
        }
        if let Ok(f) = v.parse::<f64>() {
            info.insert(k.to_string(), f);
        }
    }
    (info, keyspace)
}

impl App for Redis {
    fn collect(&mut self, _http: &dyn HttpGet, e: &mut Emitter) -> Result<(), String> {
        let c = &self.0;
        let mut send = Vec::new();
        let env = |k: &str| {
            if k.is_empty() {
                String::new()
            } else {
                std::env::var(k).unwrap_or_default()
            }
        };
        let pw = env(&c.password_env);
        if !pw.is_empty() {
            let user = env(&c.username_env);
            send.push(if user.is_empty() {
                resp_command(&["AUTH", &pw])
            } else {
                resp_command(&["AUTH", &user, &pw])
            });
        }
        send.push(resp_command(&["INFO"]));
        let (mut want, mut got) = (0usize, 0usize);
        let lines = line_conn(c, &send, |l| {
            if want == 0 {
                if l.starts_with('-') {
                    return true;
                }
                if let Some(n) = l.strip_prefix('$') {
                    want = n.parse::<i64>().unwrap_or(0).max(0) as usize;
                    return want == 0;
                }
                return false;
            }
            got += l.len() + 2;
            got >= want
        })?;
        if let Some(err) = lines.first().and_then(|l| l.strip_prefix('-')) {
            return Err(format!("redis: {err}"));
        }
        let (info, keyspace) = parse_redis_info(&lines);
        if !info.contains_key("connected_clients") {
            return Err("redis: unexpected INFO reply".into());
        }
        let g = |k: &str| info.get(k).copied().unwrap_or(0.0);
        const MIB: f64 = (1u64 << 20) as f64;
        let cl = c.chart("clients", "clients", "clients", "Redis clients", "line");
        e.gauge(&cl, "connected", g("connected_clients"));
        e.gauge(&cl, "blocked", g("blocked_clients"));
        let mem = c.chart("memory", "memory", "MiB", "Redis memory", "line");
        e.gauge(&mem, "used", g("used_memory") / MIB);
        e.gauge(&mem, "rss", g("used_memory_rss") / MIB);
        if g("maxmemory") > 0.0 {
            e.gauge(&mem, "max", g("maxmemory") / MIB);
        }
        if let Some(&v) = info.get("mem_fragmentation_ratio") {
            e.gauge(
                &c.chart(
                    "mem_fragmentation_ratio",
                    "memory",
                    "ratio",
                    "Redis memory fragmentation",
                    "line",
                ),
                "ratio",
                v,
            );
        }
        e.incremental(
            &c.chart(
                "commands",
                "commands",
                "commands/s",
                "Redis commands processed",
                "line",
            ),
            "processed",
            g("total_commands_processed"),
            1.0,
        );
        let hm = c.chart(
            "keyspace_lookups",
            "keys",
            "lookups/s",
            "Redis keyspace lookups",
            "stacked",
        );
        e.incremental(&hm, "hits", g("keyspace_hits"), 1.0);
        e.incremental(&hm, "misses", g("keyspace_misses"), 1.0);
        let ev = c.chart(
            "keys_removed",
            "keys",
            "keys/s",
            "Redis expired and evicted keys",
            "line",
        );
        e.incremental(&ev, "expired", g("expired_keys"), 1.0);
        e.incremental(&ev, "evicted", g("evicted_keys"), 1.0);
        let nw = c.chart("net", "network", "kilobits/s", "Redis bandwidth", "area");
        e.incremental(&nw, "received", g("total_net_input_bytes"), 8.0 / 1000.0);
        e.incremental(&nw, "sent", g("total_net_output_bytes"), 8.0 / 1000.0);
        let cn = c.chart(
            "connections",
            "connections",
            "connections/s",
            "Redis connections",
            "line",
        );
        e.incremental(&cn, "accepted", g("total_connections_received"), 1.0);
        e.incremental(&cn, "rejected", g("rejected_connections"), 1.0);
        if let Some(&v) = info.get("rdb_changes_since_last_save") {
            e.gauge(
                &c.chart(
                    "rdb_changes",
                    "persistence",
                    "operations",
                    "Redis changes since last save",
                    "line",
                ),
                "changes",
                v,
            );
        }
        if let Some(&v) = info.get("connected_slaves") {
            e.gauge(
                &c.chart(
                    "replicas",
                    "replication",
                    "replicas",
                    "Redis connected replicas",
                    "line",
                ),
                "connected",
                v,
            );
        }
        if let Some(&v) = info.get("master_link_up") {
            e.gauge(
                &c.chart(
                    "master_link",
                    "replication",
                    "boolean",
                    "Redis master link (1 up)",
                    "line",
                ),
                "up",
                v,
            );
        }
        if !keyspace.is_empty() {
            let k = c.chart(
                "keyspace",
                "keys",
                "keys",
                "Redis keys per database",
                "stacked",
            );
            for (db, n) in &keyspace {
                e.gauge(&k, db, *n);
            }
        }
        Ok(())
    }
}

pub(crate) struct Memcached(pub AppConfig);

impl App for Memcached {
    fn collect(&mut self, _http: &dyn HttpGet, e: &mut Emitter) -> Result<(), String> {
        let c = &self.0;
        let lines = line_conn(c, &["stats\r\n".to_string()], |l| {
            l == "END" || l.starts_with("ERROR")
        })?;
        let st: HashMap<&str, f64> = lines
            .iter()
            .filter_map(|l| {
                let f: Vec<&str> = l.split_whitespace().collect();
                (f.len() == 3 && f[0] == "STAT")
                    .then(|| Some((f[1], f[2].parse().ok()?)))
                    .flatten()
            })
            .collect();
        if !st.contains_key("curr_connections") {
            return Err("memcached: unexpected stats reply".into());
        }
        let g = |k: &str| st.get(k).copied().unwrap_or(0.0);
        const MIB: f64 = (1u64 << 20) as f64;
        e.gauge(
            &c.chart(
                "connections",
                "connections",
                "connections",
                "memcached connections",
                "line",
            ),
            "current",
            g("curr_connections"),
        );
        e.incremental(
            &c.chart(
                "connections_rate",
                "connections",
                "connections/s",
                "memcached new connections",
                "line",
            ),
            "opened",
            g("total_connections"),
            1.0,
        );
        let ops = c.chart(
            "operations",
            "operations",
            "operations/s",
            "memcached operations",
            "line",
        );
        e.incremental(&ops, "get", g("cmd_get"), 1.0);
        e.incremental(&ops, "set", g("cmd_set"), 1.0);
        let hm = c.chart(
            "get_hits",
            "operations",
            "lookups/s",
            "memcached get hits and misses",
            "stacked",
        );
        e.incremental(&hm, "hits", g("get_hits"), 1.0);
        e.incremental(&hm, "misses", g("get_misses"), 1.0);
        let mem = c.chart("memory", "memory", "MiB", "memcached memory", "line");
        e.gauge(&mem, "used", g("bytes") / MIB);
        e.gauge(&mem, "limit", g("limit_maxbytes") / MIB);
        e.gauge(
            &c.chart("items", "items", "items", "memcached items", "line"),
            "current",
            g("curr_items"),
        );
        e.incremental(
            &c.chart(
                "evictions",
                "items",
                "items/s",
                "memcached evictions",
                "line",
            ),
            "evicted",
            g("evictions"),
            1.0,
        );
        let nw = c.chart(
            "net",
            "network",
            "kilobits/s",
            "memcached bandwidth",
            "area",
        );
        e.incremental(&nw, "received", g("bytes_read"), 8.0 / 1000.0);
        e.incremental(&nw, "sent", g("bytes_written"), 8.0 / 1000.0);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::apps::{AppKind, HttpGetFn};
    use super::*;
    use std::io::Read;
    use std::net::TcpListener;

    #[allow(clippy::type_complexity)]
    fn no_http() -> HttpGetFn<
        impl Fn(&str, &[(String, String)], bool, std::time::Duration) -> Result<Vec<u8>, String>,
    > {
        HttpGetFn(|_: &str, _: &[(String, String)], _, _| Err("unused".to_string()))
    }

    #[test]
    fn parsers() {
        let a = parse_apache_status(
            "Total Accesses: 10\nBusyWorkers: 2\nIdleWorkers: 8\nScoreboard: __WK.\n",
        );
        assert_eq!(a.0["BusyWorkers"], 2.0);
        assert_eq!(a.1["waiting"], 2.0);
        let rows = parse_haproxy_csv(
            "# pxname,svname,scur,status\nweb,FRONTEND,3,OPEN\nweb,BACKEND,1,UP\n",
        )
        .unwrap();
        assert_eq!(rows[1]["status"], "UP");
        assert!(parse_haproxy_csv("<html>").is_err());
        let (info, ks) = parse_redis_info(&[
            "$100".into(),
            "# Clients".into(),
            "connected_clients:3".into(),
            "db0:keys=12,expires=0".into(),
            "master_link_status:up".into(),
        ]);
        assert_eq!(info["connected_clients"], 3.0);
        assert_eq!(info["master_link_up"], 1.0);
        assert_eq!(ks["db0"], 12.0);
        assert!(parse_nginx_status("hello").is_err());
    }

    fn serve_once(reply: &'static str) -> String {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap().to_string();
        std::thread::spawn(move || {
            let (mut s, _) = l.accept().unwrap();
            let mut buf = [0u8; 256];
            let _ = s.read(&mut buf);
            s.write_all(reply.as_bytes()).unwrap();
        });
        addr
    }

    #[test]
    fn redis_info_over_resp() {
        let body = "# Clients\r\nconnected_clients:7\r\nused_memory:1048576\r\n";
        let reply: &'static str =
            Box::leak(format!("${}\r\n{body}\r\n", body.len()).into_boxed_str());
        let mut r = Redis(AppConfig::new(AppKind::Redis, "cache", &serve_once(reply)));
        r.0.normalize().unwrap();
        let mut e = Emitter::new();
        e.begin(1);
        r.collect(&no_http(), &mut e).unwrap();
        let v = e
            .samples()
            .iter()
            .find(|s| s.series.context == "redis.clients" && s.series.dimension == "connected")
            .unwrap()
            .v;
        assert_eq!(v, 7.0);
    }

    #[test]
    fn memcached_stats() {
        let mut m = Memcached(AppConfig::new(
            AppKind::Memcached,
            "mc",
            &serve_once("STAT curr_connections 5\r\nSTAT bytes 2097152\r\nEND\r\n"),
        ));
        m.0.normalize().unwrap();
        let mut e = Emitter::new();
        e.begin(1);
        m.collect(&no_http(), &mut e).unwrap();
        let used = e
            .samples()
            .iter()
            .find(|s| s.series.context == "memcached.memory" && s.series.dimension == "used")
            .unwrap()
            .v;
        assert_eq!(used, 2.0);
    }
}
