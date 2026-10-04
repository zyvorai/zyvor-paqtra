use super::expr::{compile as compile_expr, Expr};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::path::Path;

const DEFAULTS: &str = include_str!("defaults.yaml");

/// One alert definition as written in YAML.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rule {
    #[serde(rename(deserialize = "alarm", serialize = "name"), alias = "name")]
    pub name: String,
    #[serde(rename(deserialize = "on", serialize = "context"), alias = "context")]
    pub context: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub charts: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub dimensions: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub labels: BTreeMap<String, String>,
    pub lookup: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub aggregate: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub per: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub vars: BTreeMap<String, f64>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub calc: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub warn: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub crit: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub every: String,
    #[serde(default, alias = "delay_up", skip_serializing_if = "String::is_empty")]
    pub delay_up: String,
    #[serde(
        default,
        alias = "delay_down",
        skip_serializing_if = "String::is_empty"
    )]
    pub delay_down: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub repeat: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub units: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub class: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub info: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(default, skip_deserializing)]
    pub source: String,
}

#[derive(Deserialize)]
struct RuleFile {
    #[serde(default)]
    alerts: Vec<Rule>,
}

pub(crate) struct Compiled {
    pub rule: Rule,
    pub per: String,
    pub func: &'static str,
    pub window: i64,
    pub charts: Vec<String>,
    pub dims: Vec<String>,
    pub calc: Option<Expr>,
    pub warn: Option<Expr>,
    pub crit: Option<Expr>,
    pub every: i64,
    pub delay_up: i64,
    pub delay_down: i64,
    pub repeat: i64,
}

fn lookup_fn(s: &str) -> Option<&'static str> {
    Some(match s.to_lowercase().as_str() {
        "average" | "avg" | "mean" => "avg",
        "min" => "min",
        "max" => "max",
        "sum" => "sum",
        "last" => "last",
        "median" | "p50" => "p50",
        "p90" => "p90",
        "p95" => "p95",
        "p99" => "p99",
        "anomaly-rate" | "anomaly_rate" => "anomaly-rate",
        _ => return None,
    })
}

fn split_globs(s: &str) -> Vec<String> {
    let f: Vec<String> = s
        .split([',', ' ', '|'])
        .filter(|x| !x.is_empty())
        .map(str::to_string)
        .collect();
    if f.len() == 1 && f[0] == "*" {
        Vec::new()
    } else {
        f
    }
}

/// Seconds from "30", "30s", "5m", "2h" or "7d". Empty is `default`.
pub(crate) fn parse_dur(s: &str, default: i64) -> Result<i64, String> {
    let s = s.trim();
    if s.is_empty() {
        return Ok(default);
    }
    if let Ok(n) = s.parse::<i64>() {
        return Ok(n);
    }
    let (num, unit) = s.split_at(s.len() - 1);
    let n: f64 = num.parse().map_err(|_| format!("bad duration {s:?}"))?;
    let mult = match unit {
        "s" => 1.0,
        "m" => 60.0,
        "h" => 3600.0,
        "d" => 86400.0,
        _ => return Err(format!("bad duration {s:?}")),
    };
    Ok((n * mult) as i64)
}

pub(crate) fn compile(r: Rule) -> Result<Compiled, String> {
    if r.name.is_empty() {
        return Err("alarm name is required".into());
    }
    if r.context.is_empty() {
        return Err(format!("{}: on (context) is required", r.name));
    }
    let f: Vec<&str> = r.lookup.split_whitespace().collect();
    if f.len() != 2 {
        return Err(format!("{}: lookup must be '<function> -<window>'", r.name));
    }
    let func =
        lookup_fn(f[0]).ok_or_else(|| format!("{}: unknown lookup function {:?}", r.name, f[0]))?;
    let window = parse_dur(f[1].trim_start_matches('-'), 0)
        .ok()
        .filter(|w| (1..=31 * 86400).contains(w))
        .ok_or_else(|| format!("{}: bad lookup window {:?}", r.name, f[1]))?;
    if !["", "sum", "avg", "min", "max"].contains(&r.aggregate.as_str()) {
        return Err(format!(
            "{}: aggregate must be sum, avg, min or max",
            r.name
        ));
    }
    let per = match r.per.as_str() {
        "" => "chart".to_string(),
        "chart" | "dimension" | "node" => r.per.clone(),
        _ => return Err(format!("{}: per must be chart, dimension or node", r.name)),
    };
    let ex = |s: &str| compile_expr(s).map_err(|e| format!("{}: {e}", r.name));
    let (calc, warn, crit) = (ex(&r.calc)?, ex(&r.warn)?, ex(&r.crit)?);
    if warn.is_none() && crit.is_none() {
        return Err(format!("{}: needs warn or crit", r.name));
    }
    let dur = |s: &str, d: i64| {
        parse_dur(s, d)
            .ok()
            .filter(|v| *v >= 0)
            .ok_or_else(|| format!("{}: bad duration {s:?}", r.name))
    };
    Ok(Compiled {
        per,
        func,
        window,
        charts: split_globs(&r.charts),
        dims: split_globs(&r.dimensions),
        calc,
        warn,
        crit,
        every: dur(&r.every, 10)?.max(1),
        delay_up: dur(&r.delay_up, 0)?,
        delay_down: dur(&r.delay_down, 0)?,
        repeat: dur(&r.repeat, 0)?,
        rule: r,
    })
}

