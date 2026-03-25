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
#[path = "commands/marketplace.rs"]
pub(crate) mod marketplace_commands;
#[cfg(test)]
#[allow(dead_code)]
mod repo_root;
#[cfg(test)]
#[allow(dead_code)]
mod restart;
#[path = "commands/slash.rs"]
pub(crate) mod slash_commands;
#[cfg(test)]
#[allow(dead_code)]
mod start_stop_command {
    include!("commands/start_stop.rs");
}
mod commands {
    pub(crate) use super::marketplace_commands as marketplace;
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

use fx_canary::{CanaryConfig, CanaryMonitor, RipcordTrigger, RollbackTrigger};
use fx_consensus::ProgressCallback;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

pub use persisted_memory::persisted_memory_entry_count;

struct HeadlessAppBuildConfig {
    router: Arc<std::sync::RwLock<fx_llm::ModelRouter>>,
    config: fx_config::FawxConfig,
    improvement_provider: Option<Arc<dyn fx_llm::CompletionProvider + Send + Sync>>,
    system_prompt: Option<PathBuf>,
    config_manager: Option<Arc<std::sync::Mutex<fx_config::manager::ConfigManager>>>,
    data_dir: PathBuf,
    experiment_progress: Option<ProgressCallback>,
}

/// Build a headless app suitable for embedded use.
pub fn build_headless_app(system_prompt: Option<PathBuf>) -> anyhow::Result<headless::HeadlessApp> {
    build_headless_app_with_progress(system_prompt, None)
}

/// Build a headless app suitable for embedded use with optional experiment progress reporting.
pub fn build_headless_app_with_progress(
    system_prompt: Option<PathBuf>,
    experiment_progress: Option<ProgressCallback>,
) -> anyhow::Result<headless::HeadlessApp> {
    let auth_manager = startup::load_auth_manager()?;
    let config = prepare_embedded_config(startup::load_config()?);
    let mut router = startup::build_router(&auth_manager)?;
    headless::seed_headless_router_active_model(&mut router, &config);
    let router = Arc::new(std::sync::RwLock::new(router));
    let build_config = HeadlessAppBuildConfig {
        data_dir: configured_data_dir(&config),
        config_manager: Some(build_config_manager(&config)),
        improvement_provider: startup::build_improvement_provider(&auth_manager, &config),
        system_prompt,
        router,
        config,
        experiment_progress,
    };

    build_initialized_headless_app(build_config)
}

fn build_initialized_headless_app(
    build_config: HeadlessAppBuildConfig,
) -> anyhow::Result<headless::HeadlessApp> {
    let mut app = build_app_with_dependencies(build_config)?;
    app.initialize();
    Ok(app)
}

fn build_app_with_dependencies(
    build_config: HeadlessAppBuildConfig,
) -> anyhow::Result<headless::HeadlessApp> {
    let session_bus = startup::build_session_bus_for_data_dir(&build_config.data_dir);
    let credential_store = startup::open_credential_store(&build_config.data_dir).ok();
    let subagent_manager = build_subagent_manager(
        Arc::clone(&build_config.router),
        &build_config.config,
        build_config.improvement_provider.clone(),
        session_bus.clone(),
        credential_store.clone(),
    );
    let bundle = startup::build_headless_loop_engine_bundle(
        &build_config.config,
        build_config.improvement_provider,
        startup::HeadlessLoopBuildOptions {
            credential_store: credential_store.clone(),
            ..parent_loop_build_options(
                &subagent_manager,
                build_config.config_manager.clone(),
                session_bus.clone(),
                build_config.experiment_progress,
            )
        },
    )?;

    headless::HeadlessApp::new(headless::HeadlessAppDeps {
        loop_engine: bundle.engine,
        router: build_config.router,
        runtime_info: bundle.runtime_info,
        config: build_config.config,
        memory: bundle.memory,
        embedding_index_persistence: bundle.embedding_index_persistence,
        system_prompt_path: build_config.system_prompt,
        config_manager: build_config.config_manager,
        system_prompt_text: None,
        subagent_manager,
        canary_monitor: Some(build_canary_monitor(&build_config.data_dir)),
        session_bus,
        session_key: Some(headless::main_session_key()),
        cron_store: bundle.cron_store,
        startup_warnings: bundle.startup_warnings,
        stream_callback_slot: bundle.stream_callback_slot,
        ripcord_journal: bundle.ripcord_journal,
        #[cfg(feature = "http")]
        experiment_registry: None,
    })
}

fn build_config_manager(
    config: &fx_config::FawxConfig,
) -> Arc<std::sync::Mutex<fx_config::manager::ConfigManager>> {
    let data_dir = config
        .general
        .data_dir
        .clone()
        .unwrap_or_else(startup::fawx_data_dir);
    let config_path = data_dir.join("config.toml");
    let manager = fx_config::manager::ConfigManager::from_config(config.clone(), config_path);
    Arc::new(std::sync::Mutex::new(manager))
}

fn build_subagent_manager(
    router: Arc<std::sync::RwLock<fx_llm::ModelRouter>>,
    config: &fx_config::FawxConfig,
    improvement_provider: Option<Arc<dyn fx_llm::CompletionProvider + Send + Sync>>,
    session_bus: Option<fx_bus::SessionBus>,
    credential_store: Option<startup::SharedCredentialStore>,
) -> Arc<fx_subagent::SubagentManager> {
    let token_broker = startup::build_token_broker(config, credential_store.as_ref());
    let factory = headless::HeadlessSubagentFactory::new(headless::HeadlessSubagentFactoryDeps {
        router,
        config: config.clone(),
        improvement_provider,
        session_bus,
        token_broker,
    });

    Arc::new(fx_subagent::SubagentManager::new(
        fx_subagent::SubagentManagerDeps {
            factory: Arc::new(factory),
            limits: fx_subagent::SubagentLimits::default(),
        },
    ))
}

fn parent_loop_build_options(
    subagent_manager: &Arc<fx_subagent::SubagentManager>,
    config_manager: Option<Arc<std::sync::Mutex<fx_config::manager::ConfigManager>>>,
    session_bus: Option<fx_bus::SessionBus>,
    experiment_progress: Option<ProgressCallback>,
) -> startup::HeadlessLoopBuildOptions {
    startup::HeadlessLoopBuildOptions {
        memory_enabled: true,
        subagent_control: Some(
            Arc::clone(subagent_manager) as Arc<dyn fx_subagent::SubagentControl>
        ),
        config_manager,
        session_bus,
        experiment_progress,
        ..startup::HeadlessLoopBuildOptions::default()
    }
}

fn build_canary_monitor(data_dir: &Path) -> CanaryMonitor {
    let trigger = resolve_ripcord_path(data_dir).map(|path| {
        Arc::new(RipcordTrigger::new(path, data_dir.to_path_buf())) as Arc<dyn RollbackTrigger>
    });
    if trigger.is_none() {
        tracing::warn!(
            data_dir = %data_dir.display(),
            "fawx-ripcord not found; automatic rollback is disabled"
        );
    }
    CanaryMonitor::new(CanaryConfig::default(), trigger)
}

fn resolve_ripcord_path(data_dir: &Path) -> Option<PathBuf> {
    resolve_ripcord_path_with(
        ripcord_current_exe_candidate(),
        data_dir,
        std::env::var_os("PATH"),
    )
}

fn resolve_ripcord_path_with(
    current_exe_candidate: Option<PathBuf>,
    data_dir: &Path,
    path_env: Option<std::ffi::OsString>,
) -> Option<PathBuf> {
    current_exe_candidate
        .into_iter()
        .chain(std::iter::once(
            data_dir.join("bin").join(ripcord_binary_name()),
        ))
        .chain(path_candidates_from(path_env))
        .find(|path| path.is_file())
}

fn ripcord_current_exe_candidate() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    Some(exe.parent()?.join(ripcord_binary_name()))
}

fn path_candidates_from(path_env: Option<std::ffi::OsString>) -> Vec<PathBuf> {
    let Some(paths) = path_env else {
        return Vec::new();
    };
    std::env::split_paths(&paths)
        .map(|dir| dir.join(ripcord_binary_name()))
        .collect()
}

fn ripcord_binary_name() -> &'static str {
    #[cfg(windows)]
    {
        "fawx-ripcord.exe"
    }
    #[cfg(not(windows))]
    {
        "fawx-ripcord"
    }
}

/// Normalize embedded-mode config before constructing the headless app.
///
/// Embedded callers run inside another host process, so they should inherit
/// the host process working directory unless config already overrides it.
pub fn prepare_embedded_config(mut config: fx_config::FawxConfig) -> fx_config::FawxConfig {
    if config.tools.working_dir.is_none() {
        config.tools.working_dir = Some(startup::configured_working_dir(&config));
    }
    config
}

fn configured_data_dir(config: &fx_config::FawxConfig) -> PathBuf {
    config
        .general
        .data_dir
        .clone()
        .unwrap_or_else(startup::fawx_data_dir)
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
