//! Agent-to-API metric stream. The agent's tier-0 store doubles as the replay
//! buffer: after a disconnect or an API restart the API reports the newest
//! second it holds for the node and the agent resends everything after it.

use crate::tsdb::{Sample, Series, SeriesPoints};
use flate2::{read::GzDecoder, write::GzEncoder, Compression};
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};

/// The API's ingest endpoint.
pub const PATH: &str = "/api/v1/agents/metrics";

/// Caps the decompressed size of one batch.
pub const MAX_DECODED: u64 = 64 << 20;
/// Caps the compressed body the API accepts.
pub const MAX_COMPRESSED: usize = 16 << 20;

/// One POST body.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Batch {
    pub node: String,
    /// Exclusive.
    pub from: i64,
    /// Inclusive.
    pub to: i64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub series: Vec<WireSeries>,
}

/// Points travel as `[t, v, anomalous]` triples.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WireSeries {
    #[serde(flatten)]
    pub series: Series,
    pub points: Vec<[f64; 3]>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Response {
    /// The node's newest stored second before this batch.
    pub prev_last_t: i64,
    pub last_t: i64,
    pub stored: usize,
    /// `prev_last_t` is older than `Batch::from`: nothing was stored and the
    /// agent replays from `prev_last_t` so seconds arrive in order.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub gap: bool,
}

/// Converts a tsdb batch, dropping non-finite values JSON cannot carry.
pub fn from_series_points(input: Vec<SeriesPoints>) -> Vec<WireSeries> {
    input
        .into_iter()
        .filter_map(|sp| {
            let points: Vec<[f64; 3]> = sp
                .points
                .iter()
                .filter(|p| p.v.is_finite())
                .map(|p| [p.t as f64, p.v, if p.anomalous { 1.0 } else { 0.0 }])
                .collect();
            (!points.is_empty()).then_some(WireSeries {
                series: sp.series,
                points,
            })
        })
        .collect()
}

impl Batch {
    /// Flattens into samples in time order per series.
    pub fn samples(&self) -> Vec<Sample> {
        self.series
            .iter()
            .flat_map(|ws| {
                ws.points.iter().map(move |p| Sample {
                    series: ws.series.clone(),
                    t: p[0] as i64,
                    v: p[1],
                    a: p[2] != 0.0,
                })
            })
            .collect()
    }
}

pub fn encode(b: &Batch) -> std::io::Result<Vec<u8>> {
    let mut zw = GzEncoder::new(Vec::new(), Compression::fast());
    serde_json::to_writer(&mut zw, b)?;
    zw.flush()?;
    zw.finish()
}

/// Reads a gzip or plain JSON batch, refusing more than `MAX_DECODED` bytes
/// after decompression.
pub fn decode(body: &[u8], gzipped: bool) -> Result<Batch, String> {
    let mut buf = Vec::new();
    let read = if gzipped {
        GzDecoder::new(body)
            .take(MAX_DECODED + 1)
            .read_to_end(&mut buf)
    } else {
        body.take(MAX_DECODED + 1).read_to_end(&mut buf)
    };
    read.map_err(|e| format!("gzip: {e}"))?;
    if buf.len() as u64 > MAX_DECODED {
        return Err("batch exceeds size limit".into());
    }
    serde_json::from_slice(&buf).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tsdb::Point;

    #[test]
    fn roundtrip_drops_non_finite() {
        let s = Series {
            context: "net.net".into(),
            chart: "net.eth0".into(),
            dimension: "received".into(),
            labels: [("interface".to_string(), "eth0".to_string())].into(),
            ..Default::default()
        };
        let pts = vec![
            Point {
                t: 10,
                v: 1.5,
                anomalous: false,
            },
            Point {
                t: 11,
                v: f64::NAN,
                anomalous: false,
            },
            Point {
                t: 12,
                v: 2.5,
                anomalous: true,
            },
        ];
        let b = Batch {
            node: "n1".into(),
            from: 9,
            to: 12,
            series: from_series_points(vec![SeriesPoints {
                series: s.clone(),
                points: pts,
            }]),
        };
        let enc = encode(&b).unwrap();
        let got = decode(&enc, true).unwrap();
        let smp = got.samples();
        assert_eq!(smp.len(), 2);
        assert_eq!(smp[1].t, 12);
        assert!(smp[1].a);
        assert_eq!(smp[0].series.key(), s.key());
        assert!(decode(b"{not json", false).is_err());
    }

    /// Pins the JSON shape both crates speak.
    #[test]
    fn wire_shape() {
        let j = r#"{"node":"n","from":1,"to":2,"series":[{"context":"c","chart":"c","dimension":"d","chartType":"line","labels":{"pod":"p"},"points":[[2,3.5,0]]}]}"#;
        let b = decode(j.as_bytes(), false).unwrap();
        assert_eq!(b.series[0].series.chart_type, "line");
        assert_eq!(b.series[0].series.labels["pod"], "p");
        let r = Response {
            prev_last_t: 1,
            last_t: 2,
            stored: 1,
            gap: false,
        };
        assert_eq!(
            serde_json::to_string(&r).unwrap(),
            r#"{"prevLastT":1,"lastT":2,"stored":1}"#
        );
    }
}
