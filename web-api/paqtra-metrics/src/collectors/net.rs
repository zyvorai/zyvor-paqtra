use super::{
    header_pairs, key_value_file, labels, pf, phex, read_float, read_lines, read_trim, Chart,
    Collector, Emitter, Fsys, Get, Info,
};
use std::collections::{HashMap, HashSet};

/// Per-interface counters from /proc/net/dev plus sysfs state.
/// `net.operstate` is charted only for interfaces seen up since the agent
/// started, so idle bridges and unplugged ports never read as an outage.
pub struct NetDev {
    fs: Fsys,
    seen_up: HashSet<String>,
}

impl NetDev {
    pub fn new(fs: Fsys) -> Self {
        Self {
            fs,
            seen_up: HashSet::new(),
        }
    }
}

impl Collector for NetDev {
    fn info(&self) -> Info {
        Info::new("proc.net.dev", "net", 1)
    }

    fn collect(&mut self, _now: i64, e: &mut Emitter) -> Result<(), String> {
        let lines = read_lines(&self.fs.proc("net/dev"))?;
        let mut present = HashSet::new();
        let (mut rx_all, mut tx_all) = (0.0, 0.0);
        for l in &lines {
            let Some((name, rest)) = l.split_once(':') else {
                continue;
            };
            let iface = name.trim();
            let f: Vec<f64> = rest.split_whitespace().map(pf).collect();
            if f.len() < 16 {
                continue;
            }
            let virt = self
                .fs
                .sys(&format!("devices/virtual/net/{iface}"))
                .exists();
            let lbl = labels([
                ("interface", iface),
                ("virtual", if virt { "true" } else { "false" }),
            ]);
            let mk = |ctx: &str, id: &str, units: &str, title: &str| {
                Chart::new(ctx, "net", units, title)
                    .id(format!("{id}.{iface}"))
                    .labels(&lbl)
            };
            let bw = mk("net.net", "net", "kilobits/s", "Bandwidth").ty("area");
            e.incremental(&bw, "received", f[0], 8.0 / 1000.0);
            e.incremental(&bw, "sent", f[8], 8.0 / 1000.0);
            let pk = mk("net.packets", "net_packets", "packets/s", "Packets");
            e.incremental(&pk, "received", f[1], 1.0);
            e.incremental(&pk, "sent", f[9], 1.0);
            e.incremental(&pk, "multicast", f[7], 1.0);
            let er = mk("net.errors", "net_errors", "errors/s", "Interface errors");
            e.incremental(&er, "inbound", f[2], 1.0);
            e.incremental(&er, "outbound", f[10], 1.0);
            let dr = mk("net.drops", "net_drops", "drops/s", "Interface drops");
            e.incremental(&dr, "inbound", f[3], 1.0);
            e.incremental(&dr, "outbound", f[11], 1.0);
            let ff = mk(
                "net.fifo",
                "net_fifo",
                "errors/s",
                "Interface FIFO buffer errors",
            );
            e.incremental(&ff, "receive", f[4], 1.0);
            e.incremental(&ff, "transmit", f[12], 1.0);
            e.incremental(
                &mk(
                    "net.frames",
                    "net_frames",
                    "frames/s",
                    "Interface frame errors",
                ),
                "frames",
                f[5],
                1.0,
            );
            let ca = mk(
                "net.carrier",
                "net_carrier",
                "events/s",
                "Interface carrier and collision events",
            );
            e.incremental(&ca, "carrier", f[14], 1.0);
            e.incremental(&ca, "collisions", f[13], 1.0);
            let class = |file: &str| self.fs.sys(&format!("class/net/{iface}/{file}"));
            if let Some(v) = read_float(&class("speed")).filter(|v| *v > 0.0) {
                e.gauge(
                    &mk("net.speed", "net_speed", "kilobits/s", "Interface speed"),
                    "speed",
                    v * 1000.0,
                );
            }
            if let Some(v) = read_float(&class("mtu")) {
                e.gauge(
                    &mk("net.mtu", "net_mtu", "octets", "Interface MTU"),
                    "mtu",
                    v,
                );
            }
            if let Some(s) = read_trim(&class("operstate")) {
                let up = s == "up" || s == "unknown";
                if up {
                    self.seen_up.insert(iface.to_string());
                }
                present.insert(iface.to_string());
                if self.seen_up.contains(iface) {
                    e.gauge(
                        &mk(
                            "net.operstate",
                            "net_operstate",
                            "state",
                            "Interface operational state (1 up)",
                        ),
                        "up",
                        if up { 1.0 } else { 0.0 },
                    );
                }
            }
            if !virt {
                rx_all += f[0];
                tx_all += f[8];
            }
        }
        self.seen_up.retain(|i| present.contains(i));
        let sys = Chart::new(
            "system.net",
            "net",
            "kilobits/s",
            "Physical network interfaces aggregated bandwidth",
        )
        .ty("area");
        e.incremental(&sys, "received", rx_all, 8.0 / 1000.0);
        e.incremental(&sys, "sent", tx_all, 8.0 / 1000.0);
        Ok(())
    }
}

