// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

//! Multiwindow, multi-burn-rate evaluation of an availability SLO over stored
//! flow history.
//!
//! `slo::evaluate` projects the error budget from the latest flow sample. This
//! module measures how fast the budget is being spent over real time windows,
//! using the alert shape from the Google SRE Workbook (chapter 5, table 5-8):
//! an alert fires only when both a long and a short window burn faster than a
//! threshold, so it neither pages on a blip nor stays red after recovery.
//!
//! The good/bad signal is the same as the SLO's: forwarded versus dropped
//! flows. Hubble flows are captured by polling, so ratios describe the sampled
//! flows, not every request. A window the stored history does not fully cover
//! is not evaluated; it is reported as `insufficient_history` instead of being
//! judged on part of its data. Everything here notifies; nothing applies policy.

use super::flow_agg::round;
use super::flow_store::TimelineBucket;
use serde::Serialize;

/// Width of the buckets the windows are summed from, in seconds. Every window
/// below is a multiple of it, so window edges snap to 5 minutes.
pub const BUCKET_SECS: i64 = 300;
/// Longest window evaluated: how far back the caller must read.
pub const MAX_WINDOW_SECS: i64 = 3 * 86_400;
/// Fewer judged flows than this in a window and its ratio means nothing.
pub const MIN_JUDGED: u64 = 20;

struct Tier {
    severity: &'static str,
    long_secs: i64,
    short_secs: i64,
    burn: f64,
}

