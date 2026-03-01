//! Bridge between fx-config CLI types and fx-core domain types.

use fx_config::SelfModifyCliConfig;
use fx_core::self_modify::SelfModifyConfig;

pub fn to_core_self_modify(cli: &SelfModifyCliConfig) -> SelfModifyConfig {
    SelfModifyConfig {
        enabled: cli.enabled,
        branch_prefix: cli.branch_prefix.clone(),
        require_tests: cli.require_tests,
        allow_paths: cli.paths.allow.clone(),
        propose_paths: cli.paths.propose.clone(),
        deny_paths: cli.paths.deny.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fx_config::SelfModifyPathsCliConfig;

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
        };
        let core = to_core_self_modify(&cli);
        assert!(core.enabled);
        assert_eq!(core.branch_prefix, "test/prefix");
        assert!(!core.require_tests);
        assert_eq!(core.allow_paths, vec!["src/**"]);
        assert_eq!(core.propose_paths, vec!["kernel/**"]);
        assert_eq!(core.deny_paths, vec!["*.key"]);
    }
}