/// Parses one YAML document of rules (`alerts: [...]`).
pub fn parse_rules(yaml: &str, source: &str) -> Result<Vec<Rule>, String> {
    let f: RuleFile = serde_yaml::from_str(yaml).map_err(|e| format!("{source}: {e}"))?;
    Ok(f.alerts
        .into_iter()
        .map(|mut r| {
            r.source = source.into();
            r
        })
        .collect())
}

/// The built-in rule pack.
pub fn default_rules() -> Vec<Rule> {
    parse_rules(DEFAULTS, "builtin").expect("built-in metric alert rules parse")
}

/// The built-in pack (unless `include_defaults` is false) overlaid with
/// `*.yaml`/`*.yml` files from `dirs`. A rule with the same name replaces an
/// earlier one; `enabled: false` removes it.
pub fn load_rules(include_defaults: bool, dirs: &[&Path]) -> Result<Vec<Rule>, String> {
    let mut by_name: HashMap<String, Rule> = HashMap::new();
    let mut order = Vec::new();
    let mut add = |rs: Vec<Rule>| {
        for r in rs {
            if !by_name.contains_key(&r.name) {
                order.push(r.name.clone());
            }
            by_name.insert(r.name.clone(), r);
        }
    };
    if include_defaults {
        add(default_rules());
    }
    for dir in dirs {
        let rd = match std::fs::read_dir(dir) {
            Ok(rd) => rd,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(format!("{}: {e}", dir.display())),
        };
        let mut names: Vec<_> = rd
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.is_file()
                    && matches!(p.extension().and_then(|x| x.to_str()), Some("yaml" | "yml"))
            })
            .collect();
        names.sort();
        for p in names {
            let data = std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?;
            add(parse_rules(&data, &p.display().to_string())?);
        }
    }
    Ok(order
        .into_iter()
        .filter_map(|n| by_name.remove(&n))
        .filter(|r| r.enabled != Some(false))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_compile_and_count() {
        let rs = default_rules();
        assert!(rs.len() >= 40, "{} rules", rs.len());
        for r in rs {
            let name = r.name.clone();
            compile(r).unwrap_or_else(|e| panic!("{name}: {e}"));
        }
    }

    #[test]
    fn durations() {
        assert_eq!(parse_dur("90", 0), Ok(90));
        assert_eq!(parse_dur("5m", 0), Ok(300));
        assert_eq!(parse_dur("2d", 0), Ok(172800));
        assert_eq!(parse_dur("", 7), Ok(7));
        assert!(parse_dur("5x", 0).is_err());
    }

    #[test]
    fn overlay_replaces_and_disables() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(
            d.path().join("10-local.yaml"),
            "alerts:\n  - alarm: ram_in_use\n    on: mem.used_percent\n    lookup: average -1m\n    warn: $this > 50\n  - alarm: oom_kill\n    on: mem.oom_kill\n    lookup: sum -1m\n    warn: $this > 0\n    enabled: false\n",
        )
        .unwrap();
        let rs = load_rules(true, &[d.path()]).unwrap();
        let ram = rs.iter().find(|r| r.name == "ram_in_use").unwrap();
        assert_eq!(ram.warn, "$this > 50");
        assert!(ram.source.ends_with("10-local.yaml"));
        assert!(!rs.iter().any(|r| r.name == "oom_kill"));
    }

    #[test]
    fn compile_errors() {
        let base = Rule {
            name: "x".into(),
            context: "c".into(),
            lookup: "avg -1m".into(),
            warn: "$this > 1".into(),
            ..Default::default()
        };
        assert!(compile(base.clone()).is_ok());
        for (field, val) in [
            ("lookup", "avg"),
            ("lookup", "bogus -1m"),
            ("per", "pod"),
            ("warn", ""),
        ] {
            let mut r = base.clone();
            match field {
                "lookup" => r.lookup = val.into(),
                "per" => r.per = val.into(),
                _ => r.warn = val.into(),
            }
            assert!(compile(r).is_err(), "{field}={val}");
        }
    }
}
