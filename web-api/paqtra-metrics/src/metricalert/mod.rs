//! Threshold and anomaly-rate rules evaluated against the per-second store.
//! Transitions go to a publish callback (the API wires it to the notifier).
//! Read-only with respect to the cluster: a rule can raise an alert, never
//! change policy or the datapath. See docs/metric-alerts.md.

mod engine;
mod expr;
mod rules;

pub use engine::{
    Alert, Engine, Event, Options, Publish, Silence, Snapshot, Sources, Stats, Status, Transition,
};
pub use expr::{compile as compile_expr, Expr};
pub use rules::{default_rules, load_rules, parse_rules, Rule};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tsdb::{Db, Options as DbOptions, Sample, Series, Source};
    use std::sync::{Arc, Mutex};

    fn series(ctx: &str, chart: &str, dim: &str, units: &str) -> Series {
        Series {
            context: ctx.into(),
            chart: chart.into(),
            dimension: dim.into(),
            units: units.into(),
            ..Default::default()
        }
    }

    struct Rig {
        db: Arc<Db>,
        engine: Engine,
        events: Arc<Mutex<Vec<Event>>>,
    }

    fn rig(yaml: &str) -> Rig {
        let db = Arc::new(Db::open(DbOptions::default()).unwrap());
        let events = Arc::new(Mutex::new(Vec::new()));
        let (d, ev) = (db.clone(), events.clone());
        let engine = Engine::new(Options {
            rules: parse_rules(yaml, "test").unwrap(),
            sources: Arc::new(move || {
                vec![Source {
                    node: "n1".into(),
                    db: d.clone(),
                }]
            }),
            publish: Some(Arc::new(move |e| ev.lock().unwrap().push(e))),
            silence_file: None,
            history_size: 0,
            max_alerts: 0,
        })
        .unwrap();
        Rig { db, engine, events }
    }

    fn put(db: &Db, s: &Series, from: i64, to: i64, v: f64) {
        for t in from..to {
            db.append(&Sample {
                series: s.clone(),
                t,
                v,
                a: false,
            })
            .unwrap();
        }
    }

    const RAM: &str = "alerts:\n  - alarm: ram\n    on: mem.used_percent\n    lookup: average -10s\n    units: '%'\n    every: 1s\n    warn: '$this > (($status >= $WARNING) ? 80 : 90)'\n    crit: $this > 97\n    delay_down: 5s\n";

    #[test]
    fn raises_with_hysteresis_and_delay_down() {
        let r = rig(RAM);
        let s = series("mem.used_percent", "mem.used_percent", "used", "%");
        put(&r.db, &s, 1000, 1011, 95.0);
        r.engine.evaluate(1010);
        let a = r.engine.active();
        assert_eq!(a.len(), 1);
        assert_eq!(a[0].status, Status::Warning);
        assert_eq!(r.events.lock().unwrap().len(), 1);
        assert_eq!(r.events.lock().unwrap()[0].severity, "warning");

        // 85 stays warning thanks to hysteresis (80 once raised).
        put(&r.db, &s, 1011, 1031, 85.0);
        r.engine.evaluate(1030);
        assert_eq!(r.engine.active()[0].status, Status::Warning);

        // Clear needs delay_down (5 s) of good readings.
        put(&r.db, &s, 1031, 1051, 50.0);
        r.engine.evaluate(1050);
        assert_eq!(r.engine.active().len(), 1);
        r.engine.evaluate(1056);
        assert!(r.engine.active().is_empty());
        let ev = r.events.lock().unwrap();
        assert_eq!(ev.last().unwrap().status, Status::Clear);
        let snap = r.engine.snapshot(true, 0);
        assert_eq!(snap.history[0].to, Status::Clear);
        assert_eq!(snap.stats.rules, 1);
    }

    #[test]
    fn calc_uses_dimension_variables_and_missing_data_is_undefined() {
        let yaml = "alerts:\n  - alarm: swap\n    on: mem.swap\n    lookup: average -10s\n    every: 1s\n    calc: '($used + $free) > 0 ? $used * 100 / ($used + $free) : nan'\n    warn: $this > 50\n";
        let r = rig(yaml);
        put(
            &r.db,
            &series("mem.swap", "mem.swap", "used", "MiB"),
            1000,
            1011,
            60.0,
        );
        put(
            &r.db,
            &series("mem.swap", "mem.swap", "free", "MiB"),
            1000,
            1011,
            40.0,
        );
        r.engine.evaluate(1010);
        let a = r.engine.active();
        assert_eq!(a.len(), 1);
        assert!((a[0].value.0 - 60.0).abs() < 1e-9);
        // No data for longer than the window: instance goes undefined, not clear.
        r.engine.evaluate(1100);
        assert!(r.engine.active().is_empty());
        let all = r.engine.snapshot(true, 0);
        assert_eq!(all.active[0].status, Status::Undefined);
    }

    #[test]
    fn silence_suppresses_notifications_and_ack_stops_repeat() {
        let yaml = format!("{RAM}    repeat: 10s\n");
        let r = rig(&yaml);
        let s = series("mem.used_percent", "mem.used_percent", "used", "%");
        let sil = r
            .engine
            .add_silence(
                Silence {
                    rule: "ra*".into(),
                    until: 2000,
                    ..Default::default()
                },
                1000,
            )
            .unwrap();
        put(&r.db, &s, 1000, 1100, 99.0);
        r.engine.evaluate(1010);
        assert_eq!(r.engine.active()[0].status, Status::Critical);
        assert!(r.engine.active()[0].silenced);
        assert!(r.events.lock().unwrap().is_empty());

        r.engine.delete_silence(&sil.id).unwrap();
        r.engine.evaluate(1020);
        assert_eq!(r.events.lock().unwrap().len(), 1, "repeat after unsilence");
        let id = r.engine.active()[0].id.clone();
        r.engine.ack(&id, "ops").unwrap();
        r.engine.evaluate(1040);
        assert_eq!(
            r.events.lock().unwrap().len(),
            1,
            "acked alerts do not repeat"
        );

        assert!(r.engine.add_silence(Silence::default(), 1000).is_err());
        assert!(r
            .engine
            .add_silence(
                Silence {
                    rule: "x".into(),
                    until: 999,
                    ..Default::default()
                },
                1000
            )
            .is_err());
    }

    #[test]
    fn per_dimension_and_anomaly_rate() {
        let yaml = "alerts:\n  - alarm: app_down\n    on: apps.up\n    lookup: last -10s\n    per: dimension\n    every: 1s\n    warn: $this == 0\n  - alarm: anom\n    on: system.cpu\n    lookup: anomaly-rate -10s\n    per: node\n    every: 1s\n    warn: $this > 25\n";
        let r = rig(yaml);
        put(
            &r.db,
            &series("apps.up", "apps.up", "redis", "up"),
            1000,
            1011,
            1.0,
        );
        put(
            &r.db,
            &series("apps.up", "apps.up", "nginx", "up"),
            1000,
            1011,
            0.0,
        );
        let cpu = series("system.cpu", "system.cpu", "user", "%");
        for t in 1000..1011 {
            r.db.append(&Sample {
                series: cpu.clone(),
                t,
                v: 1.0,
                a: t % 2 == 0,
            })
            .unwrap();
        }
        r.engine.evaluate(1010);
        let a = r.engine.active();
        assert_eq!(a.len(), 2);
        assert!(a
            .iter()
            .any(|x| x.rule == "app_down" && x.dimension == "nginx"));
        assert!(a
            .iter()
            .any(|x| x.rule == "anom" && x.chart == "system.cpu"));
    }

    #[test]
    fn silences_persist() {
        let d = tempfile::tempdir().unwrap();
        let file = d.path().join("silences.json");
        let mk = || {
            Engine::new(Options {
                rules: parse_rules(RAM, "t").unwrap(),
                sources: Arc::new(Vec::new),
                publish: None,
                silence_file: Some(file.clone()),
                history_size: 0,
                max_alerts: 0,
            })
            .unwrap()
        };
        mk().add_silence(
            Silence {
                node: "n*".into(),
                until: 5000,
                ..Default::default()
            },
            1000,
        )
        .unwrap();
        assert_eq!(mk().snapshot(false, 0).silences.len(), 1);
    }
}