fn incs(e: &mut Emitter, ch: &Chart, m: Option<&HashMap<String, f64>>, dims: &[(&str, &str)]) {
    for (dim, key) in dims {
        e.incremental(ch, dim, m.g(key), 1.0);
    }
}

/// IPv4 IP, ICMP, TCP and UDP counters plus IPv6 basics.
pub struct Snmp {
    fs: Fsys,
}

impl Snmp {
    pub fn new(fs: Fsys) -> Self {
        Self { fs }
    }
}

impl Collector for Snmp {
    fn info(&self) -> Info {
        Info::new("proc.net.snmp", "ip", 1)
    }

    fn collect(&mut self, _now: i64, e: &mut Emitter) -> Result<(), String> {
        let m = header_pairs(&self.fs.proc("net/snmp"))?;
        let ip = m.get("Ip");
        incs(
            e,
            &Chart::new("ipv4.packets", "ipv4", "packets/s", "IPv4 packets"),
            ip,
            &[
                ("received", "InReceives"),
                ("sent", "OutRequests"),
                ("forwarded", "ForwDatagrams"),
                ("delivered", "InDelivers"),
            ],
        );
        let ie = Chart::new("ipv4.errors", "ipv4", "packets/s", "IPv4 errors");
        for k in [
            "InDiscards",
            "OutDiscards",
            "InHdrErrors",
            "OutNoRoutes",
            "InAddrErrors",
            "InUnknownProtos",
        ] {
            e.incremental(&ie, k, ip.g(k), 1.0);
        }
        let ic = m.get("Icmp");
        incs(
            e,
            &Chart::new("ipv4.icmp", "icmp", "packets/s", "IPv4 ICMP packets"),
            ic,
            &[("received", "InMsgs"), ("sent", "OutMsgs")],
        );
        let ice = Chart::new("ipv4.icmp_errors", "icmp", "packets/s", "IPv4 ICMP errors");
        for k in ["InErrors", "OutErrors", "InCsumErrors"] {
            e.incremental(&ice, k, ic.g(k), 1.0);
        }
        let tcp = m.get("Tcp");
        e.gauge(
            &Chart::new(
                "ipv4.tcpsock",
                "tcp",
                "active connections",
                "IPv4 TCP connections",
            ),
            "connections",
            tcp.g("CurrEstab"),
        );
        incs(
            e,
            &Chart::new("ipv4.tcppackets", "tcp", "packets/s", "IPv4 TCP packets"),
            tcp,
            &[("received", "InSegs"), ("sent", "OutSegs")],
        );
        let te = Chart::new("ipv4.tcperrors", "tcp", "packets/s", "IPv4 TCP errors");
        for k in ["InErrs", "InCsumErrors", "RetransSegs"] {
            e.incremental(&te, k, tcp.g(k), 1.0);
        }
        if tcp.g("OutSegs") > 0.0 {
            e.incremental(
                &Chart::new(
                    "ipv4.tcp_retrans_segments",
                    "tcp",
                    "segments/s",
                    "IPv4 TCP retransmitted segments",
                ),
                "retransmits",
                tcp.g("RetransSegs"),
                1.0,
            );
        }
        incs(
            e,
            &Chart::new("ipv4.tcpopens", "tcp", "connections/s", "IPv4 TCP opens"),
            tcp,
            &[("active", "ActiveOpens"), ("passive", "PassiveOpens")],
        );
        let th = Chart::new(
            "ipv4.tcphandshake",
            "tcp",
            "events/s",
            "IPv4 TCP handshake issues",
        );
        for k in ["EstabResets", "OutRsts", "AttemptFails"] {
            e.incremental(&th, k, tcp.g(k), 1.0);
        }
        let udp = m.get("Udp");
        incs(
            e,
            &Chart::new("ipv4.udppackets", "udp", "packets/s", "IPv4 UDP packets"),
            udp,
            &[("received", "InDatagrams"), ("sent", "OutDatagrams")],
        );
        let ue = Chart::new("ipv4.udperrors", "udp", "events/s", "IPv4 UDP errors");
        for k in [
            "RcvbufErrors",
            "SndbufErrors",
            "InErrors",
            "NoPorts",
            "InCsumErrors",
            "IgnoredMulti",
        ] {
            e.incremental(&ue, k, udp.g(k), 1.0);
        }
        if let Ok(v6) = key_value_file(&self.fs.proc("net/snmp6")) {
            let v6 = Some(&v6);
            incs(
                e,
                &Chart::new("ipv6.packets", "ipv6", "packets/s", "IPv6 packets"),
                v6,
                &[
                    ("received", "Ip6InReceives"),
                    ("sent", "Ip6OutRequests"),
                    ("forwarded", "Ip6OutForwDatagrams"),
                    ("delivered", "Ip6InDelivers"),
                ],
            );
            let e6 = Chart::new("ipv6.errors", "ipv6", "packets/s", "IPv6 errors");
            for k in [
                "Ip6InDiscards",
                "Ip6OutDiscards",
                "Ip6InHdrErrors",
                "Ip6InNoRoutes",
                "Ip6OutNoRoutes",
                "Ip6InAddrErrors",
            ] {
                e.incremental(&e6, &k[3..], v6.g(k), 1.0);
            }
            incs(
                e,
                &Chart::new("ipv6.udppackets", "udp6", "packets/s", "IPv6 UDP packets"),
                v6,
                &[
                    ("received", "Udp6InDatagrams"),
                    ("sent", "Udp6OutDatagrams"),
                ],
            );
            incs(
                e,
                &Chart::new("ipv6.icmp", "icmp6", "messages/s", "IPv6 ICMP messages"),
                v6,
                &[("received", "Icmp6InMsgs"), ("sent", "Icmp6OutMsgs")],
            );
        }
        Ok(())
    }
}

