//! CLI command implementations
//!
//! Most modules here are consumed by the binary target (`main.rs`).
//! The lib target only exposes headless/startup paths, so many command
//! functions appear unused from the lib's perspective — especially on
//! macOS where cfg-gated code changes dead-code analysis.

pub(crate) mod api_client;
#[allow(dead_code)]
pub mod audit;
#[allow(dead_code)]
pub mod auth;
#[allow(dead_code)]
pub mod backup;
pub mod bootstrap;
#[allow(dead_code)]
pub mod completions;
#[allow(dead_code)]
pub mod config;
#[allow(dead_code)]
pub mod devices;
#[allow(dead_code)]
pub(crate) mod diagnostics;
#[allow(dead_code)]
pub mod doctor;
#[allow(dead_code)]
pub mod eval_harness;
#[allow(dead_code)]
pub mod experiment;
#[allow(dead_code)]
pub mod fleet;
#[allow(dead_code)]
pub mod import;
#[allow(dead_code)]
pub(crate) mod log_files;
#[allow(dead_code)]
pub mod logs;
#[allow(dead_code)]
pub mod marketplace;
#[cfg(feature = "oauth-bridge")]
#[allow(dead_code)]
pub mod oauth_bridge;
#[allow(dead_code)]
pub mod pair;
#[allow(dead_code)]
pub mod reset;
#[allow(dead_code)]
pub(crate) mod runtime_layout;
#[allow(dead_code)]
pub mod security_audit;
#[allow(dead_code)]
pub mod serve_fleet;
#[allow(dead_code)]
pub mod sessions;
#[allow(dead_code)]
pub mod setup;
#[allow(dead_code)]
pub(crate) mod skill_signatures;
#[allow(dead_code)]
pub mod skills;
pub mod slash;
#[allow(dead_code)]
pub(crate) mod start_stop;
#[allow(dead_code)]
pub mod status;
#[allow(dead_code)]
pub mod tailscale;
#[allow(dead_code)]
pub mod update;
#[allow(dead_code)]
pub mod version;
