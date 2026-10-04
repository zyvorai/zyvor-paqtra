use super::expr::truth;
use super::rules::{compile, Compiled, Rule};
use crate::tsdb::{match_any, match_glob, reduce, Db, NullFloat, Source};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// Status of one alert instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    #[default]
    Undefined,
    Clear,
    Warning,
    Critical,
}

impl Status {
    /// Netdata-compatible codes for `$status`, `$WARNING` and friends.
    fn code(self) -> f64 {
        match self {
            Status::Undefined => -1.0,
            Status::Clear => 1.0,
            Status::Warning => 3.0,
            Status::Critical => 4.0,
        }
    }

    pub fn raised(self) -> bool {
        matches!(self, Status::Warning | Status::Critical)
    }

    fn rank(self) -> u8 {
        match self {
            Status::Undefined => 0,
            Status::Clear => 1,
            Status::Warning => 2,
            Status::Critical => 3,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Status::Undefined => "undefined",
            Status::Clear => "clear",
            Status::Warning => "warning",
            Status::Critical => "critical",
        }
    }
}

/// One rule instance (a rule on one node and chart or dimension).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Alert {
    pub id: String,
    pub rule: String,
    pub node: String,
    pub context: String,
    pub chart: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub dimension: String,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub labels: BTreeMap<String, String>,
    pub status: Status,
    pub value: NullFloat,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub units: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub class: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub info: String,
    pub since: i64,
    pub last_eval: i64,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub silenced: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub acked: bool,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub acked_by: String,
    #[serde(skip)]
    pending: Option<(Status, i64)>,
    #[serde(skip)]
    last_notify: i64,
    #[serde(skip)]
    last_seen: i64,
}

/// One status change, kept in a bounded history.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Transition {
    pub time: i64,
    pub id: String,
    pub rule: String,
    pub node: String,
    pub chart: String,
    pub from: Status,
    pub to: Status,
    pub value: NullFloat,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub units: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub info: String,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub silenced: bool,
}

/// Suppresses notifications for matching alerts until `until`. Status is
/// still tracked and shown. Matchers are globs; empty matches anything.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Silence {
    #[serde(default)]
    pub id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub rule: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub node: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub chart: String,
    pub until: i64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub comment: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub created_by: String,
    #[serde(default)]
    pub created: i64,
}

impl Silence {
    fn matches(&self, a: &Alert) -> bool {
        let g = |p: &str, v: &str| p.is_empty() || match_glob(p, v);
        g(&self.rule, &a.rule) && g(&self.node, &a.node) && g(&self.chart, &a.chart)
    }
}

/// A notification: a raised status, a repeat, or a return to clear.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Event {
    pub alert_id: String,
    pub rule: String,
    pub node: String,
    pub subject: String,
    pub status: Status,
    /// info, warning or critical.
    pub severity: &'static str,
    pub message: String,
    pub value: NullFloat,
    pub time: i64,
}

pub type Publish = Arc<dyn Fn(Event) + Send + Sync>;
pub type Sources = Arc<dyn Fn() -> Vec<Source> + Send + Sync>;

