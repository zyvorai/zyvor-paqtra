use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::BTreeMap;

/// One dimension of one chart instance.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Series {
    pub context: String,
    pub chart: String,
    pub dimension: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub family: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub units: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub title: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub chart_type: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub labels: BTreeMap<String, String>,
}

impl Series {
    /// Stable identity. Family, units, title and chart type are descriptive
    /// and do not affect it.
    pub fn key(&self) -> String {
        let mut k =
            String::with_capacity(self.context.len() + self.chart.len() + self.dimension.len() + 2);
        k.push_str(&self.context);
        k.push('|');
        k.push_str(&self.chart);
        k.push('|');
        k.push_str(&self.dimension);
        for (lk, lv) in &self.labels {
            k.push('|');
            k.push_str(lk);
            k.push('=');
            k.push_str(lv);
        }
        k
    }
}

/// One value of one series at a unix second.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Sample {
    #[serde(flatten)]
    pub series: Series,
    pub t: i64,
    pub v: f64,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub a: bool,
}

/// A decoded tier-0 value.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Point {
    pub t: i64,
    pub v: f64,
    #[serde(default)]
    pub anomalous: bool,
}

/// Aggregate of one series inside one tier interval.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct Rollup {
    pub start: i64,
    pub min: f64,
    pub max: f64,
    pub sum: f64,
    pub count: u32,
    pub anomalous: u32,
}

impl Rollup {
    pub fn add(&mut self, v: f64, anomalous: bool) {
        if self.count == 0 {
            self.min = v;
            self.max = v;
        } else {
            self.min = self.min.min(v);
            self.max = self.max.max(v);
        }
        self.sum += v;
        self.count += 1;
        if anomalous {
            self.anomalous += 1;
        }
    }

    pub fn avg(&self) -> f64 {
        if self.count == 0 {
            f64::NAN
        } else {
            self.sum / self.count as f64
        }
    }
}

/// Serializes NaN and infinities as JSON null.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NullFloat(pub f64);

impl Serialize for NullFloat {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        if self.0.is_finite() {
            s.serialize_f64(self.0)
        } else {
            s.serialize_none()
        }
    }
}

impl<'de> Deserialize<'de> for NullFloat {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(NullFloat(
            Option::<f64>::deserialize(d)?.unwrap_or(f64::NAN),
        ))
    }
}

/// `*` matches any run of characters. An empty pattern matches everything.
pub fn match_glob(pattern: &str, s: &str) -> bool {
    if pattern.is_empty() || pattern == "*" {
        return true;
    }
    if !pattern.contains('*') {
        return pattern == s;
    }
    let parts: Vec<&str> = pattern.split('*').collect();
    let Some(mut rest) = s.strip_prefix(parts[0]) else {
        return false;
    };
    for p in &parts[1..parts.len() - 1] {
        match rest.find(p) {
            Some(i) => rest = &rest[i + p.len()..],
            None => return false,
        }
    }
    rest.ends_with(parts[parts.len() - 1])
}

/// No patterns match everything.
pub fn match_any(patterns: &[String], s: &str) -> bool {
    patterns.is_empty() || patterns.iter().any(|p| match_glob(p, s))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_ignores_descriptive_fields_and_sorts_labels() {
        let mut a = Series {
            context: "net.net".into(),
            chart: "net.eth0".into(),
            dimension: "received".into(),
            units: "kilobits/s".into(),
            ..Default::default()
        };
        a.labels.insert("z".into(), "1".into());
        a.labels.insert("a".into(), "2".into());
        let mut b = a.clone();
        b.units.clear();
        assert_eq!(a.key(), b.key());
        assert_eq!(a.key(), "net.net|net.eth0|received|a=2|z=1");
    }

    #[test]
    fn globs() {
        assert!(match_glob("", "x"));
        assert!(match_glob("net.*", "net.eth0"));
        assert!(match_glob("*eth*", "net.eth0"));
        assert!(match_glob("a*c*e", "abcde"));
        assert!(!match_glob("a*c*e", "abcdf"));
        assert!(!match_glob("net.eth0", "net.eth1"));
        assert!(match_any(&[], "x"));
        assert!(match_any(&["y".into(), "x*".into()], "xx"));
    }

    #[test]
    fn null_float_json() {
        let v = vec![NullFloat(1.5), NullFloat(f64::NAN)];
        let s = serde_json::to_string(&v).unwrap();
        assert_eq!(s, "[1.5,null]");
        let back: Vec<NullFloat> = serde_json::from_str(&s).unwrap();
        assert!(back[1].0.is_nan());
    }
}
