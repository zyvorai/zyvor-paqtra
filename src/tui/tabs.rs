// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabIndex {
    Flows = 0,
    Connections = 1,
    Endpoints = 2,
    Policies = 3,
    Metrics = 4,
    Healer = 5,
    AutoPolicy = 6,
    RootCause = 7,
    Simulator = 8,
    Replay = 9,
    Chaos = 10,
    Canary = 11,
    MultiCluster = 12,
}

impl TabIndex {
    /// Returns the display names for all tabs in order.
    pub fn tab_names() -> Vec<&'static str> {
        vec![
            "Flows",
            "Connections",
            "Endpoints",
            "Policies",
            "Metrics",
            "Healer",
            "AutoPolicy",
            "RootCause",
            "Simulator",
            "Replay",
            "Chaos",
            "Canary",
            "MultiCluster",
        ]
    }

    /// Returns the total number of tabs.
    pub fn count() -> usize {
        Self::tab_names().len()
    }
}

impl From<usize> for TabIndex {
    fn from(value: usize) -> Self {
        match value {
            0 => TabIndex::Flows,
            1 => TabIndex::Connections,
            2 => TabIndex::Endpoints,
            3 => TabIndex::Policies,
            4 => TabIndex::Metrics,
            5 => TabIndex::Healer,
            6 => TabIndex::AutoPolicy,
            7 => TabIndex::RootCause,
            8 => TabIndex::Simulator,
            9 => TabIndex::Replay,
            10 => TabIndex::Chaos,
            11 => TabIndex::Canary,
            12 => TabIndex::MultiCluster,
            _ => TabIndex::Flows,
        }
    }
}

impl From<TabIndex> for usize {
    fn from(tab: TabIndex) -> usize {
        tab as usize
    }
}

impl fmt::Display for TabIndex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let names = TabIndex::tab_names();
        let idx: usize = (*self).into();
        write!(f, "{}", names[idx])
    }
}
