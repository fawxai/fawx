use crate::handlers;
use crate::handlers::config::{
    handle_apply_config_preset, handle_config_get, handle_config_patch, handle_config_preset_diff,
    handle_config_presets, handle_config_set,
};
use crate::handlers::devices::{handle_delete_device, handle_list_devices};
use crate::handlers::errors::handle_recent_errors;
use crate::handlers::fleet::fleet_router;
use crate::handlers::health::{handle_health, handle_status};
use crate::handlers::message::handle_message;
use crate::handlers::pairing::{
    handle_adopt_local_device, handle_exchange_pair, handle_generate_pair,
};
use crate::handlers::phase4::{
    handle_server_restart, handle_server_status, handle_server_stop, handle_setup_status,
};
use crate::handlers::sessions::{
    handle_clear_session, handle_create_session, handle_delete_session, handle_get_context,
    handle_get_messages, handle_get_session, handle_get_session_memory, handle_list_sessions,
    handle_send_message, handle_send_to_session, handle_update_session_memory,
};
use crate::handlers::settings::{
    handle_get_thinking, handle_list_auth, handle_list_models, handle_list_skills,
    handle_set_model, handle_set_thinking,
};
use crate::handlers::webhook::handle_webhook;
use crate::middleware::auth_middleware;
use crate::state::HttpState;
use crate::telegram::webhook::handle_telegram_webhook;
use axum::middleware;
use axum::routing::{delete, get, patch, post, put};
use axum::Router;
use fx_fleet::FleetManager;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::Mutex;

const MAX_REQUEST_BYTES: usize = 1_048_576;

