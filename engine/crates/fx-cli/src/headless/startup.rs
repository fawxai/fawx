use super::*;
use fx_canary::{CanaryConfig, RipcordTrigger, RollbackTrigger};
use fx_consensus::ProgressCallback;
use std::ffi::OsString;

/// Request payload for embedded-mode headless startup.
///
/// Embedded callers inherit the host process working directory and do not
/// perform detached-lane workspace-root rebinding.
pub struct EmbeddedHeadlessAppRequest {
    pub system_prompt: Option<PathBuf>,
    pub experiment_progress: Option<ProgressCallback>,
}

/// Request payload for CLI/server headless startup.
///
/// Server-mode startup binds the workspace root to the built checkout so
/// detached clean-bisect lanes execute against the tested repo.
pub struct HeadlessStartupRequest {
    pub system_prompt: Option<PathBuf>,
    pub skip_session_db: bool,
    #[cfg(feature = "http")]
    pub wire_experiment_registry: bool,
}

pub struct HeadlessStartup {
    pub app: HeadlessApp,
    pub _logging_guard: WorkerGuard,
    #[cfg(feature = "http")]
    pub http_config: fx_config::HttpConfig,
    #[cfg(feature = "http")]
    pub telegram_config: fx_config::TelegramChannelConfig,
    #[cfg(feature = "http")]
    pub webhook_config: fx_config::WebhookConfig,
    #[cfg(feature = "http")]
    pub data_dir: PathBuf,
    pub improvement_provider: Option<Arc<dyn CompletionProvider + Send + Sync>>,
}

struct HeadlessAppBuildConfig {
    router: SharedModelRouter,
    config: FawxConfig,
    improvement_provider: Option<Arc<dyn CompletionProvider + Send + Sync>>,
    system_prompt: Option<PathBuf>,
    config_manager: Option<Arc<Mutex<ConfigManager>>>,
    data_dir: PathBuf,
    skip_session_db: bool,
    experiment_progress: Option<ProgressCallback>,
    #[cfg(feature = "http")]
    experiment_registry: Option<fx_api::SharedExperimentRegistry>,
}

struct PreparedHeadlessStartup {
    build_config: HeadlessAppBuildConfig,
    logging_guard: WorkerGuard,
    #[cfg(feature = "http")]
    http_config: fx_config::HttpConfig,
    #[cfg(feature = "http")]
    telegram_config: fx_config::TelegramChannelConfig,
    #[cfg(feature = "http")]
    webhook_config: fx_config::WebhookConfig,
    #[cfg(feature = "http")]
    data_dir: PathBuf,
    improvement_provider: Option<Arc<dyn CompletionProvider + Send + Sync>>,
}

pub fn build_embedded_headless_app(
    request: EmbeddedHeadlessAppRequest,
) -> anyhow::Result<HeadlessApp> {
    let auth_manager = crate::startup::load_auth_manager()?;
    let config = prepare_embedded_config(crate::startup::load_config()?);
    let router = build_seeded_router(&auth_manager, &config)?;
    let config_manager = Some(build_config_manager(&config));
    let data_dir = config
        .general
        .data_dir
        .clone()
        .unwrap_or_else(crate::startup::fawx_data_dir);
    let improvement_provider = crate::startup::build_improvement_provider(&auth_manager, &config);
    let build_config = HeadlessAppBuildConfig {
        data_dir,
        config_manager,
        improvement_provider,
        system_prompt: request.system_prompt,
        router,
        config,
        skip_session_db: false,
        experiment_progress: request.experiment_progress,
        #[cfg(feature = "http")]
        experiment_registry: None,
    };

    let mut app = build_headless_app(build_config)?;
    app.initialize();
    Ok(app)
}

pub fn build_headless_startup(request: HeadlessStartupRequest) -> anyhow::Result<HeadlessStartup> {
    touch_embedded_startup_symbols();
    let prepared = prepare_headless_startup(request)?;
    build_headless_startup_inner(prepared)
}

pub fn prepare_embedded_config(mut config: FawxConfig) -> FawxConfig {
    if config.tools.working_dir.is_none() {
        config.tools.working_dir = Some(crate::startup::configured_working_dir(&config));
    }
    config
}

pub(crate) fn resolve_ripcord_path_with(
    current_exe_candidate: Option<PathBuf>,
    data_dir: &Path,
    path_env: Option<OsString>,
) -> Option<PathBuf> {
    current_exe_candidate
        .into_iter()
        .chain(std::iter::once(
            data_dir.join("bin").join(ripcord_binary_name()),
        ))
        .chain(path_candidates_from(path_env))
        .find(|path| path.is_file())
}

