//! Per-metric anomaly detection (k-means per dimension), node anomaly rates,
//! and highlight-window correlation.

mod detector;
mod summary;

pub use detector::{Detector, Options, Stats};
pub use summary::{correlate, ks, summarize, NodeRate, Ranked, Summary, TimelinePoint};