pub struct Options {
    pub rules: Vec<Rule>,
    pub sources: Sources,
    /// None keeps alerts API-only.
    pub publish: Option<Publish>,
    /// Persists silences across restarts; None keeps them in memory.
    pub silence_file: Option<PathBuf>,
    pub history_size: usize, // default 1000
    pub max_alerts: usize,   // default 50000
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Stats {
    pub rules: usize,
    pub instances: usize,
    pub warning: usize,
    pub critical: usize,
    pub evaluations: u64,
    pub dropped_instances: u64,
}

/// The API view.
#[derive(Debug, Clone, Serialize)]
pub struct Snapshot {
    pub active: Vec<Alert>,
    pub history: Vec<Transition>,
    pub rules: Vec<Rule>,
    pub silences: Vec<Silence>,
    pub stats: Stats,
}

#[derive(Default)]
struct State {
    next_eval: HashMap<String, i64>,
    alerts: HashMap<String, Alert>,
    history: Vec<Transition>,
    silences: HashMap<String, Silence>,
    evals: u64,
    dropped: u64,
}

/// Evaluates rules on a schedule. Read-only: an alert can notify, never
/// change policy or the datapath.
pub struct Engine {
    rules: Vec<Compiled>,
    sources: Sources,
    publish: Option<Publish>,
    silence_file: Option<PathBuf>,
    history_size: usize,
    max_alerts: usize,
    state: Mutex<State>,
}

#[derive(Default)]
struct Instance {
    chart: String,
    dim: String,
    labels: BTreeMap<String, String>,
    vals: Vec<f64>,
    dim_vals: HashMap<String, f64>,
    anom: usize,
    total: usize,
}

fn alert_id(rule: &str, node: &str, chart: &str, dim: &str) -> String {
    // FNV-1a, 64 bit: stable across restarts and platforms.
    let mut h: u64 = 0xcbf29ce484222325;
    for b in [rule, node, chart, dim].join("\0").bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{h:016x}")
}

fn random_id() -> String {
    let mut b = [0u8; 8];
    if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
        use std::io::Read;
        let _ = f.read_exact(&mut b);
    }
    if b == [0; 8] {
        let n = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        b = (n as u64).to_le_bytes();
    }
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// Reduces one series over [after, before]: tier 0 when the window is inside
/// its retention, 1-minute rollups otherwise.
fn lookup(
    db: &Db,
    key: &str,
    func: &str,
    after: i64,
    before: i64,
    now: i64,
) -> (f64, usize, usize) {
    if after >= now - db.retention()[0].as_secs() as i64 {
        let pts = db.points(key, after, before);
        if pts.is_empty() {
            return (f64::NAN, 0, 0);
        }
        let anom = pts.iter().filter(|p| p.anomalous).count();
        let vals: Vec<f64> = pts.iter().map(|p| p.v).collect();
        let f = if func == "anomaly-rate" { "avg" } else { func };
        return (reduce(f, &vals), anom, pts.len());
    }
    let Ok(rs) = db.rollups(1, &[key.to_string()], after, before) else {
        return (f64::NAN, 0, 0);
    };
    let Some(rs) = rs.get(key).filter(|r| !r.is_empty()) else {
        return (f64::NAN, 0, 0);
    };
    let (mut sum, mut cnt, mut anom) = (0.0, 0usize, 0usize);
    let (mut mn, mut mx, mut last) = (f64::INFINITY, f64::NEG_INFINITY, 0.0);
    for r in rs {
        sum += r.sum;
        cnt += r.count as usize;
        mn = mn.min(r.min);
        mx = mx.max(r.max);
        last = r.avg();
        anom += r.anomalous as usize;
    }
    let v = match func {
        "sum" => sum,
        "min" => mn,
        "max" | "p90" | "p95" | "p99" => mx,
        "last" => last,
        _ => sum / cnt.max(1) as f64,
    };
    (v, anom, cnt)
}

fn labels_match(want: &BTreeMap<String, String>, have: &BTreeMap<String, String>) -> bool {
    want.iter()
        .all(|(k, v)| match_glob(v, have.get(k).map(String::as_str).unwrap_or("")))
}

impl Compiled {
    /// Evaluates the lookup on one node. Also returns the node's CPU count
    /// (NaN if unknown) and the aggregate to apply.
    fn instances(&self, db: &Db, now: i64) -> (HashMap<String, Instance>, f64, &'static str) {
        let before = now;
        let after = before - self.window;
        let mut out: HashMap<String, Instance> = HashMap::new();
        let mut ncpu = 0usize;
        let mut units = String::new();
        let r = &self.rule;
        for info in db.list() {
            let s = &info.series;
            if s.context == "cpu.cpu" && s.dimension == "user" {
                ncpu += 1;
            }
            if s.context != r.context
                || !match_any(&self.charts, &s.chart)
                || !match_any(&self.dims, &s.dimension)
                || !labels_match(&r.labels, &s.labels)
            {
                continue;
            }
            if info.last_t != 0 && info.last_t < after {
                continue;
            }
            let (v, anom, total) = lookup(db, &info.key, self.func, after, before, now);
            if total == 0 {
                continue;
            }
            if units.is_empty() {
                units = s.units.clone();
            }
            let (key, chart, dim) = match self.per.as_str() {
                "dimension" => (
                    format!("{}/{}", s.chart, s.dimension),
                    s.chart.clone(),
                    s.dimension.clone(),
                ),
                "node" => (r.context.clone(), r.context.clone(), String::new()),
                _ => (s.chart.clone(), s.chart.clone(), String::new()),
            };
            let per_node = self.per == "node";
            let inst = out.entry(key).or_insert_with(|| Instance {
                chart,
                dim,
                labels: if per_node {
                    BTreeMap::new()
                } else {
                    s.labels.clone()
                },
                ..Default::default()
            });
            inst.vals.push(v);
            *inst.dim_vals.entry(s.dimension.clone()).or_default() += v;
            inst.anom += anom;
            inst.total += total;
        }
        let agg = match r.aggregate.as_str() {
            "avg" => "avg",
            "min" => "min",
            "max" => "max",
            "sum" => "sum",
            _ if units == "%" || units.starts_with("percent") => "avg",
            _ => "sum",
        };
        (out, if ncpu > 0 { ncpu as f64 } else { f64::NAN }, agg)
    }
}

fn format_value(v: f64) -> String {
    if v.is_nan() {
        "n/a".into()
    } else if v.abs() >= 100.0 || v == v.trunc() {
        format!("{v:.0}")
    } else {
        format!("{v:.2}")
    }
}

impl Engine {
    /// Compiles `opts.rules`. A rule that does not compile is an error.
    pub fn new(opts: Options) -> Result<Self, String> {
        let mut seen = HashSet::new();
        let mut rules = Vec::new();
        for r in opts.rules {
            if !seen.insert(r.name.clone()) {
                return Err(format!("duplicate rule {:?}", r.name));
            }
            rules.push(compile(r)?);
        }
        let e = Engine {
            rules,
            sources: opts.sources,
            publish: opts.publish,
            silence_file: opts.silence_file,
            history_size: if opts.history_size == 0 {
                1000
            } else {
                opts.history_size
            },
            max_alerts: if opts.max_alerts == 0 {
                50_000
            } else {
                opts.max_alerts
            },
            state: Mutex::new(State::default()),
        };
        e.load_silences();
        Ok(e)
    }

