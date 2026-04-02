//! Shared library surface for the Fawx CLI.
//!
//! The binary target (`src/main.rs`) keeps the CLI entrypoint. This library
//! exposes the headless engine and startup helpers for other crates such as
//! `fawx-tui` embedded mode.

mod auth_store;
#[cfg(test)]
#[allow(dead_code)]
mod backup_command {
    mod implementation {
        include!("commands/backup.rs");
    }
    use implementation::*;
    mod tests {
        include!("commands/backup_tests.rs");
    }
}
#[cfg(test)]
#[allow(dead_code)]
mod import_command {
    mod implementation {
        include!("commands/import.rs");
    }
    use implementation::*;
    mod tests {
        include!("commands/import_tests.rs");
    }
}
#[cfg(test)]
#[allow(dead_code)]
mod fleet_command {
    mod implementation {
        include!("commands/fleet.rs");
    }
}
#[allow(dead_code)]
#[path = "commands/keys.rs"]
pub(crate) mod keys_commands;
#[path = "commands/marketplace.rs"]
pub(crate) mod marketplace_commands;
#[allow(dead_code)]
mod repo_root;
#[cfg(test)]
#[allow(dead_code)]
mod restart;
#[path = "commands/skill_sign.rs"]
pub(crate) mod skill_sign_commands;
#[path = "commands/slash.rs"]
pub(crate) mod slash_commands;
#[cfg(test)]
#[allow(dead_code)]
mod start_stop_command {
    include!("commands/start_stop.rs");
}
mod commands {
    pub(crate) use super::keys_commands as keys;
    pub(crate) use super::marketplace_commands as marketplace;
    pub(crate) use super::skill_sign_commands as skill_sign;
    pub(crate) use super::slash_commands as slash;
}
mod config_bridge;
mod context;
pub mod headless;
pub(crate) mod helpers;
#[cfg(feature = "http")]
pub mod http_serve;
#[cfg(test)]
mod markdown;
mod persisted_memory;
mod proposal_review;
#[allow(dead_code)]
// TODO(#1282): narrow this once embedded/lib and CLI startup paths stop leaving target-specific helpers unused.
pub(crate) mod startup;

use fx_consensus::ProgressCallback;
use std::path::PathBuf;

pub use persisted_memory::persisted_memory_entry_count;

/// Build a headless app suitable for embedded use.
pub fn build_headless_app(system_prompt: Option<PathBuf>) -> anyhow::Result<headless::HeadlessApp> {
    build_headless_app_with_progress(system_prompt, None)
}

/// Build a headless app suitable for embedded use with optional experiment progress reporting.
pub fn build_headless_app_with_progress(
    system_prompt: Option<PathBuf>,
    experiment_progress: Option<ProgressCallback>,
) -> anyhow::Result<headless::HeadlessApp> {
    headless::startup::build_embedded_headless_app(headless::startup::EmbeddedHeadlessAppRequest {
        system_prompt,
        experiment_progress,
    })
}

/// Normalize embedded-mode config before constructing the headless app.
///
/// Embedded callers run inside another host process, so they should inherit
/// the host process working directory unless config already overrides it.
pub fn prepare_embedded_config(config: fx_config::FawxConfig) -> fx_config::FawxConfig {
    headless::startup::prepare_embedded_config(config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use fx_config::test_support::CurrentDirGuard;
    use std::path::PathBuf;

    #[test]
    fn normalize_embedded_working_dir_defaults_to_process_current_dir() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let _guard = CurrentDirGuard::set(tempdir.path()).expect("set current dir");

        let config = prepare_embedded_config(fx_config::FawxConfig::default());

        // On macOS /var → /private/var symlink, so canonicalize both sides.
        let expected = tempdir.path().canonicalize().ok();
        let actual = config
            .tools
            .working_dir
            .as_ref()
            .and_then(|p| p.canonicalize().ok());
        assert_eq!(actual, expected);
    }

    #[test]
    fn normalize_embedded_working_dir_preserves_explicit_config_value() {
        let explicit = PathBuf::from("/tmp/fawx-explicit-working-dir");
        let mut config = fx_config::FawxConfig::default();
        config.tools.working_dir = Some(explicit.clone());

        let config = prepare_embedded_config(config);

        assert_eq!(config.tools.working_dir, Some(explicit));
    }
}
