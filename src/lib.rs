#![allow(clippy::upper_case_acronyms)]
// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Many types and methods are intentionally public for the TUI binary
// but appear unused in library-only builds.
#![allow(dead_code)]
// Library re-exports for integration testing
//
// This file exposes internal modules so that integration tests in the
// `tests/` directory can import crate types.

pub mod bootstrap;
pub mod cilium;
pub mod ebpf;
pub mod endpoints;
pub mod hubble;
pub mod integration;
pub mod kubernetes;
pub mod modules;
pub mod policies;
pub mod tui;