/// TcpExt and IpExt extended counters.
pub struct Netstat {
    fs: Fsys,
}

impl Netstat {
    pub fn new(fs: Fsys) -> Self {
        Self { fs }
    }
}

impl Collector for Netstat {
    fn info(&self) -> Info {
        Info::new("proc.net.netstat", "tcp", 1)
    }

    fn collect(&mut self, _now: i64, e: &mut Emitter) -> Result<(), String> {
        let m = header_pairs(&self.fs.proc("net/netstat"))?;
        let t = m.get("TcpExt");
        incs(
            e,
            &Chart::new(
                "ip.tcp_accept_queue",
                "tcp",
                "packets/s",
                "TCP accept queue issues",
            ),
            t,
            &[("overflows", "ListenOverflows"), ("drops", "ListenDrops")],
        );
        incs(
            e,
            &Chart::new(
                "ip.tcp_syn_queue",
                "tcp",
                "packets/s",
                "TCP SYN queue issues",
            ),
            t,
            &[
                ("drops", "TCPReqQFullDrop"),
                ("cookies", "TCPReqQFullDoCookies"),
            ],
        );
        incs(
            e,
            &Chart::new("ip.tcpsyncookies", "tcp", "packets/s", "TCP SYN cookies"),
            t,
            &[
                ("received", "SyncookiesRecv"),
                ("sent", "SyncookiesSent"),
                ("failed", "SyncookiesFailed"),
            ],
        );
        incs(
            e,
            &Chart::new(
                "ip.tcpconnaborts",
                "tcp",
                "connections/s",
                "TCP connection aborts",
            ),
            t,
            &[
                ("baddata", "TCPAbortOnData"),
                ("userclosed", "TCPAbortOnClose"),
                ("nomemory", "TCPAbortOnMemory"),
                ("timeout", "TCPAbortOnTimeout"),
                ("linger", "TCPAbortOnLinger"),
                ("failed", "TCPAbortFailed"),
            ],
        );
        incs(
            e,
            &Chart::new(
                "ip.tcp_retransmits",
                "tcp",
                "events/s",
                "TCP retransmission events",
            ),
            t,
            &[
                ("timeouts", "TCPTimeouts"),
                ("fast", "TCPFastRetrans"),
                ("slow_start", "TCPSlowStartRetrans"),
                ("lost", "TCPLostRetransmit"),
                ("syn", "TCPSynRetrans"),
            ],
        );
        incs(
            e,
            &Chart::new(
                "ip.tcpreorders",
                "tcp",
                "packets/s",
                "TCP reordered packets by detection method",
            ),
            t,
            &[
                ("ts", "TCPTSReorder"),
                ("sack", "TCPSACKReorder"),
                ("reno", "TCPRenoReorder"),
            ],
        );
        incs(
            e,
            &Chart::new("ip.tcpofo", "tcp", "packets/s", "TCP out-of-order queue"),
            t,
            &[
                ("inqueue", "TCPOFOQueue"),
                ("dropped", "TCPOFODrop"),
                ("merged", "TCPOFOMerge"),
            ],
        );
        incs(
            e,
            &Chart::new(
                "ip.tcp_memory_pressure",
                "tcp",
                "events/s",
                "TCP memory pressure events",
            ),
            t,
            &[("pressures", "TCPMemoryPressures")],
        );
        incs(
            e,
            &Chart::new(
                "ip.tcp_backlog_drops",
                "tcp",
                "packets/s",
                "TCP backlog queue drops",
            ),
            t,
            &[("drops", "TCPBacklogDrop")],
        );
        let x = m.get("IpExt");
        let bw = Chart::new("system.ip", "ip", "kilobits/s", "IP bandwidth").ty("area");
        e.incremental(&bw, "received", x.g("InOctets"), 8.0 / 1000.0);
        e.incremental(&bw, "sent", x.g("OutOctets"), 8.0 / 1000.0);
        let mc = Chart::new(
            "ip.mcast",
            "multicast",
            "kilobits/s",
            "IP multicast bandwidth",
        );
        e.incremental(&mc, "received", x.g("InMcastOctets"), 8.0 / 1000.0);
        e.incremental(&mc, "sent", x.g("OutMcastOctets"), 8.0 / 1000.0);
        incs(
            e,
            &Chart::new("ip.ecnpkts", "ecn", "packets/s", "IP ECN statistics"),
            x,
            &[
                ("CEP", "InCEPkts"),
                ("NoECTP", "InNoECTPkts"),
                ("ECTP0", "InECT0Pkts"),
                ("ECTP1", "InECT1Pkts"),
            ],
        );
        Ok(())
    }
}

