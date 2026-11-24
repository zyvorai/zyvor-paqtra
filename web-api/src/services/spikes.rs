// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

//! Drop spikes: when did dropping jump above its normal level.
//!
//! Takes the per-bucket drop counts the timeline already computes and finds
//! episodes where drops sit well above the window's own typical level. The
//! level is the median of dropped flows per bucket and the spread is the
//! median absolute deviation, so a few big spikes do not hide themselves by
//! raising the average. Adjacent spiking buckets are one episode. Pure.

use super::flow_agg::round;
use super::flow_store::TimelineBucket;
use serde::Serialize;

pub const DEFAULT_SENSITIVITY: f64 = 3.0;
pub const MIN_SENSITIVITY: f64 = 1.0;
pub const MAX_SENSITIVITY: f64 = 20.0;
pub const DEFAULT_MIN_DROPS: u64 = 10;
pub const MAX_MIN_DROPS: u64 = 1_000_000;
/// Fewer buckets than this have no meaningful "normal".
pub const MIN_BUCKETS: usize = 8;
pub const MAX_EPISODES: usize = 50;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Episode {
    /// Unix seconds of the first spiking bucket.
    pub start: i64,
    /// Unix seconds when the last spiking bucket ends.
    pub end: i64,
    pub buckets: usize,
    pub dropped: u64,
    pub forwarded: u64,
    pub peak_dropped: u64,
    pub peak_start: i64,
    /// Peak bucket's drops as a multiple of the typical bucket (null when
    /// the typical bucket had none).
    pub peak_vs_typical: Option<f64>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SpikeReport {
    pub bucket_secs: i64,
    pub buckets: usize,
    pub status: &'static str,
    pub typical_dropped_per_bucket: f64,
    pub threshold_dropped_per_bucket: f64,
    pub sensitivity: f64,
    pub min_drops: u64,
    pub episodes_total: usize,
    pub episodes: Vec<Episode>,
    pub note: &'static str,
}

fn median(v: &mut [f64]) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    v.sort_by(|a, b| a.total_cmp(b));
    let m = v.len() / 2;
    if v.len() % 2 == 1 {
        v[m]
    } else {
        (v[m - 1] + v[m]) / 2.0
    }
}

