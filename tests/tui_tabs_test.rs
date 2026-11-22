// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

/// Integration tests for TUI tab management.
///
/// These tests exercise the production `TabIndex` type from `src/tui/tabs.rs`
/// to verify tab index logic, display names, and boundary conditions.
use paqtra::tui::tabs::TabIndex;

/// Expected number of tabs in the TUI.
const EXPECTED_TAB_COUNT: usize = 13;

// ---------------------------------------------------------------------------
// Tests: tab names are all non-empty
// ---------------------------------------------------------------------------

#[test]
fn test_all_tab_names_are_nonempty() {
    for (i, name) in TabIndex::tab_names().iter().enumerate() {
        assert!(
            !name.is_empty(),
            "Tab name at index {} must not be empty",
            i
        );
    }
}

#[test]
fn test_all_tab_names_have_no_leading_trailing_whitespace() {
    for (i, name) in TabIndex::tab_names().iter().enumerate() {
        assert_eq!(
            *name,
            name.trim(),
            "Tab name at index {} ('{}') should not have leading/trailing whitespace",
            i,
            name
        );
    }
}

// ---------------------------------------------------------------------------
// Tests: tab count matches expected
// ---------------------------------------------------------------------------

#[test]
fn test_tab_count_matches_expected() {
    assert_eq!(
        TabIndex::count(),
        EXPECTED_TAB_COUNT,
        "There should be exactly {} tabs",
        EXPECTED_TAB_COUNT
    );
}

#[test]
fn test_tab_names_are_unique() {
    let names = TabIndex::tab_names();
    let mut seen = std::collections::HashSet::new();
    for name in &names {
        assert!(seen.insert(name), "Duplicate tab name found: '{}'", name);
    }
}

// ---------------------------------------------------------------------------
// Tests: TabIndex conversion from usize
// ---------------------------------------------------------------------------

#[test]
fn test_tab_index_from_usize_valid_range() {
    for i in 0..EXPECTED_TAB_COUNT {
        let tab = TabIndex::from(i);
        let idx: usize = tab.into();
        assert_eq!(
            idx, i,
            "TabIndex::from({}) should roundtrip back to {}",
            i, i
        );
    }
}

#[test]
fn test_tab_index_from_usize_out_of_range_defaults_to_flows() {
    // The production From<usize> maps out-of-range values to Flows
    let tab = TabIndex::from(EXPECTED_TAB_COUNT);
    assert_eq!(tab, TabIndex::Flows);
    let tab = TabIndex::from(100);
    assert_eq!(tab, TabIndex::Flows);
    let tab = TabIndex::from(usize::MAX);
    assert_eq!(tab, TabIndex::Flows);
}

#[test]
fn test_tab_index_roundtrip() {
    // Converting to usize and back should yield the same TabIndex
    let all_tabs = vec![
        TabIndex::Flows,
        TabIndex::Connections,
        TabIndex::Endpoints,
        TabIndex::Policies,
        TabIndex::Metrics,
        TabIndex::Healer,
        TabIndex::AutoPolicy,
        TabIndex::RootCause,
        TabIndex::Simulator,
        TabIndex::Replay,
        TabIndex::Chaos,
        TabIndex::Canary,
        TabIndex::MultiCluster,
    ];

    for tab in &all_tabs {
        let idx: usize = (*tab).into();
        let roundtripped = TabIndex::from(idx);
        assert_eq!(
            *tab, roundtripped,
            "TabIndex::{:?} should roundtrip through usize {}",
            tab, idx
        );
    }
}

#[test]
fn test_tab_index_covers_all_names() {
    // Every tab_names entry should have a corresponding TabIndex with matching Display
    let names = TabIndex::tab_names();
    for (i, name) in names.iter().enumerate() {
        let tab = TabIndex::from(i);
        assert_eq!(
            format!("{}", tab),
            *name,
            "TabIndex at {} should display as '{}', got '{}'",
            i,
            name,
            tab
        );
    }
}

// ---------------------------------------------------------------------------
// Tests: TabIndex Display
// ---------------------------------------------------------------------------