fn parse_sockstat(lines: &[String]) -> HashMap<String, HashMap<String, f64>> {
    lines
        .iter()
        .filter_map(|l| {
            let (name, rest) = l.split_once(':')?;
            let f: Vec<&str> = rest.split_whitespace().collect();
            let m = f
                .chunks(2)
                .filter(|c| c.len() == 2)
                .map(|c| (c[0].to_string(), pf(c[1])))
                .collect();
            Some((name.to_string(), m))
        })
        .collect()
}

/// Socket counts from /proc/net/sockstat and sockstat6.
pub struct Sockstat {
    fs: Fsys,
}

impl Sockstat {
    pub fn new(fs: Fsys) -> Self {
        Self { fs }
    }
}

impl Collector for Sockstat {
    fn info(&self) -> Info {
        Info::new("proc.net.sockstat", "sockets", 1)
    }

    fn collect(&mut self, _now: i64, e: &mut Emitter) -> Result<(), String> {
        let m = parse_sockstat(&read_lines(&self.fs.proc("net/sockstat"))?);
        let g = |sec: &str, k: &str| m.get(sec).g(k);
        e.gauge(
            &Chart::new(
                "ipv4.sockstat_sockets",
                "sockets",
                "sockets",
                "IPv4 sockets used",
            ),
            "used",
            g("sockets", "used"),
        );
        let tc = Chart::new(
            "ipv4.sockstat_tcp_sockets",
            "tcp",
            "sockets",
            "IPv4 TCP sockets",
        );
        for k in ["alloc", "orphan", "inuse", "tw"] {
            e.gauge(&tc, k, g("TCP", k));
        }
        e.gauge(
            &Chart::new(
                "ipv4.sockstat_tcp_mem",
                "tcp",
                "KiB",
                "IPv4 TCP sockets memory",
            ),
            "mem",
            g("TCP", "mem") * 4.0,
        );
        e.gauge(
            &Chart::new(
                "ipv4.sockstat_udp_sockets",
                "udp",
                "sockets",
                "IPv4 UDP sockets",
            ),
            "inuse",
            g("UDP", "inuse"),
        );
        e.gauge(
            &Chart::new(
                "ipv4.sockstat_udp_mem",
                "udp",
                "KiB",
                "IPv4 UDP sockets memory",
            ),
            "mem",
            g("UDP", "mem") * 4.0,
        );
        e.gauge(
            &Chart::new(
                "ipv4.sockstat_raw_sockets",
                "raw",
                "sockets",
                "IPv4 RAW sockets",
            ),
            "inuse",
            g("RAW", "inuse"),
        );
        e.gauge(
            &Chart::new(
                "ipv4.sockstat_frag_sockets",
                "fragments",
                "fragments",
                "IPv4 fragments in use",
            ),
            "inuse",
            g("FRAG", "inuse"),
        );
        if let Ok(l6) = read_lines(&self.fs.proc("net/sockstat6")) {
            let m6 = parse_sockstat(&l6);
            let g6 = |sec: &str| m6.get(sec).g("inuse");
            e.gauge(
                &Chart::new(
                    "ipv6.sockstat6_tcp_sockets",
                    "tcp6",
                    "sockets",
                    "IPv6 TCP sockets",
                ),
                "inuse",
                g6("TCP6"),
            );
            e.gauge(
                &Chart::new(
                    "ipv6.sockstat6_udp_sockets",
                    "udp6",
                    "sockets",
                    "IPv6 UDP sockets",
                ),
                "inuse",
                g6("UDP6"),
            );
            e.gauge(
                &Chart::new(
                    "ipv6.sockstat6_raw_sockets",
                    "raw6",
                    "sockets",
                    "IPv6 RAW sockets",
                ),
                "inuse",
                g6("RAW6"),
            );
        }
        Ok(())
    }
}