/// Find drop spikes in gap-filled `buckets`. `sensitivity` (NaN = default) is
/// clamped to 1..=20 and is how many scaled deviations above the median a
/// bucket must be; `min_drops` is a floor so a quiet cluster's 3 drops are
/// not an incident.
pub fn detect(
    buckets: &[TimelineBucket],
    bucket_secs: i64,
    sensitivity: f64,
    min_drops: u64,
) -> SpikeReport {
    let sensitivity = if sensitivity.is_nan() {
        DEFAULT_SENSITIVITY
    } else {
        sensitivity.clamp(MIN_SENSITIVITY, MAX_SENSITIVITY)
    };
    let min_drops = min_drops.clamp(1, MAX_MIN_DROPS);
    let note = "Compared with this window's own median; a window that is mostly spike has no \
                quiet baseline. Counts are over sampled flows. Notifies only.";
    let base = |status, typical, threshold, episodes: Vec<Episode>| SpikeReport {
        bucket_secs,
        buckets: buckets.len(),
        status,
        typical_dropped_per_bucket: round(typical, 2),
        threshold_dropped_per_bucket: round(threshold, 2),
        sensitivity,
        min_drops,
        episodes_total: episodes.len(),
        episodes: episodes.into_iter().take(MAX_EPISODES).collect(),
        note,
    };
    if buckets.len() < MIN_BUCKETS {
        return base("insufficient_data", 0.0, 0.0, vec![]);
    }

    let mut d: Vec<f64> = buckets.iter().map(|b| b.dropped as f64).collect();
    let med = median(&mut d);
    let mut dev: Vec<f64> = d.iter().map(|x| (x - med).abs()).collect();
    let mad = median(&mut dev);
    // 1.4826 makes MAD comparable to a standard deviation for normal data. A
    // zero MAD (flat series) falls back to the square root of the median so
    // Poisson-like counts still get some tolerance.
    let spread = if mad > 0.0 {
        1.4826 * mad
    } else {
        med.max(1.0).sqrt()
    };
    let threshold = (med + sensitivity * spread).max(min_drops as f64);

    let mut episodes: Vec<Episode> = Vec::new();
    let mut cur: Option<Episode> = None;
    for b in buckets {
        let hot = b.dropped as f64 >= threshold;
        match (hot, cur.as_mut()) {
            (true, Some(e)) => {
                e.end = b.start + bucket_secs;
                e.buckets += 1;
                e.dropped = e.dropped.saturating_add(b.dropped);
                e.forwarded = e.forwarded.saturating_add(b.forwarded);
                if b.dropped > e.peak_dropped {
                    e.peak_dropped = b.dropped;
                    e.peak_start = b.start;
                }
            }
            (true, None) => {
                cur = Some(Episode {
                    start: b.start,
                    end: b.start + bucket_secs,
                    buckets: 1,
                    dropped: b.dropped,
                    forwarded: b.forwarded,
                    peak_dropped: b.dropped,
                    peak_start: b.start,
                    peak_vs_typical: None,
                });
            }
            (false, _) => {
                if let Some(e) = cur.take() {
                    episodes.push(e);
                }
            }
        }
    }
    if let Some(e) = cur.take() {
        episodes.push(e);
    }
    for e in &mut episodes {
        e.peak_vs_typical = (med > 0.0).then(|| round(e.peak_dropped as f64 / med, 1));
    }
    // Biggest episodes first; ties by time so the order is stable.
    episodes.sort_by(|a, b| b.dropped.cmp(&a.dropped).then(a.start.cmp(&b.start)));
    base(
        if episodes.is_empty() { "ok" } else { "spikes" },
        med,
        threshold,
        episodes,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn series(drops: &[u64]) -> Vec<TimelineBucket> {
        drops
            .iter()
            .enumerate()
            .map(|(i, d)| TimelineBucket {
                start: 1000 + i as i64 * 60,
                forwarded: 100,
                dropped: *d,
                other: 0,
            })
            .collect()
    }

    #[test]
    fn finds_a_spike_and_merges_adjacent_buckets() {
        let r = detect(
            &series(&[2, 3, 2, 3, 2, 80, 120, 3, 2, 3, 2, 3]),
            60,
            3.0,
            10,
        );
        assert_eq!(r.status, "spikes");
        assert_eq!(r.episodes_total, 1);
        let e = &r.episodes[0];
        assert_eq!((e.buckets, e.dropped, e.peak_dropped), (2, 200, 120));
        assert_eq!(e.start, 1000 + 5 * 60);
        assert_eq!(e.end, 1000 + 7 * 60);
        assert_eq!(e.peak_start, 1000 + 6 * 60);
        assert_eq!(e.peak_vs_typical, Some(40.0));
        assert_eq!(r.typical_dropped_per_bucket, 3.0);
    }

    #[test]
    fn two_separate_episodes_rank_by_size() {
        let r = detect(&series(&[1, 1, 50, 1, 1, 1, 1, 200, 1, 1]), 60, 3.0, 10);
        assert_eq!(r.episodes_total, 2);
        assert_eq!(r.episodes[0].dropped, 200);
        assert_eq!(r.episodes[1].dropped, 50);
    }

    #[test]
    fn min_drops_floor_keeps_a_quiet_cluster_quiet() {
        let r = detect(&series(&[0, 0, 0, 0, 0, 5, 0, 0, 0, 0]), 60, 3.0, 10);
        assert_eq!(r.status, "ok");
        assert!(r.episodes.is_empty());
        let r = detect(&series(&[0, 0, 0, 0, 0, 5, 0, 0, 0, 0]), 60, 3.0, 1);
        assert_eq!(r.episodes_total, 1);
        assert_eq!(r.episodes[0].peak_vs_typical, None);
    }

    #[test]
    fn a_steady_high_level_is_not_a_spike() {
        let r = detect(
            &series(&[100, 102, 98, 101, 99, 100, 103, 97, 100, 101]),
            60,
            3.0,
            10,
        );
        assert_eq!(r.status, "ok");
    }

    #[test]
    fn too_few_buckets_is_insufficient_data() {
        let r = detect(&series(&[1, 500, 1]), 60, 3.0, 10);
        assert_eq!(r.status, "insufficient_data");
        assert!(r.episodes.is_empty());
        assert_eq!(detect(&[], 60, 3.0, 10).buckets, 0);
    }

    #[test]
    fn parameters_are_clamped_and_nan_is_default() {
        let r = detect(&series(&[1; 10]), 60, f64::NAN, 0);
        assert_eq!((r.sensitivity, r.min_drops), (DEFAULT_SENSITIVITY, 1));
        let r = detect(&series(&[1; 10]), 60, 1e9, u64::MAX);
        assert_eq!(
            (r.sensitivity, r.min_drops),
            (MAX_SENSITIVITY, MAX_MIN_DROPS)
        );
    }

    #[test]
    fn episodes_are_capped() {
        let mut v = vec![0u64; 400];
        for i in (0..400).step_by(4) {
            v[i] = 50;
        }
        let r = detect(&series(&v), 60, 3.0, 10);
        assert!(r.episodes_total > MAX_EPISODES);
        assert_eq!(r.episodes.len(), MAX_EPISODES);
    }
}