pub fn build_router(state: HttpState, fleet_manager: Option<Arc<Mutex<FleetManager>>>) -> Router {
    let v1_router = Router::new()
        .route(
            "/sessions",
            post(handle_create_session).get(handle_list_sessions),
        )
        .route(
            "/sessions/{id}",
            get(handle_get_session).delete(handle_delete_session),
        )
        .route("/sessions/{id}/clear", post(handle_clear_session))
        .route(
            "/sessions/{id}/messages",
            get(handle_get_messages).post(handle_send_message),
        )
        .route("/sessions/{id}/send", post(handle_send_to_session))
        .route("/sessions/{id}/context", get(handle_get_context))
        .route(
            "/sessions/{id}/memory",
            get(handle_get_session_memory).put(handle_update_session_memory),
        )
        .route(
            "/proposals/pending",
            get(handlers::proposals::handle_list_pending),
        )
        .route(
            "/proposals/history",
            get(handlers::proposals::handle_history),
        )
        .route(
            "/proposals/{id}/decide",
            post(handlers::proposals::handle_decide),
        )
        .route(
            "/proposals/{id}/diff",
            get(handlers::proposals::handle_get_diff),
        )
        .route("/models", get(handle_list_models))
        .route("/model", put(handle_set_model))
        .route(
            "/thinking",
            get(handle_get_thinking).put(handle_set_thinking),
        )
        .route("/skills", get(handle_list_skills))
        .route(
            "/skills/search",
            get(handlers::marketplace::handle_search_skills),
        )
        .route(
            "/skills/install",
            post(handlers::marketplace::handle_install_skill),
        )
        .route(
            "/skills/{name}",
            delete(handlers::marketplace::handle_remove_skill)
                .patch(handlers::marketplace::handle_update_skill_permissions),
        )
        .route("/usage", get(handlers::usage::handle_usage))
        .route(
            "/experiments",
            get(handlers::experiments::handle_list_experiments)
                .post(handlers::experiments::handle_create_experiment),
        )
        .route(
            "/experiments/{id}",
            get(handlers::experiments::handle_get_experiment),
        )
        .route(
            "/experiments/{id}/results",
            get(handlers::experiments::handle_get_experiment_results),
        )
        .route(
            "/experiments/{id}/stop",
            post(handlers::experiments::handle_stop_experiment),
        )
        .route(
            "/permissions",
            get(handlers::permissions::handle_get_permissions)
                .patch(handlers::permissions::handle_patch_permissions),
        )
        .route(
            "/permissions/prompts/{id}/respond",
            post(handlers::permission_prompts::handle_respond),
        )
        .route("/ripcord/status", get(handlers::ripcord::handle_status))
        .route("/ripcord/journal", get(handlers::ripcord::handle_journal))
        .route("/ripcord/pull", post(handlers::ripcord::handle_pull))
        .route("/ripcord/approve", post(handlers::ripcord::handle_approve))
        .route(
            "/synthesis",
            get(handlers::synthesis::handle_get_synthesis)
                .put(handlers::synthesis::handle_set_synthesis)
                .delete(handlers::synthesis::handle_clear_synthesis),
        )
        .route("/config", patch(handle_config_patch))
        .route("/config/presets", get(handle_config_presets))
        .route("/config/preset/{name}", post(handle_apply_config_preset))
        .route("/config/preset/{name}/diff", get(handle_config_preset_diff))
        .route("/auth", get(handle_list_auth))
        .route(
            "/auth/{provider}",
            delete(handlers::auth::handle_delete_provider),
        )
        .route(
            "/auth/{provider}/refresh",
            post(handlers::oauth::handle_oauth_refresh),
        )
        .route("/server/status", get(handle_server_status))
        .route("/server/restart", post(handle_server_restart))
        .route("/server/stop", post(handle_server_stop))
        .route(
            "/launchagent/reload",
            post(handlers::launchagent::handle_launchagent_reload),
        )
        .route(
            "/fleet/overview",
            get(handlers::fleet_dashboard::handle_fleet_overview),
        )
        .route(
            "/fleet/nodes",
            get(handlers::fleet_dashboard::handle_fleet_nodes),
        )
        .route(
            "/fleet/nodes/{id}",
            get(handlers::fleet_dashboard::handle_fleet_node_detail)
                .delete(handlers::fleet_dashboard::handle_remove_fleet_node),
        )
        .route(
            "/fleet/nodes/{id}/tasks",
            post(handlers::fleet_dashboard::handle_dispatch_task),
        )
        .route("/devices", get(handle_list_devices))
        .route("/errors/recent", get(handle_recent_errors))
        .route("/git/status", get(handlers::git::handle_git_status))
        .route("/git/log", get(handlers::git::handle_git_log))
        .route("/git/diff", get(handlers::git::handle_git_diff))
        .route("/git/stage", post(handlers::git::handle_git_stage))
        .route("/git/unstage", post(handlers::git::handle_git_unstage))
        .route("/git/commit", post(handlers::git::handle_git_commit))
        .route("/git/push", post(handlers::git::handle_git_push))
        .route("/git/pull", post(handlers::git::handle_git_pull))
        .route("/git/fetch", post(handlers::git::handle_git_fetch))
        .route(
            "/telemetry/consent",
            get(handlers::telemetry::handle_get_consent)
                .patch(handlers::telemetry::handle_patch_consent),
        )
        .route(
            "/telemetry/signals",
            get(handlers::telemetry::handle_get_signals)
                .delete(handlers::telemetry::handle_delete_signals),
        )
        .route("/devices/{id}", delete(handle_delete_device))
        .route("/pair/generate", post(handle_generate_pair))
        .route(
            "/cron/jobs",
            get(crate::handlers::cron::handle_list_jobs)
                .post(crate::handlers::cron::handle_create_job),
        )
        .route(
            "/cron/jobs/{id}",
            get(crate::handlers::cron::handle_get_job)
                .put(crate::handlers::cron::handle_update_job)
                .delete(crate::handlers::cron::handle_delete_job),
        )
        .route(
            "/cron/jobs/{id}/run",
            post(crate::handlers::cron::handle_trigger_job),
        )
        .route(
            "/cron/jobs/{id}/runs",
            get(crate::handlers::cron::handle_list_runs),
        );

    let setup_v1_router = Router::new()
        .route("/setup/status", get(handle_setup_status))
        .route("/setup/adopt-local", post(handle_adopt_local_device))
        .route(
            "/auth/anthropic/setup-token",
            post(handlers::auth::handle_setup_token),
        )
        .route(
            "/auth/{provider}/api-key",
            post(handlers::auth::handle_store_api_key),
        )
        .route(
            "/auth/{provider}/verify",
            post(handlers::auth::handle_verify_provider),
        )
        .route(
            "/auth/{provider}/oauth-start",
            get(handlers::oauth::handle_oauth_start),
        )
        .route(
            "/auth/{provider}/oauth-callback",
            post(handlers::oauth::handle_oauth_callback),
        )
        .route(
            "/launchagent/status",
            get(handlers::launchagent::handle_launchagent_status),
        )
        .route(
            "/launchagent/install",
            post(handlers::launchagent::handle_launchagent_install),
        )
        .route(
            "/launchagent/uninstall",
            post(handlers::launchagent::handle_launchagent_uninstall),
        )
        .route("/pair/qr", get(handlers::pairing::handle_qr_pairing))
        .route(
            "/tailscale/cert",
            post(handlers::pairing::handle_tailscale_cert),
        );

    let authenticated = Router::new()
        .route("/message", post(handle_message))
        .route("/status", get(handle_status))
        .route("/config", get(handle_config_get).post(handle_config_set))
        .route("/webhook/{channel_id}", post(handle_webhook))
        .nest("/v1", v1_router)
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ));

    let public = Router::new()
        .route("/health", get(handle_health))
        .route("/v1/pair", post(handle_exchange_pair))
        .route("/telegram/webhook", post(handle_telegram_webhook))
        .nest("/v1", setup_v1_router);
    let router = authenticated.merge(public).with_state(state);

    merge_fleet_router(router, fleet_manager)
        .layer(axum::extract::DefaultBodyLimit::max(MAX_REQUEST_BYTES))
}

pub fn merge_fleet_router(
    router: Router,
    fleet_manager: Option<Arc<Mutex<FleetManager>>>,
) -> Router {
    match fleet_manager {
        Some(manager) => router.merge(fleet_router(manager)),
        None => router,
    }
}

pub fn load_fleet_manager_if_initialized(
    data_dir: &Path,
) -> anyhow::Result<Option<Arc<Mutex<FleetManager>>>> {
    let fleet_dir = data_dir.join("fleet");
    if !fleet_dir.join("fleet.key").is_file() {
        return Ok(None);
    }
    let manager = FleetManager::load(&fleet_dir)?;
    Ok(Some(Arc::new(Mutex::new(manager))))
}
