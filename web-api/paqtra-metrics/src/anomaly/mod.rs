// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

//! Per-metric anomaly detection (k-means per dimension), node anomaly rates,
//! and highlight-window correlation.

mod detector;
mod summary;

pub use detector::{Detector, Options, Stats};
pub use summary::{correlate, ks, summarize, NodeRate, Ranked, Summary, TimelinePoint};