/// SRE Workbook table 5-8, recommended parameters for a 99.9% SLO.
const TIERS: [Tier; 3] = [
    Tier {
        severity: "page",
        long_secs: 3_600,
        short_secs: 300,
        burn: 14.4,
    },
    Tier {
        severity: "page",
        long_secs: 6 * 3_600,
        short_secs: 1_800,
        burn: 6.0,
    },
    Tier {
        severity: "ticket",
        long_secs: 3 * 86_400,
        short_secs: 6 * 3_600,
        burn: 1.0,
    },
];

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct WindowStats {
    pub seconds: i64,
    pub forwarded: u64,
    pub dropped: u64,
    /// Dropped / (forwarded + dropped); null when nothing was judged.
    pub error_ratio: Option<f64>,
    /// Error ratio divided by the error budget ratio (1 - target).
    pub burn_rate: Option<f64>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct TierResult {
    pub severity: &'static str,
    pub long: WindowStats,
    pub short: WindowStats,
    pub burn_threshold: f64,
    /// `evaluated`, `insufficient_data` (too few flows) or `insufficient_history`
    /// (the store does not reach back to the start of the long window).
    pub status: &'static str,
    pub firing: bool,
    /// Share of the whole SLO window's budget the long window has burned at
    /// the observed rate; null when not evaluated.
    pub budget_consumed_pct: Option<f64>,
    /// How long a full, untouched error budget would last at the long window's
    /// burn rate, in hours; null when not evaluated or when nothing burns.
    pub budget_lasts_hours: Option<f64>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct BurnReport {
    /// `page`, `ticket`, `ok` or `insufficient_data`.
    pub status: &'static str,
    pub target: f64,
    pub tiers: Vec<TierResult>,
    pub measurement: &'static str,
}

fn window(buckets: &[TimelineBucket], now: i64, secs: i64, budget_ratio: f64) -> WindowStats {
    let cutoff = now.saturating_sub(secs);
    let (mut fwd, mut drop) = (0u64, 0u64);
    for b in buckets {
        if b.start.saturating_add(BUCKET_SECS) > cutoff && b.start <= now {
            fwd = fwd.saturating_add(b.forwarded);
            drop = drop.saturating_add(b.dropped);
        }
    }
    let judged = fwd.saturating_add(drop);
    let error_ratio = (judged > 0).then(|| drop as f64 / judged as f64);
    let burn_rate = error_ratio
        .map(|r| r / budget_ratio)
        .filter(|b| b.is_finite());
    WindowStats {
        seconds: secs,
        forwarded: fwd,
        dropped: drop,
        error_ratio: error_ratio.map(|r| round(r, 6)),
        burn_rate: burn_rate.map(|b| round(b, 3)),
    }
}

/// Evaluate every tier.
///
/// * `target`: availability in percent, exclusive of 0 and 100.
/// * `slo_window_secs`: the SLO's own window, for `budget_consumed_pct`.
/// * `buckets`: flow counts in [`BUCKET_SECS`] buckets, any order, sparse.
/// * `now`, `oldest`: unix seconds of the evaluation and of the oldest stored
///   flow in scope (`None` when the store is empty).
pub fn evaluate(
    target: f64,
    slo_window_secs: i64,
    buckets: &[TimelineBucket],
    now: i64,
    oldest: Option<i64>,
) -> BurnReport {
    let budget_ratio = 1.0 - target / 100.0;
    // An invalid target makes every ratio meaningless; callers validate, this
    // keeps the function total.
    let valid = budget_ratio.is_finite() && budget_ratio > 0.0 && budget_ratio < 1.0;
    let slo_window_secs = slo_window_secs.max(1);

    let mut tiers = Vec::with_capacity(TIERS.len());
    for t in &TIERS {
        let long = window(
            buckets,
            now,
            t.long_secs,
            budget_ratio.max(f64::MIN_POSITIVE),
        );
        let short = window(
            buckets,
            now,
            t.short_secs,
            budget_ratio.max(f64::MIN_POSITIVE),
        );
        let history_ok = oldest.is_some_and(|o| o <= now.saturating_sub(t.long_secs));
        let enough = long.forwarded.saturating_add(long.dropped) >= MIN_JUDGED
            && short.forwarded.saturating_add(short.dropped) >= MIN_JUDGED;
        let status = if !valid {
            "insufficient_data"
        } else if !history_ok {
            "insufficient_history"
        } else if !enough {
            "insufficient_data"
        } else {
            "evaluated"
        };
        let firing = status == "evaluated"
            && long.burn_rate.is_some_and(|b| b >= t.burn)
            && short.burn_rate.is_some_and(|b| b >= t.burn);
        let budget_consumed_pct = if status == "evaluated" {
            long.burn_rate
                .map(|b| round(b * t.long_secs as f64 / slo_window_secs as f64 * 100.0, 3))
        } else {
            None
        };
        let budget_lasts_hours = if status == "evaluated" {
            long.burn_rate
                .filter(|b| *b > 0.0)
                .map(|b| round(slo_window_secs as f64 / 3_600.0 / b, 1))
        } else {
            None
        };
        tiers.push(TierResult {
            severity: t.severity,
            long,
            short,
            burn_threshold: t.burn,
            status,
            firing,
            budget_consumed_pct,
            budget_lasts_hours,
        });
    }

    let status = if tiers.iter().any(|t| t.firing && t.severity == "page") {
        "page"
    } else if tiers.iter().any(|t| t.firing) {
        "ticket"
    } else if tiers.iter().any(|t| t.status == "evaluated") {
        "ok"
    } else {
        "insufficient_data"
    };
    BurnReport {
        status,
        target,
        tiers,
        measurement: "Burn rate over stored flow history: dropped / (forwarded + dropped) in each \
                      window divided by the error budget (1 - target). Flows are captured by \
                      polling Hubble, so this describes the sampled flows, not every request. \
                      Window edges snap to 5-minute buckets. A tier fires only when its long and \
                      short windows both exceed the threshold (SRE Workbook, table 5-8).",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_800_000_000 - (1_800_000_000 % BUCKET_SECS);

    /// Buckets covering `span` seconds back from now, `per_bucket` judged flows
    /// each, of which `bad` per bucket are drops.
    fn steady(span: i64, per_bucket: u64, bad: u64, until_ago: i64) -> Vec<TimelineBucket> {
        let mut v = Vec::new();
        let mut s = NOW - span;
        while s < NOW - until_ago {
            v.push(TimelineBucket {
                start: s,
                forwarded: per_bucket - bad,
                dropped: bad,
                other: 0,
            });
            s += BUCKET_SECS;
        }
        v
    }

    fn eval(target: f64, b: &[TimelineBucket]) -> BurnReport {
        evaluate(target, 30 * 86_400, b, NOW + 1, Some(NOW - 4 * 86_400))
    }

    #[test]
    fn healthy_traffic_is_ok() {
        let r = eval(99.9, &steady(MAX_WINDOW_SECS, 100, 0, 0));
        assert_eq!(r.status, "ok");
        assert!(r.tiers.iter().all(|t| t.status == "evaluated" && !t.firing));
        assert_eq!(r.tiers[0].long.error_ratio, Some(0.0));
        // Nothing burns, so there is no exhaustion time.
        assert_eq!(r.tiers[0].budget_lasts_hours, None);
    }

    #[test]
    fn fast_burn_pages_and_budget_share_matches_the_workbook() {
        // 2% errors against a 0.1% budget is burn rate 20 > 14.4, for the last hour+.
        let mut b = steady(MAX_WINDOW_SECS, 100, 0, 7_200);
        b.extend(steady(7_200, 100, 2, 0));
        let r = eval(99.9, &b);
        assert_eq!(r.status, "page");
        let t = &r.tiers[0];
        assert!(t.firing);
        assert_eq!(t.long.burn_rate, Some(20.0));
        // 20x for one hour of a 30-day window = 20 / 720 = 2.78%.
        assert_eq!(t.budget_consumed_pct, Some(2.778));
        // A full 30-day budget (720 h) at 20x lasts 36 hours.
        assert_eq!(t.budget_lasts_hours, Some(36.0));
    }

    #[test]
    fn page_threshold_is_exclusive_of_just_under() {
        // 1000 flows per bucket, 14 or 15 dropped: 14.0x and 15.0x of a 0.1% budget.
        for (bad, fires) in [(14u64, false), (15, true)] {
            let mut b = steady(MAX_WINDOW_SECS, 1000, 0, 3_600);
            b.extend(steady(3_600, 1000, bad, 0));
            let t = &eval(99.9, &b).tiers[0].clone();
            assert_eq!(t.long.burn_rate, Some(bad as f64), "bad={bad}");
            assert_eq!(t.firing, fires, "bad={bad}");
        }
    }

    #[test]
    fn recovered_blip_does_not_page_because_the_short_window_is_clean() {
        // Bad from 50 to 40 minutes ago, clean since: the 1h long window still
        // burns hot, the 5m and 30m short windows do not.
        let mut b = steady(MAX_WINDOW_SECS, 100, 0, 3_000);
        b.extend(steady(3_000, 100, 50, 2_400));
        b.extend(steady(2_400, 100, 0, 0));
        let r = eval(99.9, &b);
        let t = &r.tiers[0];
        assert!(t.long.burn_rate.unwrap() >= 14.4);
        assert!(t.short.burn_rate.unwrap() < 14.4);
        assert!(!t.firing);
        assert!(!r.tiers[1].firing, "6h tier: 30m short window is clean too");
        assert_ne!(r.status, "page");
    }

    #[test]
    fn slow_burn_opens_a_ticket_not_a_page() {
        // 0.2% errors all along: burn 2 > 1 over 3d and 6h, < 6 and < 14.4.
        let r = eval(99.9, &steady(MAX_WINDOW_SECS, 1000, 2, 0));
        assert_eq!(r.status, "ticket");
        assert!(r.tiers[2].firing && !r.tiers[0].firing && !r.tiers[1].firing);
    }

    #[test]
    fn short_history_is_not_judged_on_part_of_the_data() {
        // Store only reaches back 2 hours: the 1h tier can run, 6h and 3d cannot.
        let b = steady(7_200, 100, 5, 0);
        let r = evaluate(99.9, 30 * 86_400, &b, NOW + 1, Some(NOW - 7_200));
        assert_eq!(r.tiers[0].status, "evaluated");
        assert_eq!(r.tiers[1].status, "insufficient_history");
        assert_eq!(r.tiers[2].status, "insufficient_history");
        assert!(!r.tiers[2].firing);
        assert_eq!(r.tiers[1].budget_consumed_pct, None);
    }

    #[test]
    fn empty_store_and_quiet_cluster_are_insufficient_data_not_ok() {
        let r = evaluate(99.9, 86_400, &[], NOW, None);
        assert_eq!(r.status, "insufficient_data");
        assert!(r.tiers.iter().all(|t| t.status == "insufficient_history"));
        // History reaches back, but only 3 flows ever: a ratio on 3 flows is noise.
        let b = vec![TimelineBucket {
            start: NOW - 300,
            forwarded: 1,
            dropped: 2,
            other: 0,
        }];
        let r = evaluate(99.9, 86_400, &b, NOW, Some(NOW - 4 * 86_400));
        assert_eq!(r.status, "insufficient_data");
        assert!(r
            .tiers
            .iter()
            .all(|t| t.status == "insufficient_data" && !t.firing));
    }

    #[test]
    fn other_verdicts_are_not_judged() {
        let b: Vec<_> = (0..12)
            .map(|i| TimelineBucket {
                start: NOW - 300 * (i + 1),
                forwarded: 50,
                dropped: 0,
                other: 10_000,
            })
            .collect();
        let r = eval(99.9, &b);
        assert_eq!(r.tiers[0].long.error_ratio, Some(0.0));
    }

    #[test]
    fn hostile_inputs_never_panic() {
        // Invalid targets and extreme numbers.
        for target in [0.0, 100.0, -5.0, 250.0, f64::NAN, f64::INFINITY] {
            let r = eval(target, &steady(3_600, 100, 100, 0));
            assert_eq!(r.status, "insufficient_data", "{target}");
        }
        let huge = vec![
            TimelineBucket {
                start: NOW - 300,
                forwarded: u64::MAX,
                dropped: u64::MAX,
                other: u64::MAX,
            },
            TimelineBucket {
                start: NOW - 600,
                forwarded: u64::MAX,
                dropped: 1,
                other: 0,
            },
            TimelineBucket {
                start: i64::MIN,
                forwarded: 1,
                dropped: 1,
                other: 0,
            },
            TimelineBucket {
                start: i64::MAX,
                forwarded: 1,
                dropped: 1,
                other: 0,
            },
        ];
        let r = evaluate(99.9, 0, &huge, i64::MAX, Some(i64::MIN));
        assert_eq!(r.tiers.len(), 3);
        let r = evaluate(99.9, i64::MIN, &huge, i64::MIN, Some(i64::MAX));
        assert_eq!(r.tiers.len(), 3);
        // A bucket in the future (clock skew) is ignored, not summed.
        let future = vec![TimelineBucket {
            start: NOW + 10_000,
            forwarded: 0,
            dropped: 1000,
            other: 0,
        }];
        let r = eval(99.9, &future);
        assert_eq!(r.tiers[0].long.dropped, 0);
        // A tight target keeps the burn rate finite.
        let r = eval(99.999999999, &steady(3_600, 100, 1, 0));
        assert!(r.tiers[0].long.burn_rate.is_none_or(f64::is_finite));
    }
}