#[test]
fn test_tab_index_display() {
    assert_eq!(format!("{}", TabIndex::Flows), "Flows");
    assert_eq!(format!("{}", TabIndex::Connections), "Connections");
    assert_eq!(format!("{}", TabIndex::MultiCluster), "MultiCluster");
}

#[test]
fn test_tab_index_display_matches_name_array() {
    let all_tabs = vec![
        TabIndex::Flows,
        TabIndex::Connections,
        TabIndex::Endpoints,
        TabIndex::Policies,
        TabIndex::Metrics,
        TabIndex::Healer,
        TabIndex::AutoPolicy,
        TabIndex::RootCause,
        TabIndex::Simulator,
        TabIndex::Replay,
        TabIndex::Chaos,
        TabIndex::Canary,
        TabIndex::MultiCluster,
    ];

    let names = TabIndex::tab_names();
    for (tab, name) in all_tabs.iter().zip(names.iter()) {
        assert_eq!(
            format!("{}", tab),
            *name,
            "Display of {:?} should match tab_names entry",
            tab
        );
    }
}

// ---------------------------------------------------------------------------
// Tests: Tab navigation wrapping logic (as used in the TUI)
// ---------------------------------------------------------------------------

/// Simulate the TUI's right-arrow tab navigation: `(selected + 1) % count`
fn navigate_right(current: usize, count: usize) -> usize {
    (current + 1) % count
}

/// Simulate the TUI's left-arrow tab navigation
fn navigate_left(current: usize, count: usize) -> usize {
    if current == 0 {
        count - 1
    } else {
        current - 1
    }
}

#[test]
fn test_tab_navigation_right_wraps() {
    let count = TabIndex::count();
    let last = count - 1;
    assert_eq!(
        navigate_right(last, count),
        0,
        "Right from last tab should wrap to first"
    );
}

#[test]
fn test_tab_navigation_left_wraps() {
    let count = TabIndex::count();
    assert_eq!(
        navigate_left(0, count),
        count - 1,
        "Left from first tab should wrap to last"
    );
}

#[test]
fn test_tab_navigation_right_increments() {
    let count = TabIndex::count();
    for i in 0..(count - 1) {
        assert_eq!(
            navigate_right(i, count),
            i + 1,
            "Right from tab {} should go to {}",
            i,
            i + 1
        );
    }
}

#[test]
fn test_tab_navigation_left_decrements() {
    let count = TabIndex::count();
    for i in 1..count {
        assert_eq!(
            navigate_left(i, count),
            i - 1,
            "Left from tab {} should go to {}",
            i,
            i - 1
        );
    }
}

#[test]
fn test_full_right_cycle_returns_to_start() {
    let count = TabIndex::count();
    let mut pos = 0;
    for _ in 0..count {
        pos = navigate_right(pos, count);
    }
    assert_eq!(
        pos, 0,
        "Cycling right through all tabs should return to start"
    );
}

#[test]
fn test_full_left_cycle_returns_to_start() {
    let count = TabIndex::count();
    let mut pos = 0;
    for _ in 0..count {
        pos = navigate_left(pos, count);
    }
    assert_eq!(
        pos, 0,
        "Cycling left through all tabs should return to start"
    );
}

// ---------------------------------------------------------------------------
// Tests: specific tab positions match expected module indices
// ---------------------------------------------------------------------------

#[test]
fn test_known_tab_positions() {
    let names = TabIndex::tab_names();
    // These match the indices used in src/tui/mod.rs for key handling
    assert_eq!(names[0], "Flows"); // tab 0: flow list, packet explainer
    assert_eq!(names[5], "Healer"); // tab 5: self-healer
    assert_eq!(names[6], "AutoPolicy"); // tab 6: policy learning
    assert_eq!(names[7], "RootCause"); // tab 7: root-cause analysis
    assert_eq!(names[8], "Simulator"); // tab 8: what-if simulator
    assert_eq!(names[9], "Replay"); // tab 9: traffic replay
    assert_eq!(names[10], "Chaos"); // tab 10: chaos engineering
    assert_eq!(names[11], "Canary"); // tab 11: canary deployments
    assert_eq!(names[12], "MultiCluster"); // tab 12: multi-cluster
}
