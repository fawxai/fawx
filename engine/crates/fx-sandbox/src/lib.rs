//! OS-level enforcement for AX-first security.
//!
//! Provides Landlock filesystem sandboxing, seccomp syscall filtering,
//! and network policy enforcement. Gracefully degrades on platforms
//! or kernels that don't support these features.

pub mod config;

#[cfg(target_os = "linux")]
pub mod landlock;
#[cfg(target_os = "linux")]
pub mod seccomp;

pub mod network;

pub use config::{
    sandbox_config_from_preset, NetworkCapability, PathMode, SandboxConfig, SandboxPath,
    SandboxStatus,
};

#[derive(Debug)]
pub enum SandboxError {
    Landlock(String),
    Seccomp(String),
    Network(String),
}

impl std::fmt::Display for SandboxError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Landlock(message) => write!(f, "Landlock error: {message}"),
            Self::Seccomp(message) => write!(f, "seccomp error: {message}"),
            Self::Network(message) => write!(f, "network error: {message}"),
        }
    }
}

impl std::error::Error for SandboxError {}

/// Apply all available sandbox layers based on config.
/// Returns status indicating which layers were successfully applied.
pub fn apply_sandbox(config: &SandboxConfig) -> SandboxStatus {
    if !config.enabled {
        tracing::info!("Sandbox disabled; skipping OS enforcement");
        return SandboxStatus::default();
    }

    let mut status = SandboxStatus::default();

    #[cfg(target_os = "linux")]
    apply_linux_sandbox_layers(config, &mut status);

    #[cfg(not(target_os = "linux"))]
    tracing::info!("OS sandbox not available on this platform (Linux required)");

    apply_network_layer(config, &mut status);
    status
}

#[cfg(target_os = "linux")]
fn apply_linux_sandbox_layers(config: &SandboxConfig, status: &mut SandboxStatus) {
    apply_landlock_layer(config, status);
    apply_seccomp_layer(status);
}

#[cfg(target_os = "linux")]
fn apply_landlock_layer(config: &SandboxConfig, status: &mut SandboxStatus) {
    match landlock::apply_filesystem_sandbox(&config.filesystem_allow) {
        Ok(()) => {
            tracing::info!("Landlock filesystem sandbox applied");
            status.landlock = true;
        }
        Err(error) => tracing::warn!("Landlock not available: {error}"),
    }
}

#[cfg(target_os = "linux")]
fn apply_seccomp_layer(status: &mut SandboxStatus) {
    match seccomp::apply_syscall_sandbox() {
        Ok(()) => {
            tracing::info!("seccomp syscall sandbox applied");
            status.seccomp = true;
        }
        Err(error) => tracing::warn!("seccomp not available: {error}"),
    }
}

fn apply_network_layer(config: &SandboxConfig, status: &mut SandboxStatus) {
    match network::apply_network_policy(&config.network) {
        Ok(()) => {
            tracing::info!("Network policy applied: {:?}", config.network);
            status.network = true;
        }
        Err(error) => tracing::warn!("Network policy not available: {error}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_sandbox_disabled_returns_default_status() {
        let status = apply_sandbox(&SandboxConfig::default());

        assert_eq!(status, SandboxStatus::default());
    }

    #[test]
    fn sandbox_status_default_is_all_false() {
        let status = SandboxStatus::default();

        assert!(!status.landlock);
        assert!(!status.seccomp);
        assert!(!status.network);
    }
}