pub(crate) fn ripcord_binary_name() -> &'static str {
    #[cfg(windows)]
    {
        "fawx-ripcord.exe"
    }
    #[cfg(not(windows))]
    {
        "fawx-ripcord"
    }
}

fn build_seeded_router(
    auth_manager: &fx_auth::auth::AuthManager,
    config: &FawxConfig,
) -> anyhow::Result<SharedModelRouter> {
    let mut router = crate::startup::build_router(auth_manager)?;
    seed_headless_router_active_model(&mut router, config);
    Ok(Arc::new(RwLock::new(router)))
}

fn build_config_manager(config: &FawxConfig) -> Arc<Mutex<ConfigManager>> {
    let data_dir = config
        .general
        .data_dir
        .clone()
        .unwrap_or_else(crate::startup::fawx_data_dir);
    let config_path = data_dir.join("config.toml");
    let manager = ConfigManager::from_config(config.clone(), config_path);
    Arc::new(Mutex::new(manager))
}

fn prepare_headless_startup(
    request: HeadlessStartupRequest,
) -> anyhow::Result<PreparedHeadlessStartup> {
    let mut config = crate::startup::load_config()?;
    crate::startup::bind_headless_workspace_root(&mut config);
    let logging_guard = init_serve_logging(&config)?;
    let auth_manager = crate::startup::load_auth_manager()?;
    let router = build_seeded_router(&auth_manager, &config)?;
    let data_dir = crate::startup::fawx_data_dir();
    let improvement_provider = crate::startup::build_improvement_provider(&auth_manager, &config);
    #[cfg(feature = "http")]
    let http_config = config.http.clone();
    #[cfg(feature = "http")]
    let telegram_config = config.telegram.clone();
    #[cfg(feature = "http")]
    let webhook_config = config.webhook.clone();
    Ok(PreparedHeadlessStartup {
        build_config: build_headless_startup_config(
            request,
            router,
            config,
            data_dir.clone(),
            improvement_provider.clone(),
        )?,
        logging_guard,
        #[cfg(feature = "http")]
        http_config,
        #[cfg(feature = "http")]
        telegram_config,
        #[cfg(feature = "http")]
        webhook_config,
        #[cfg(feature = "http")]
        data_dir,
        improvement_provider,
    })
}

fn build_headless_startup_config(
    request: HeadlessStartupRequest,
    router: SharedModelRouter,
    config: FawxConfig,
    data_dir: PathBuf,
    improvement_provider: Option<Arc<dyn CompletionProvider + Send + Sync>>,
) -> anyhow::Result<HeadlessAppBuildConfig> {
    let config_manager = Some(build_config_manager(&config));
    #[cfg(feature = "http")]
    let experiment_registry = build_experiment_registry(&request, &data_dir, &config)?;
    Ok(HeadlessAppBuildConfig {
        router,
        config,
        improvement_provider,
        system_prompt: request.system_prompt,
        config_manager,
        data_dir,
        skip_session_db: request.skip_session_db,
        experiment_progress: None,
        #[cfg(feature = "http")]
        experiment_registry,
    })
}

fn build_headless_startup_inner(
    prepared: PreparedHeadlessStartup,
) -> anyhow::Result<HeadlessStartup> {
    let app = build_headless_app(prepared.build_config)?;
    Ok(HeadlessStartup {
        app,
        _logging_guard: prepared.logging_guard,
        #[cfg(feature = "http")]
        http_config: prepared.http_config,
        #[cfg(feature = "http")]
        telegram_config: prepared.telegram_config,
        #[cfg(feature = "http")]
        webhook_config: prepared.webhook_config,
        #[cfg(feature = "http")]
        data_dir: prepared.data_dir,
        improvement_provider: prepared.improvement_provider,
    })
}

fn build_headless_app(build_config: HeadlessAppBuildConfig) -> anyhow::Result<HeadlessApp> {
    let session_bus = crate::startup::build_session_bus_for_data_dir(&build_config.data_dir);
    let credential_store = crate::startup::open_credential_store(&build_config.data_dir).ok();
    let subagent_manager = build_subagent_manager(
        Arc::clone(&build_config.router),
        &build_config.config,
        build_config.improvement_provider.clone(),
        session_bus.clone(),
        credential_store.clone(),
    );
    let bundle = build_loop_bundle(
        &build_config,
        &subagent_manager,
        session_bus.clone(),
        credential_store,
    )?;
    HeadlessApp::new(build_headless_app_deps(
        build_config,
        bundle,
        subagent_manager,
        session_bus,
    ))
}

