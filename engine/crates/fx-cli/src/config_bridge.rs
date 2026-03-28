//! Bridge between fx-config CLI types and fx-core domain types.

use fx_config::{CapabilityMode, PermissionAction, PermissionsConfig, SelfModifyCliConfig};
use fx_core::self_modify::{SelfModifyConfig, KERNEL_SOURCE_PATH_PATTERNS};

pub fn to_core_self_modify(cli: &SelfModifyCliConfig) -> SelfModifyConfig {
    SelfModifyConfig {
        enabled: cli.enabled,
        branch_prefix: cli.branch_prefix.clone(),
        require_tests: cli.require_tests,
        allow_paths: cli.paths.allow.clone(),
        propose_paths: cli.paths.propose.clone(),
        deny_paths: cli.paths.deny.clone(),
        proposals_dir: cli
            .proposals_dir
            .clone()
            .unwrap_or_else(fx_core::self_modify::default_proposals_dir),
    }
}

/// Build the effective self-modify policy for the current session.
///
/// In capability mode, the action-level permissions are the source of truth for
/// whether self-modification is allowed. The standalone `[self_modify]` toggle
/// and path tiers should not silently narrow an explicitly granted
/// `self_modify` capability. Sovereign runtime protections remain enforced by
/// compiled proposal-gate invariants (Tier 3 / kernel-blind), not by this
/// config bridge.
pub fn effective_self_modify_config(
    cli: &SelfModifyCliConfig,
    permissions: &PermissionsConfig,
) -> SelfModifyConfig {
    let mut core = to_core_self_modify(cli);

    if permissions.mode != CapabilityMode::Capability {
        return core;
    }

    if permissions
        .unrestricted
        .contains(&PermissionAction::SelfModify)
    {
        core.enabled = true;
        core.allow_paths = vec!["**".to_string()];
        core.propose_paths.clear();
        return core;
    }

    if permissions
        .unrestricted
        .contains(&PermissionAction::KernelModify)
    {
        core.enabled = true;
        for pattern in KERNEL_SOURCE_PATH_PATTERNS {
            let pattern = (*pattern).to_string();
            if !core.allow_paths.contains(&pattern) {
                core.allow_paths.push(pattern);
            }
        }
        core.propose_paths
            .retain(|pattern| !KERNEL_SOURCE_PATH_PATTERNS.contains(&pattern.as_str()));
    }

    core
}

#[cfg(test)]
mod tests {
    use super::*;
    use fx_config::{PermissionsConfig, SelfModifyPathsCliConfig};
    use std::path::PathBuf;

    #[test]
    fn self_modify_cli_config_converts_to_core_config() {
        let cli = SelfModifyCliConfig {
            enabled: true,
            branch_prefix: "test/prefix".to_string(),
            require_tests: false,
            paths: SelfModifyPathsCliConfig {
                allow: vec!["src/**".to_string()],
                propose: vec!["kernel/**".to_string()],
                deny: vec!["*.key".to_string()],
            },
            proposals_dir: Some(PathBuf::from("/tmp/test-proposals")),
        };
        let core = to_core_self_modify(&cli);
        assert!(core.enabled);
        assert_eq!(core.branch_prefix, "test/prefix");
        assert!(!core.require_tests);
        assert_eq!(core.allow_paths, vec!["src/**"]);
        assert_eq!(core.propose_paths, vec!["kernel/**"]);
        assert_eq!(core.deny_paths, vec!["*.key"]);
        assert_eq!(core.proposals_dir, PathBuf::from("/tmp/test-proposals"));
    }

    #[test]
    fn self_modify_proposals_dir_defaults_when_none() {
        let cli = SelfModifyCliConfig {
            proposals_dir: None,
            ..SelfModifyCliConfig::default()
        };
        let core = to_core_self_modify(&cli);
        assert!(
            core.proposals_dir.ends_with(".fawx/proposals"),
            "expected default proposals dir, got: {}",
            core.proposals_dir.display()
        );
    }

    #[test]
    fn capability_mode_unrestricted_self_modify_overrides_disabled_cli_toggle() {
        let cli = SelfModifyCliConfig::default();

        let core = effective_self_modify_config(&cli, &PermissionsConfig::power());

        assert!(core.enabled);
        assert_eq!(core.allow_paths, vec!["**"]);
        assert!(core.propose_paths.is_empty());
    }

    #[test]
    fn capability_mode_unrestricted_self_modify_does_not_keep_path_narrowing() {
        let cli = SelfModifyCliConfig {
            enabled: true,
            paths: SelfModifyPathsCliConfig {
                allow: vec!["skills/**".to_string()],
                propose: vec!["engine/**".to_string()],
                deny: vec![".git/**".to_string()],
            },
            ..SelfModifyCliConfig::default()
        };

        let core = effective_self_modify_config(&cli, &PermissionsConfig::power());

        assert!(core.enabled);
        assert_eq!(core.allow_paths, vec!["**"]);
        assert!(core.propose_paths.is_empty());
        assert_eq!(core.deny_paths, vec![".git/**"]);
    }

    #[test]
    fn prompt_mode_preserves_explicit_self_modify_policy() {
        let cli = SelfModifyCliConfig {
            enabled: true,
            paths: SelfModifyPathsCliConfig {
                allow: vec!["skills/**".to_string()],
                propose: vec!["engine/**".to_string()],
                deny: vec![".git/**".to_string()],
            },
            ..SelfModifyCliConfig::default()
        };
        let mut permissions = PermissionsConfig::power();
        permissions.mode = CapabilityMode::Prompt;

        let core = effective_self_modify_config(&cli, &permissions);

        assert!(core.enabled);
        assert_eq!(core.allow_paths, vec!["skills/**"]);
        assert_eq!(core.propose_paths, vec!["engine/**"]);
        assert_eq!(core.deny_paths, vec![".git/**"]);
    }

    #[test]
    fn capability_mode_kernel_modify_does_not_widen_self_modify_surface() {
        let cli = SelfModifyCliConfig::default();
        let mut permissions = PermissionsConfig {
            preset: fx_config::PermissionPreset::Custom,
            unrestricted: Vec::new(),
            proposal_required: Vec::new(),
            ..PermissionsConfig::default()
        };
        permissions.mode = CapabilityMode::Capability;
        permissions.unrestricted = vec![PermissionAction::KernelModify];

        let core = effective_self_modify_config(&cli, &permissions);

        assert!(core.enabled);
        assert!(core
            .allow_paths
            .iter()
            .any(|p| p == "**/engine/crates/fx-kernel/**"));
        assert!(!core.allow_paths.iter().any(|p| p == "**"));
        assert!(core.propose_paths.is_empty());
    }
}