    /// Runs every rule that is due at `now` (unix seconds).
    pub fn evaluate(&self, now: i64) {
        let sources = (self.sources)();
        let mut events = Vec::new();
        {
            let mut st = self.state.lock().unwrap();
            for c in &self.rules {
                let name = &c.rule.name;
                if now < st.next_eval.get(name).copied().unwrap_or(0) {
                    continue;
                }
                st.next_eval.insert(name.clone(), now + c.every);
                st.evals += 1;
                let mut seen = HashSet::new();
                for src in &sources {
                    let (ins, ncpu, agg) = c.instances(&src.db, now);
                    for inst in ins.into_values() {
                        let id = alert_id(name, &src.node, &inst.chart, &inst.dim);
                        seen.insert(id.clone());
                        let mut val = if inst.vals.is_empty() {
                            f64::NAN
                        } else {
                            reduce(agg, &inst.vals)
                        };
                        let anom_rate = if inst.total > 0 {
                            100.0 * inst.anom as f64 / inst.total as f64
                        } else {
                            f64::NAN
                        };
                        if c.func == "anomaly-rate" {
                            val = anom_rate;
                        }
                        if !st.alerts.contains_key(&id) {
                            if st.alerts.len() >= self.max_alerts {
                                st.dropped += 1;
                                continue;
                            }
                            st.alerts.insert(
                                id.clone(),
                                Alert {
                                    id: id.clone(),
                                    rule: name.clone(),
                                    node: src.node.clone(),
                                    context: c.rule.context.clone(),
                                    chart: inst.chart.clone(),
                                    dimension: inst.dim.clone(),
                                    labels: inst.labels.clone(),
                                    status: Status::Undefined,
                                    value: NullFloat(f64::NAN),
                                    units: c.rule.units.clone(),
                                    class: c.rule.class.clone(),
                                    info: c.rule.info.clone(),
                                    since: now,
                                    last_eval: 0,
                                    silenced: false,
                                    acked: false,
                                    acked_by: String::new(),
                                    pending: None,
                                    last_notify: 0,
                                    last_seen: now,
                                },
                            );
                        }
                        st.alerts.get_mut(&id).unwrap().last_seen = now;
                        if let Some(ev) =
                            self.step(&mut st, c, &id, val, anom_rate, ncpu, &inst.dim_vals, now)
                        {
                            events.push(ev);
                        }
                    }
                }
                // Instances that produced no data this round go undefined.
                let stale: Vec<String> = st
                    .alerts
                    .values()
                    .filter(|a| a.rule == *name && !seen.contains(&a.id))
                    .map(|a| a.id.clone())
                    .collect();
                for id in stale {
                    if let Some(ev) = self.step(
                        &mut st,
                        c,
                        &id,
                        f64::NAN,
                        f64::NAN,
                        f64::NAN,
                        &HashMap::new(),
                        now,
                    ) {
                        events.push(ev);
                    }
                    let a = &st.alerts[&id];
                    if !a.status.raised() && now - a.last_seen > 3600 {
                        st.alerts.remove(&id);
                    }
                }
            }
            self.expire_silences(&mut st, now);
        }
        if let Some(p) = &self.publish {
            for ev in events {
                p(ev);
            }
        }
    }