/// The netfilter connection table size and statistics.
pub struct Conntrack {
    fs: Fsys,
}

impl Conntrack {
    pub fn new(fs: Fsys) -> Self {
        Self { fs }
    }
}

impl Collector for Conntrack {
    fn info(&self) -> Info {
        Info::new("netfilter.conntrack", "netfilter", 1)
    }

    fn collect(&mut self, _now: i64, e: &mut Emitter) -> Result<(), String> {
        const FAM: &str = "connection tracker";
        let count = read_float(&self.fs.proc("sys/net/netfilter/nf_conntrack_count"))
            .ok_or("nf_conntrack_count not available")?;
        e.gauge(
            &Chart::new(
                "netfilter.conntrack_sockets",
                FAM,
                "active connections",
                "Connection tracker connections",
            ),
            "connections",
            count,
        );
        if let Some(mx) =
            read_float(&self.fs.proc("sys/net/netfilter/nf_conntrack_max")).filter(|m| *m > 0.0)
        {
            e.gauge(
                &Chart::new(
                    "netfilter.conntrack_utilization",
                    FAM,
                    "%",
                    "Connection tracker table utilization",
                ),
                "used",
                count / mx * 100.0,
            );
            e.gauge(
                &Chart::new(
                    "netfilter.conntrack_max",
                    FAM,
                    "connections",
                    "Connection tracker table size",
                ),
                "max",
                mx,
            );
        }
        let Ok(lines) = read_lines(&self.fs.proc("net/stat/nf_conntrack")) else {
            return Ok(());
        };
        if lines.len() < 2 {
            return Ok(());
        }
        let hdr: Vec<&str> = lines[0].split_whitespace().collect();
        let mut sums = vec![0.0; hdr.len()];
        for l in &lines[1..] {
            for (i, v) in l.split_whitespace().take(hdr.len()).enumerate() {
                sums[i] += phex(v);
            }
        }
        let errs = Chart::new(
            "netfilter.conntrack_errors",
            FAM,
            "events/s",
            "Connection tracker errors",
        );
        let changes = Chart::new(
            "netfilter.conntrack_changes",
            FAM,
            "events/s",
            "Connection tracker changes",
        );
        for (i, h) in hdr.iter().enumerate() {
            match *h {
                "invalid" | "insert_failed" | "drop" | "early_drop" | "icmp_error"
                | "search_restart" => e.incremental(&errs, h, sums[i], 1.0),
                "found" | "new" | "insert" | "delete" => e.incremental(&changes, h, sums[i], 1.0),
                _ => {}
            }
        }
        Ok(())
    }
}

