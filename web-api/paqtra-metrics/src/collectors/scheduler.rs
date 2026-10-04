use super::{labels, Chart, Collector, Emitter, Info};
use crate::tsdb::Sample;
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Receives each run's samples. Must be safe for concurrent use.
pub type Sink = Arc<dyn Fn(Vec<Sample>) + Send + Sync>;

/// Health of one collector.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub info: Info,
    pub runs: u64,
    pub errors: u64,
    pub skipped: u64,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub last_error: String,
    pub last_run: i64,
    pub last_millis: f64,
    pub samples: usize,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub disabled: bool,
}

struct Entry {
    info: Info,
    work: Mutex<(Box<dyn Collector>, Emitter)>,
    running: AtomicBool,
    st: Mutex<(Status, u32)>,
}

/// Runs collectors on their own cadence. A collector still running when its
/// next tick arrives is skipped and counted; it never delays the others. A
/// collector that fails 30 times in a row without output is disabled.
pub struct Scheduler {
    entries: Vec<Arc<Entry>>,
    sink: Sink,
    self_em: Mutex<Emitter>,
}

impl Scheduler {
    pub fn new(sink: Sink, cs: Vec<Box<dyn Collector>>) -> Self {
        let entries = cs
            .into_iter()
            .map(|c| {
                let mut info = c.info();
                info.every = info.every.max(1);
                Arc::new(Entry {
                    st: Mutex::new((
                        Status {
                            info: info.clone(),
                            runs: 0,
                            errors: 0,
                            skipped: 0,
                            last_error: String::new(),
                            last_run: 0,
                            last_millis: 0.0,
                            samples: 0,
                            disabled: false,
                        },
                        0,
                    )),
                    info,
                    work: Mutex::new((c, Emitter::new())),
                    running: AtomicBool::new(false),
                })
            })
            .collect();
        Self {
            entries,
            sink,
            self_em: Mutex::new(Emitter::new()),
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Every collector's status sorted by name.
    pub fn statuses(&self) -> Vec<Status> {
        let mut out: Vec<Status> = self
            .entries
            .iter()
            .map(|e| e.st.lock().unwrap().0.clone())
            .collect();
        out.sort_by(|a, b| a.info.name.cmp(&b.info.name));
        out
    }

    /// Runs every collector synchronously. Used by tests.
    pub fn run_once(&self, now: i64) {
        for e in &self.entries {
            run_entry(e, now, &self.sink);
        }
        self.emit_self(now);
    }

    fn tick(&self, now: i64) {
        for e in &self.entries {
            if e.info.every > 1 && now % e.info.every as i64 != 0 {
                continue;
            }
            if e.st.lock().unwrap().0.disabled {
                continue;
            }
            if e.running.swap(true, Ordering::AcqRel) {
                e.st.lock().unwrap().0.skipped += 1;
                continue;
            }
            let e = e.clone();
            let sink = self.sink.clone();
            std::thread::spawn(move || {
                run_entry(&e, now, &sink);
                e.running.store(false, Ordering::Release);
            });
        }
        self.emit_self(now);
    }

    /// Ticks at one-second boundaries on a background thread until `stop`.
    pub fn spawn(self: Arc<Self>, stop: Arc<AtomicBool>) -> std::thread::JoinHandle<()> {
        std::thread::Builder::new()
            .name("metrics-scheduler".into())
            .spawn(move || {
                while !stop.load(Ordering::Relaxed) {
                    let now = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap_or_default();
                    let next = Duration::from_secs(now.as_secs() + 1);
                    std::thread::sleep(next.saturating_sub(now));
                    if stop.load(Ordering::Relaxed) {
                        break;
                    }
                    self.tick(next.as_secs() as i64);
                }
            })
            .expect("spawn metrics scheduler")
    }

    /// Publishes the scheduler's own health as paqtra.collector_* metrics.
    fn emit_self(&self, now: i64) {
        let mut em = self.self_em.lock().unwrap();
        em.begin(now);
        for st in self.statuses() {
            let lbl = labels([("collector", st.info.name.as_str())]);
            let mk = |ctx: &str, units: &str, title: &str| {
                Chart::new(ctx, "paqtra", units, title)
                    .id(format!("{ctx}_{}", st.info.name))
                    .labels(&lbl)
            };
            em.gauge(
                &mk("paqtra.collector_duration", "ms", "Collector run time"),
                "duration",
                st.last_millis,
            );
            let ev = mk(
                "paqtra.collector_events",
                "events/s",
                "Collector errors and skipped runs",
            );
            em.incremental(&ev, "errors", st.errors as f64, 1.0);
            em.incremental(&ev, "skipped", st.skipped as f64, 1.0);
            em.gauge(
                &mk("paqtra.collector_disabled", "boolean", "Collector disabled"),
                "disabled",
                if st.disabled { 1.0 } else { 0.0 },
            );
        }
        em.end();
        let out = em.take();
        drop(em);
        if !out.is_empty() {
            (self.sink)(out);
        }
    }
}

fn run_entry(e: &Entry, now: i64, sink: &Sink) {
    let start = Instant::now();
    let (res, samples) = {
        let mut w = e.work.lock().unwrap();
        let (c, em) = &mut *w;
        em.begin(now);
        let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| c.collect(now, em)))
            .unwrap_or_else(|_| Err("panic".into()));
        em.end();
        (res, em.take())
    };
    {
        let mut g = e.st.lock().unwrap();
        let (st, streak) = &mut *g;
        st.runs += 1;
        st.last_run = now;
        st.last_millis = start.elapsed().as_secs_f64() * 1000.0;
        st.samples = samples.len();
        match &res {
            Err(err) => {
                st.errors += 1;
                st.last_error = err.clone();
                *streak += 1;
                if *streak >= 30 && samples.is_empty() {
                    st.disabled = true;
                }
            }
            Ok(()) => {
                *streak = 0;
                st.last_error.clear();
            }
        }
    }
    if !samples.is_empty() {
        sink(samples);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fails;
    impl Collector for Fails {
        fn info(&self) -> Info {
            Info::new("fails", "test", 1)
        }
        fn collect(&mut self, _now: i64, _e: &mut Emitter) -> Result<(), String> {
            Err("nope".into())
        }
    }

    struct Panics;
    impl Collector for Panics {
        fn info(&self) -> Info {
            Info::new("panics", "test", 1)
        }
        fn collect(&mut self, _now: i64, _e: &mut Emitter) -> Result<(), String> {
            panic!("boom")
        }
    }

    struct One;
    impl Collector for One {
        fn info(&self) -> Info {
            Info::new("one", "test", 1)
        }
        fn collect(&mut self, _now: i64, e: &mut Emitter) -> Result<(), String> {
            e.gauge(&Chart::new("t.one", "t", "x", "One"), "v", 1.0);
            Ok(())
        }
    }

    #[test]
    fn statuses_disable_and_self_metrics() {
        let got = Arc::new(Mutex::new(Vec::<Sample>::new()));
        let g = got.clone();
        let s = Scheduler::new(
            Arc::new(move |v| g.lock().unwrap().extend(v)),
            vec![Box::new(Fails), Box::new(Panics), Box::new(One)],
        );
        for t in 0..31 {
            s.run_once(1000 + t);
        }
        let st = s.statuses();
        assert_eq!(st[0].info.name, "fails");
        assert!(st[0].disabled);
        assert_eq!(st[1].info.name, "one");
        assert_eq!(st[1].runs, 31);
        assert_eq!(st[2].last_error, "panic");
        let got = got.lock().unwrap();
        assert!(got.iter().any(|s| s.series.context == "t.one"));
        assert!(got
            .iter()
            .any(|s| s.series.context == "paqtra.collector_disabled" && s.v == 1.0));
    }
}
