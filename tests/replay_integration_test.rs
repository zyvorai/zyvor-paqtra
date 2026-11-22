// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

/// Integration tests for the Traffic Replay module.
///
/// Tests the replay engine's recording lifecycle, filter system,
/// and configuration using MockMapReader for eBPF data and
/// K8sClient::mock() for Kubernetes interactions.
use paqtra::ebpf::{MockMapReader, PolicyVerdict};
use paqtra::kubernetes::K8sClient;
use paqtra::modules::replay::{Recording, ReplayConfig, ReplayEngine, ReplayFilter, ReplayStats};
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// Helper
// ---------------------------------------------------------------------------

fn mock_k8s_client() -> K8sClient {
    K8sClient::mock()
}

// ---------------------------------------------------------------------------
// Tests: ReplayEngine creation and initial state
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_replay_engine_initial_state() {
    let config = ReplayConfig::default();
    let reader = MockMapReader;
    let k8s = mock_k8s_client();

    let engine = ReplayEngine::new(config, reader, k8s);
    let stats = engine.stats();
    assert_eq!(stats.total_recordings, 0);
    assert!(!stats.recording_in_progress);
    assert_eq!(stats.total_flows_recorded, 0);
}

#[tokio::test]
async fn test_replay_engine_no_recording_in_progress_initially() {
    let config = ReplayConfig::default();
    let reader = MockMapReader;
    let k8s = mock_k8s_client();

    let engine = ReplayEngine::new(config, reader, k8s);
    assert!(
        !engine.stats().recording_in_progress,
        "No recording should be in progress initially"
    );
}

// ---------------------------------------------------------------------------
// Tests: Recording lifecycle
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_replay_start_recording_returns_id() {
    let config = ReplayConfig::default();
    let reader = MockMapReader;
    let k8s = mock_k8s_client();

    let mut engine = ReplayEngine::new(config, reader, k8s);
    let id = engine
        .start_recording("test-capture".to_string())
        .await
        .unwrap();

    assert!(!id.is_empty(), "Recording ID should not be empty");
    assert!(
        id.starts_with("rec-"),
        "Recording ID should start with 'rec-'"
    );
}

#[tokio::test]
async fn test_replay_start_recording_sets_in_progress() {
    let config = ReplayConfig::default();
    let reader = MockMapReader;
    let k8s = mock_k8s_client();

    let mut engine = ReplayEngine::new(config, reader, k8s);
    engine
        .start_recording("test-capture".to_string())
        .await
        .unwrap();

    assert!(
        engine.stats().recording_in_progress,
        "Recording should be in progress after start_recording"
    );
}

#[tokio::test]
async fn test_replay_double_start_recording_fails() {
    let config = ReplayConfig::default();
    let reader = MockMapReader;
    let k8s = mock_k8s_client();

    let mut engine = ReplayEngine::new(config, reader, k8s);
    engine.start_recording("first".to_string()).await.unwrap();

    let result = engine.start_recording("second".to_string()).await;
    assert!(
        result.is_err(),
        "Starting a second recording while one is in progress should fail"
    );
}

#[tokio::test]
async fn test_replay_capture_with_mock_data() {
    let config = ReplayConfig::default();
    let reader = MockMapReader;
    let k8s = mock_k8s_client();

    let mut engine = ReplayEngine::new(config, reader, k8s);
    engine
        .start_recording("test-capture".to_string())
        .await
        .unwrap();

    // MockMapReader returns realistic conntrack data
    let _captured = engine.capture().await.unwrap();
}

#[tokio::test]
async fn test_replay_capture_without_recording_fails() {
    let config = ReplayConfig::default();
    let reader = MockMapReader;
    let k8s = mock_k8s_client();

    let mut engine = ReplayEngine::new(config, reader, k8s);
    let result = engine.capture().await;
    assert!(
        result.is_err(),
        "Capture should fail when no recording is in progress"
    );
}

#[tokio::test]
async fn test_replay_stop_recording_returns_metadata() {
    let config = ReplayConfig::default();
    let reader = MockMapReader;
    let k8s = mock_k8s_client();

    let mut engine = ReplayEngine::new(config, reader, k8s);
    engine
        .start_recording("test-capture".to_string())
        .await
        .unwrap();
    engine.capture().await.unwrap();

    let recording = engine.stop_recording().await.unwrap();
    assert_eq!(recording.name, "test-capture");
    assert!(recording.start_time > 0);
    assert!(recording.end_time >= recording.start_time);
    // flow_count is usize (always non-negative); the recording was verified above
}

#[tokio::test]
async fn test_replay_stop_recording_clears_in_progress() {
    let config = ReplayConfig::default();
    let reader = MockMapReader;
    let k8s = mock_k8s_client();

    let mut engine = ReplayEngine::new(config, reader, k8s);
    engine
        .start_recording("test-capture".to_string())
        .await
        .unwrap();
    engine.stop_recording().await.unwrap();

    assert!(
        !engine.stats().recording_in_progress,
        "Recording should no longer be in progress after stop"
    );
}