fn build_subagent_manager(
    router: SharedModelRouter,
    config: &FawxConfig,
    improvement_provider: Option<Arc<dyn CompletionProvider + Send + Sync>>,
    session_bus: Option<SessionBus>,
    credential_store: Option<crate::startup::SharedCredentialStore>,
) -> Arc<SubagentManager> {
    let token_broker = crate::startup::build_token_broker(config, credential_store.as_ref());
    let factory = HeadlessSubagentFactory::new(HeadlessSubagentFactoryDeps {
        router,
        config: config.clone(),
        improvement_provider,
        session_bus,
        credential_store,
        token_broker,
    });
    Arc::new(SubagentManager::new(SubagentManagerDeps {
        factory: Arc::new(factory),
        limits: SubagentLimits::default(),
    }))
}

fn build_loop_bundle(
    build_config: &HeadlessAppBuildConfig,
    subagent_manager: &Arc<SubagentManager>,
    session_bus: Option<SessionBus>,
    credential_store: Option<crate::startup::SharedCredentialStore>,
) -> anyhow::Result<crate::startup::LoopEngineBundle> {
    let working_dir = crate::startup::configured_working_dir(&build_config.config);
    let options = HeadlessLoopBuildOptions {
        working_dir: Some(working_dir),
        session_registry: session_registry(&build_config.data_dir, build_config.skip_session_db),
        credential_store: credential_store.clone(),
        #[cfg(feature = "http")]
        experiment_registry: build_config.experiment_registry.clone(),
        ..parent_loop_build_options(
            subagent_manager,
            build_config.config_manager.clone(),
            session_bus,
            build_config.experiment_progress.clone(),
        )
    };
    build_headless_loop_engine_bundle(
        &build_config.config,
        build_config.improvement_provider.clone(),
        options,
    )
    .map_err(anyhow::Error::new)
}

fn session_registry(data_dir: &Path, skip_session_db: bool) -> Option<fx_session::SessionRegistry> {
    (!skip_session_db)
        .then(|| crate::startup::open_session_registry(data_dir))
        .flatten()
}

fn parent_loop_build_options(
    subagent_manager: &Arc<SubagentManager>,
    config_manager: Option<Arc<Mutex<ConfigManager>>>,
    session_bus: Option<SessionBus>,
    experiment_progress: Option<ProgressCallback>,
) -> HeadlessLoopBuildOptions {
    HeadlessLoopBuildOptions {
        memory_enabled: true,
        subagent_control: Some(
            Arc::clone(subagent_manager) as Arc<dyn fx_subagent::SubagentControl>
        ),
        config_manager,
        session_bus,
        experiment_progress,
        ..HeadlessLoopBuildOptions::default()
    }
}

fn build_headless_app_deps(
    build_config: HeadlessAppBuildConfig,
    bundle: crate::startup::LoopEngineBundle,
    subagent_manager: Arc<SubagentManager>,
    session_bus: Option<SessionBus>,
) -> HeadlessAppDeps {
    HeadlessAppDeps {
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
        session_key: Some(main_session_key()),
        cron_store: bundle.cron_store,
        startup_warnings: bundle.startup_warnings,
        stream_callback_slot: bundle.stream_callback_slot,
        permission_prompt_state: Some(bundle.permission_prompt_state),
        ripcord_journal: bundle.ripcord_journal,
        #[cfg(feature = "http")]
        experiment_registry: build_config.experiment_registry,
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

fn ripcord_current_exe_candidate() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    Some(exe.parent()?.join(ripcord_binary_name()))
}

fn path_candidates_from(path_env: Option<OsString>) -> Vec<PathBuf> {
    let Some(paths) = path_env else {
        return Vec::new();
    };
    std::env::split_paths(&paths)
        .map(|dir| dir.join(ripcord_binary_name()))
        .collect()
}

#[cfg(feature = "http")]
fn build_experiment_registry(
    request: &HeadlessStartupRequest,
    data_dir: &Path,
    config: &FawxConfig,
) -> anyhow::Result<Option<fx_api::SharedExperimentRegistry>> {
    if !request.wire_experiment_registry {
        return Ok(None);
    }
    let registry_data_dir = crate::startup::configured_data_dir(data_dir, config);
    crate::startup::build_shared_experiment_registry(&registry_data_dir)
        .map(Some)
        .map_err(anyhow::Error::new)
}

fn touch_embedded_startup_symbols() {
    // Prevent LTO dead-code elimination of embedded-only startup entry points
    // when the binary target only references the server-mode path directly.
    let _ = build_embedded_headless_app
        as fn(EmbeddedHeadlessAppRequest) -> anyhow::Result<HeadlessApp>;
    let _ = prepare_embedded_config as fn(FawxConfig) -> FawxConfig;
    let _ = std::mem::size_of::<EmbeddedHeadlessAppRequest>();
}
