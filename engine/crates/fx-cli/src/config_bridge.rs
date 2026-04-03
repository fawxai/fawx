//! Bridge between fx-config CLI types and fx-core domain types.

use fx_config::{PermissionsConfig, SelfModifyCliConfig};
use fx_core::self_modify::SelfModifyConfig;

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
/// The self-modify config is the canonical source of path-policy truth.
/// Presentation mode and granted capabilities may change how the user is asked,
/// but they must not silently rewrite the path tiers the resolver evaluates.
pub fn effective_self_modify_config(
    cli: &SelfModifyCliConfig,
    _permissions: &PermissionsConfig,
) -> SelfModifyConfig {
    to_core_self_modify(cli)
}

#[cfg(test)]
mod tests {
    use super::*;
    use fx_config::{CapabilityMode, PermissionsConfig, SelfModifyPathsCliConfig};
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
    fn effective_self_modify_config_preserves_disabled_cli_toggle() {
        let cli = SelfModifyCliConfig::default();

        let core = effective_self_modify_config(&cli, &PermissionsConfig::power());

        assert!(!core.enabled);
        assert!(core.allow_paths.is_empty());
        assert!(core.propose_paths.is_empty());
    }

    #[test]
    fn effective_self_modify_config_preserves_explicit_path_policy_in_capability_mode() {
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
        assert_eq!(core.allow_paths, vec!["skills/**"]);
        assert_eq!(core.propose_paths, vec!["engine/**"]);
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
    fn capability_mode_and_prompt_mode_share_the_same_path_policy_truth() {
        let cli = SelfModifyCliConfig::default();
        let capability = effective_self_modify_config(&cli, &PermissionsConfig::power());
        let mut prompt_permissions = PermissionsConfig::power();
        prompt_permissions.mode = CapabilityMode::Prompt;
        let prompt = effective_self_modify_config(&cli, &prompt_permissions);

        assert_eq!(capability, prompt);
    }
}