#[tokio::test]
async fn test_replay_stop_recording_increments_total() {
    let config = ReplayConfig::default();
    let reader = MockMapReader;
    let k8s = mock_k8s_client();

    let mut engine = ReplayEngine::new(config, reader, k8s);
    engine
        .start_recording("test-capture".to_string())
        .await
        .unwrap();
    engine.stop_recording().await.unwrap();

    assert_eq!(
        engine.stats().total_recordings,
        1,
        "Total recordings should be 1 after completing one recording"
    );
}

#[tokio::test]
async fn test_replay_stop_without_start_fails() {
    let config = ReplayConfig::default();
    let reader = MockMapReader;
    let k8s = mock_k8s_client();

    let mut engine = ReplayEngine::new(config, reader, k8s);
    let result = engine.stop_recording().await;
    assert!(
        result.is_err(),
        "Stopping without a recording in progress should fail"
    );
}

// ---------------------------------------------------------------------------
// Tests: ReplayConfig defaults
// ---------------------------------------------------------------------------

#[test]
fn test_replay_config_defaults() {
    let config = ReplayConfig::default();
    assert!(config.enabled);
    assert!(config.compress, "Compression should be on by default");
    assert!(
        (config.replay_rate - 1.0).abs() < f32::EPSILON,
        "Default replay rate should be 1.0 (real-time)"
    );
    assert!(
        config.max_recording_size > 0,
        "Max recording size must be positive"
    );
    assert!(
        config.max_recording_duration > 0,
        "Max recording duration must be positive"
    );
    assert!(config.detailed_comparison);
}

#[test]
fn test_replay_config_recording_dir_default() {
    let config = ReplayConfig::default();
    assert_eq!(
        config.recording_dir,
        PathBuf::from("/tmp/paqtra/recordings")
    );
}

// ---------------------------------------------------------------------------
// Tests: ReplayFilter
// ---------------------------------------------------------------------------

#[test]
fn test_replay_filter_default_is_unfiltered() {
    let filter = ReplayFilter::default();
    assert!(filter.namespaces.is_none());
    assert!(filter.src_labels.is_none());
    assert!(filter.dst_labels.is_none());
    assert!(filter.ports.is_none());
    assert!(filter.protocols.is_none());
    assert!(filter.verdicts.is_none());
    assert!(filter.limit.is_none());
}

#[test]
fn test_replay_filter_with_namespace() {
    let filter = ReplayFilter {
        namespaces: Some(vec!["default".to_string(), "prod".to_string()]),
        ..ReplayFilter::default()
    };
    let ns = filter.namespaces.as_ref().unwrap();
    assert_eq!(ns.len(), 2);
    assert!(ns.contains(&"default".to_string()));
    assert!(ns.contains(&"prod".to_string()));
}

#[test]
fn test_replay_filter_with_port() {
    let filter = ReplayFilter {
        ports: Some(vec![80, 443, 8080]),
        ..ReplayFilter::default()
    };
    let ports = filter.ports.as_ref().unwrap();
    assert_eq!(ports.len(), 3);
    assert!(ports.contains(&443));
}

#[test]
fn test_replay_filter_with_limit() {
    let filter = ReplayFilter {
        limit: Some(100),
        ..ReplayFilter::default()
    };
    assert_eq!(filter.limit, Some(100));
}

#[test]
fn test_replay_filter_with_verdict() {
    let filter = ReplayFilter {
        verdicts: Some(vec![PolicyVerdict::Allow, PolicyVerdict::Deny]),
        ..ReplayFilter::default()
    };
    let verdicts = filter.verdicts.as_ref().unwrap();
    assert_eq!(verdicts.len(), 2);
    assert!(verdicts.contains(&PolicyVerdict::Allow));
    assert!(verdicts.contains(&PolicyVerdict::Deny));
}

// ---------------------------------------------------------------------------
// Tests: ReplayStats
// ---------------------------------------------------------------------------

#[test]
fn test_replay_stats_fields() {
    let stats = ReplayStats {
        total_recordings: 5,
        recording_in_progress: true,
        total_flows_recorded: 25000,
    };
    assert_eq!(stats.total_recordings, 5);
    assert!(stats.recording_in_progress);
    assert_eq!(stats.total_flows_recorded, 25000);
}

// ---------------------------------------------------------------------------
// Tests: Recording metadata
// ---------------------------------------------------------------------------

#[test]
fn test_recording_metadata_construction() {
    let recording = Recording {
        id: "rec-test-001".to_string(),
        name: "Test capture".to_string(),
        source_cluster: "test-cluster".to_string(),
        start_time: 1000,
        end_time: 2000,
        flow_count: 500,
        total_bytes: 1_048_576,
        namespaces: vec!["default".to_string(), "kube-system".to_string()],
        services: vec!["web".to_string()],
        file_path: PathBuf::from("/tmp/test.json"),
        compressed: true,
    };
    assert_eq!(recording.id, "rec-test-001");
    assert_eq!(recording.name, "Test capture");
    assert_eq!(recording.end_time - recording.start_time, 1000);
    assert_eq!(recording.flow_count, 500);
    assert_eq!(recording.namespaces.len(), 2);
    assert!(recording.compressed);
}