    /// Computes the candidate status, applies the delays and records a
    /// transition. Returns a notification when one should be sent.
    #[allow(clippy::too_many_arguments)]
    fn step(
        &self,
        st: &mut State,
        c: &Compiled,
        id: &str,
        raw: f64,
        anom_rate: f64,
        ncpu: f64,
        dims: &HashMap<String, f64>,
        now: i64,
    ) -> Option<Event> {
        let silenced = {
            let a = &st.alerts[id];
            st.silences.values().any(|s| s.matches(a))
        };
        let a = st.alerts.get_mut(id)?;
        let status = a.status;
        let mut val = raw;
        let vars = |this: f64| {
            move |name: &str| -> f64 {
                match name {
                    "this" => this,
                    "status" => status.code(),
                    "UNDEFINED" | "undefined" => -1.0,
                    "CLEAR" | "clear" => 1.0,
                    "WARNING" | "warning" => 3.0,
                    "CRITICAL" | "critical" => 4.0,
                    "now" => now as f64,
                    "anomaly_rate" => anom_rate,
                    "ncpu" => ncpu,
                    n => c
                        .rule
                        .vars
                        .get(n)
                        .or_else(|| dims.get(n))
                        .copied()
                        .unwrap_or(f64::NAN),
                }
            }
        };
        if let Some(calc) = &c.calc {
            val = calc.eval(&vars(val));
        }
        a.value = NullFloat(val);
        a.last_eval = now;
        let mut cand = Status::Undefined;
        if !val.is_nan() {
            let v = vars(val);
            cand = Status::Clear;
            if c.warn.as_ref().is_some_and(|x| truth(x.eval(&v))) {
                cand = Status::Warning;
            }
            if c.crit.as_ref().is_some_and(|x| truth(x.eval(&v))) {
                cand = Status::Critical;
            }
        }
        a.silenced = silenced;
        if cand == a.status {
            a.pending = None;
            if a.status.raised()
                && c.repeat > 0
                && !a.acked
                && !a.silenced
                && now - a.last_notify >= c.repeat
            {
                a.last_notify = now;
                return Some(event(a, now));
            }
            return None;
        }
        let pending_at = match a.pending {
            Some((p, at)) if p == cand => at,
            _ => {
                a.pending = Some((cand, now));
                now
            }
        };
        let mut wait = if cand.rank() > a.status.rank() {
            c.delay_up
        } else {
            c.delay_down
        };
        // Leaving undefined for a first reading needs no delay unless it raises.
        if a.status == Status::Undefined && !cand.raised() {
            wait = 0;
        }
        if now - pending_at < wait {
            return None;
        }
        let from = a.status;
        a.status = cand;
        a.since = now;
        a.pending = None;
        a.acked = false;
        a.acked_by.clear();
        let tr = Transition {
            time: now,
            id: a.id.clone(),
            rule: a.rule.clone(),
            node: a.node.clone(),
            chart: a.chart.clone(),
            from,
            to: cand,
            value: a.value,
            units: a.units.clone(),
            info: a.info.clone(),
            silenced: a.silenced,
        };
        let notify = cand.raised() || (from.raised() && cand == Status::Clear);
        let ev = if notify && !a.silenced {
            a.last_notify = now;
            Some(event(a, now))
        } else {
            None
        };
        st.history.push(tr);
        if st.history.len() > self.history_size {
            let over = st.history.len() - self.history_size;
            st.history.drain(..over);
        }
        ev
    }

    /// Raised alerts (every instance with `all`), newest history first,
    /// rules and silences.
    pub fn snapshot(&self, all: bool, history_limit: usize) -> Snapshot {
        let st = self.state.lock().unwrap();
        let mut stats = Stats {
            rules: self.rules.len(),
            instances: st.alerts.len(),
            evaluations: st.evals,
            dropped_instances: st.dropped,
            ..Default::default()
        };
        let mut active = Vec::new();
        for a in st.alerts.values() {
            match a.status {
                Status::Warning => stats.warning += 1,
                Status::Critical => stats.critical += 1,
                _ => {}
            }
            if all || a.status.raised() {
                active.push(a.clone());
            }
        }
        active.sort_by(|a, b| {
            b.status
                .rank()
                .cmp(&a.status.rank())
                .then(b.since.cmp(&a.since))
                .then(a.id.cmp(&b.id))
        });
        let limit = if history_limit == 0 {
            st.history.len()
        } else {
            history_limit.min(st.history.len())
        };
        let history = st.history.iter().rev().take(limit).cloned().collect();
        let mut silences: Vec<Silence> = st.silences.values().cloned().collect();
        silences.sort_by_key(|s| s.until);
        Snapshot {
            active,
            history,
            rules: self.rules.iter().map(|c| c.rule.clone()).collect(),
            silences,
            stats,
        }
    }

