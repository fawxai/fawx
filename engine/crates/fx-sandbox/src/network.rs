//! Network policy enforcement.
//!
//! Uses nftables on Linux, no-op on other platforms.
//! Phase 3 MVP: log-only for nftables (requires root/CAP_NET_ADMIN).

use crate::{config::NetworkCapability, SandboxError};

/// Apply network policy.
/// Phase 3 MVP: logs the intended policy. Actual nftables enforcement
/// requires root or CAP_NET_ADMIN and is deferred to FawxShell.
pub fn apply_network_policy(capability: &NetworkCapability) -> Result<(), SandboxError> {
    match capability {
        NetworkCapability::Full => {
            tracing::debug!("Network policy: full access (no restrictions)");
        }
        NetworkCapability::LocalhostOnly => {
            tracing::info!(
                "Network policy: localhost only (enforcement requires elevated privileges)"
            );
        }
        NetworkCapability::None => {
            tracing::info!("Network policy: no network (enforcement requires elevated privileges)");
        }
        NetworkCapability::AllowList(hosts) => log_allow_list_policy(hosts.len()),
    }

    Ok(())
}

fn log_allow_list_policy(host_count: usize) {
    tracing::info!(
        "Network policy: allow list ({} hosts, enforcement requires elevated privileges)",
        host_count
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_full_succeeds() {
        let result = apply_network_policy(&NetworkCapability::Full);

        assert!(result.is_ok());
    }

    #[test]
    fn apply_localhost_only_succeeds() {
        let result = apply_network_policy(&NetworkCapability::LocalhostOnly);

        assert!(result.is_ok());
    }

    #[test]
    fn apply_none_succeeds() {
        let result = apply_network_policy(&NetworkCapability::None);

        assert!(result.is_ok());
    }

    #[test]
    fn apply_allowlist_succeeds() {
        let result = apply_network_policy(&NetworkCapability::AllowList(vec![
            "example.com".to_string(),
            "localhost".to_string(),
        ]));

        assert!(result.is_ok());
    }
}
