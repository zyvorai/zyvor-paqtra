//! Per-second metrics platform shared by the Paqtra node agent and API:
//! collectors, the tiered time-series store, the agent-to-API stream format,
//! anomaly detection, metric alerts and exporters.
//!
//! Every collector is read-only. The process-group collector reads
//! `/proc/<pid>/stat`, `statm` and `io` only and never opens `cmdline` or
//! `environ`. Nothing here applies policy or changes the datapath.

pub mod anomaly;
pub mod collectors;
pub mod export;
pub mod metricalert;
pub mod stream;
pub mod tsdb;
pub mod wire;