    /// Raised alerts, most severe first.
    pub fn active(&self) -> Vec<Alert> {
        self.snapshot(false, 1).active
    }

    /// Acknowledges a raised alert until its next status change; repeats stop.
    pub fn ack(&self, id: &str, by: &str) -> Result<(), String> {
        let mut st = self.state.lock().unwrap();
        match st.alerts.get_mut(id) {
            Some(a) if a.status.raised() => {
                a.acked = true;
                a.acked_by = by.into();
                Ok(())
            }
            _ => Err("not found".into()),
        }
    }

    /// Stores `s` with a fresh ID. `until` must be in the future and at most
    /// 30 days away; at least one matcher is required.
    pub fn add_silence(&self, mut s: Silence, now: i64) -> Result<Silence, String> {
        if s.rule.is_empty() && s.node.is_empty() && s.chart.is_empty() {
            return Err("a silence needs a rule, node or chart matcher".into());
        }
        if s.until <= now || s.until - now > 30 * 86400 {
            return Err("until must be within the next 30 days".into());
        }
        s.id = random_id();
        s.created = now;
        let mut st = self.state.lock().unwrap();
        if st.silences.len() >= 1000 {
            return Err("too many silences".into());
        }
        st.silences.insert(s.id.clone(), s.clone());
        Self::refresh_silenced(&mut st);
        self.save_silences(&st);
        Ok(s)
    }

    pub fn delete_silence(&self, id: &str) -> Result<(), String> {
        let mut st = self.state.lock().unwrap();
        if st.silences.remove(id).is_none() {
            return Err("not found".into());
        }
        Self::refresh_silenced(&mut st);
        self.save_silences(&st);
        Ok(())
    }

    fn refresh_silenced(st: &mut State) {
        let silences: Vec<Silence> = st.silences.values().cloned().collect();
        for a in st.alerts.values_mut() {
            a.silenced = silences.iter().any(|s| s.matches(a));
        }
    }

    fn expire_silences(&self, st: &mut State, now: i64) {
        let before = st.silences.len();
        st.silences.retain(|_, s| now < s.until);
        if st.silences.len() != before {
            self.save_silences(st);
        }
    }

    fn load_silences(&self) {
        let Some(path) = &self.silence_file else {
            return;
        };
        let Ok(data) = std::fs::read(path) else {
            return;
        };
        let Ok(ss) = serde_json::from_slice::<Vec<Silence>>(&data) else {
            return;
        };
        let mut st = self.state.lock().unwrap();
        for s in ss.into_iter().filter(|s| !s.id.is_empty()) {
            st.silences.insert(s.id.clone(), s);
        }
    }

    fn save_silences(&self, st: &State) {
        let Some(path) = &self.silence_file else {
            return;
        };
        let ss: Vec<&Silence> = st.silences.values().collect();
        let Ok(data) = serde_json::to_vec(&ss) else {
            return;
        };
        let tmp = path.with_extension("tmp");
        if std::fs::write(&tmp, data).is_ok() {
            let _ = std::fs::rename(&tmp, path);
        }
    }
}

fn event(a: &Alert, now: i64) -> Event {
    let severity = match a.status {
        Status::Warning => "warning",
        Status::Critical => "critical",
        _ => "info",
    };
    let mut subject = a.chart.clone();
    if !a.dimension.is_empty() {
        subject = format!("{subject}/{}", a.dimension);
    }
    let mut msg = format!(
        "{} is {} on {} ({}): {} {}",
        a.rule,
        a.status.as_str(),
        a.node,
        subject,
        format_value(a.value.0),
        a.units
    );
    if !a.info.is_empty() {
        msg = format!("{} — {}", msg.trim_end(), a.info);
    }
    Event {
        alert_id: a.id.clone(),
        rule: a.rule.clone(),
        node: a.node.clone(),
        subject,
        status: a.status,
        severity,
        message: msg.trim().to_string(),
        value: a.value,
        time: now,
    }
}
