//! Cilium-style management CLI: banner, install, status, info, agent, features.

mod agent;
mod banner;
mod chart;
mod cilium;
mod config;
mod connectivity;
mod doctor;
mod features;
mod global;
mod helm;
mod hubble;
mod info;
mod install;
mod kube;
pub mod metrics;
mod portforward;
mod preflight;
mod redact;
mod registry;
mod status;
mod sysdump;
mod version;

pub use agent::run_agent;
pub use banner::print_root_help;
pub use cilium::DEFAULT_VERSION as CILIUM_DEFAULT_VERSION;
pub use config::{
    cmd_get as cmd_config_get, cmd_set as cmd_config_set, cmd_view as cmd_config_view,
};
pub use connectivity::{cmd_connectivity_test, ConnOpts, DEFAULT_IMAGE as CONNECTIVITY_IMAGE};
pub use doctor::cmd_doctor;
pub use features::{cmd_ebpf_attachments, cmd_ebpf_drift, cmd_features};
pub use global::Global;
pub use hubble::{
    cmd_disable as cmd_hubble_disable, cmd_enable as cmd_hubble_enable,
    cmd_port_forward as cmd_hubble_port_forward, cmd_ui, DEFAULT_RELAY_PORT,
};
pub use info::cmd_info;
pub use install::{
    cmd_install, cmd_uninstall, cmd_upgrade, parse_duration, InstallOpts, UninstallOpts,
};
pub use status::{cmd_status, StatusOpts};
pub use sysdump::{cmd_sysdump, SysdumpOpts};
pub use version::cmd_version;