/// /proc/net/softnet_stat (hex columns, one row per CPU).
pub struct Softnet {
    fs: Fsys,
}

impl Softnet {
    pub fn new(fs: Fsys) -> Self {
        Self { fs }
    }
}

impl Collector for Softnet {
    fn info(&self) -> Info {
        Info::new("proc.net.softnet_stat", "softnet", 1)
    }

    fn collect(&mut self, _now: i64, e: &mut Emitter) -> Result<(), String> {
        let lines = read_lines(&self.fs.proc("net/softnet_stat"))?;
        let mut s = [0.0; 5];
        for l in &lines {
            let f: Vec<&str> = l.split_whitespace().collect();
            if f.len() < 3 {
                continue;
            }
            for (slot, col) in [(0, 0), (1, 1), (2, 2), (3, 9), (4, 10)] {
                if let Some(v) = f.get(col) {
                    s[slot] += phex(v);
                }
            }
        }
        let ch = Chart::new(
            "system.softnet_stat",
            "softnet",
            "events/s",
            "Softnet events",
        );
        for (i, d) in [
            "processed",
            "dropped",
            "squeezed",
            "received_rps",
            "flow_limit_count",
        ]
        .iter()
        .enumerate()
        {
            e.incremental(&ch, d, s[i], 1.0);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::testutil::*;
    use super::*;

    #[test]
    fn netdev_rates_and_physical_aggregate() {
        let fx = Fixture::new("host");
        let mut n = NetDev::new(fx.fs());
        let mut r = Runs::new();
        r.run(&mut n, 1000);
        fx.rewrite(
            "proc/net/dev",
            "eth0: 1000000   10000",
            "eth0: 1125000   10100",
        );
        let got = r.run(&mut n, 1001);
        got.want("net.eth0/received", 1000.0); // 125000 bytes/s
        got.want("net_packets.eth0/received", 100.0);
        got.want("system.net/received", 1000.0);
        got.want("net_speed.eth0/speed", 10_000_000.0);
    }

    #[test]
    fn operstate_charted_only_after_up() {
        let fx = Fixture::new("host");
        fx.write("sys/class/net/eth0/operstate", "down\n");
        let mut n = NetDev::new(fx.fs());
        let mut r = Runs::new();
        assert!(!r.run(&mut n, 1000).has("net_operstate.eth0/up"));
        fx.write("sys/class/net/eth0/operstate", "up\n");
        r.run(&mut n, 1001).want("net_operstate.eth0/up", 1.0);
        fx.write("sys/class/net/eth0/operstate", "down\n");
        r.run(&mut n, 1002).want("net_operstate.eth0/up", 0.0);
    }

    #[test]
    fn snmp_netstat_sockstat_conntrack_softnet() {
        let fx = Fixture::new("host");
        let fs = fx.fs();
        let got = Runs::new().run(&mut Snmp::new(fs.clone()), 1000);
        got.want("ipv4.tcpsock/connections", 42.0);
        let mut r = Runs::new();
        let mut ns = Netstat::new(fs.clone());
        r.run(&mut ns, 1000);
        assert!(r.run(&mut ns, 1001).has("ip.tcp_accept_queue/overflows"));
        let got = Runs::new().run(&mut Sockstat::new(fs.clone()), 1000);
        assert!(got.has("ipv4.sockstat_tcp_sockets/inuse"));
        let got = Runs::new().run(&mut Conntrack::new(fs.clone()), 1000);
        assert!(got.has("netfilter.conntrack_utilization/used"));
        let mut r = Runs::new();
        let mut sn = Softnet::new(fs);
        r.run(&mut sn, 1000);
        assert!(r.run(&mut sn, 1001).has("system.softnet_stat/dropped"));
    }
}
