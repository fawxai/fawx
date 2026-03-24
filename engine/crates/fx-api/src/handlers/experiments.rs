use crate::experiment_registry::{
    Experiment, ExperimentConfig, ExperimentKind, ExperimentResult, ExperimentStatus, SkippedItem,
};
use crate::state::HttpState;
use crate::types::ErrorBody;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use std::sync::Arc;

use super::HandlerResult;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ExperimentSummary {
    pub id: String,
    pub name: String,
    pub kind: ExperimentKind,
    pub status: ExperimentStatus,
    pub score_summary: String,
    pub created_at: u64,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ExperimentsListResponse {
    pub experiments: Vec<ExperimentSummary>,
    pub total: usize,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct CreateExperimentRequest {
    pub name: Option<String>,
    pub kind: Option<String>,
    #[serde(default)]
    pub config: Option<ExperimentConfig>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct CreateExperimentResponse {
    pub id: String,
    pub created: bool,
    pub status: ExperimentStatus,
}

pub type ExperimentDetailResponse = Experiment;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ExperimentResultsResponse {
    pub id: String,
    pub status: ExperimentStatus,
    pub score_summary: String,
    pub leaders: Vec<ExperimentLeader>,
    pub tournament: Option<ExperimentTournament>,
    pub plans_generated: usize,
    pub proposals_written: Vec<String>,
    pub branches_created: Vec<String>,
    pub skipped: Vec<SkippedItem>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ExperimentLeader {
    pub chain_id: String,
    pub name: String,
    pub score: f64,
    pub risk: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ExperimentTournament {
    pub round: usize,
    pub total_rounds: usize,
    pub remaining_matches: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct StopExperimentResponse {
    pub id: String,
    pub stopping: bool,
}

#[derive(Debug, Default, Deserialize)]
pub struct ListParams {
    #[serde(default)]
    pub status: Option<String>,
}

// GET /v1/experiments
pub async fn handle_list_experiments(
    State(state): State<HttpState>,
    Query(params): Query<ListParams>,
) -> HandlerResult<Json<ExperimentsListResponse>> {
    let filter = parse_status_filter(params.status.as_deref())?;
    let response = {
        let registry = state.experiment_registry.lock().await;
        let experiments = match filter {
            Some(status) => registry.list_by_status(status),
            None => registry.list(),
        };
        let summaries: Vec<ExperimentSummary> =
            experiments.into_iter().map(experiment_summary).collect();
        Json(ExperimentsListResponse {
            total: summaries.len(),
            experiments: summaries,
        })
    };
    Ok(response)
}

// POST /v1/experiments
pub async fn handle_create_experiment(
    State(state): State<HttpState>,
    Json(request): Json<CreateExperimentRequest>,
) -> HandlerResult<(StatusCode, Json<CreateExperimentResponse>)> {
    let name = validate_name(request.name)?;
    let kind = parse_kind(request.kind)?;
    let config = request.config.unwrap_or_default();
    let provider = state.improvement_provider.clone();
    let (experiment, cancel_token) = start_experiment(
        &state.experiment_registry,
        name,
        kind,
        config,
        provider.is_some(),
    )
    .await?;

    if let (Some(provider), Some(cancel_token)) = (provider, cancel_token) {
        spawn_experiment_execution(
            experiment.id.clone(),
            Arc::clone(&state.experiment_registry),
            provider,
            state.data_dir.clone(),
            cancel_token,
        );
    }

    Ok((
        StatusCode::CREATED,
        Json(CreateExperimentResponse {
            id: experiment.id,
            created: true,
            status: experiment.status,
        }),
    ))
}

async fn start_experiment(
    registry: &Arc<tokio::sync::Mutex<crate::experiment_registry::ExperimentRegistry>>,
    name: String,
    kind: ExperimentKind,
    config: ExperimentConfig,
    include_cancel_token: bool,
) -> HandlerResult<(Experiment, Option<tokio_util::sync::CancellationToken>)> {
    let mut registry = registry.lock().await;
    if registry.has_running_experiment() {
        return Err(concurrent_experiment_conflict());
    }
    let experiment = registry
        .create(name, kind, config)
        .map_err(internal_error)?;
    registry.start(&experiment.id).map_err(internal_error)?;
    let cancel_token = if include_cancel_token {
        Some(required_cancel_token(&registry, &experiment.id)?)
    } else {
        None
    };
    let experiment = registry.get(&experiment.id).cloned().unwrap_or(experiment);
    Ok((experiment, cancel_token))
}

fn required_cancel_token(
    registry: &crate::experiment_registry::ExperimentRegistry,
    experiment_id: &str,
) -> HandlerResult<tokio_util::sync::CancellationToken> {
    registry.cancel_token(experiment_id).ok_or_else(|| {
        internal_error(format!(
            "failed to load cancellation token for experiment '{experiment_id}'"
        ))
    })
}

// GET /v1/experiments/{id}
pub async fn handle_get_experiment(
    State(state): State<HttpState>,
    Path(id): Path<String>,
) -> HandlerResult<Json<ExperimentDetailResponse>> {
    let experiment = state
        .experiment_registry
        .lock()
        .await
        .get(&id)
        .cloned()
        .ok_or_else(|| experiment_not_found(&id))?;
    Ok(Json(experiment))
}

// GET /v1/experiments/{id}/results
pub async fn handle_get_experiment_results(
    State(state): State<HttpState>,
    Path(id): Path<String>,
) -> HandlerResult<Json<ExperimentResultsResponse>> {
    let experiment = state
        .experiment_registry
        .lock()
        .await
        .get(&id)
        .cloned()
        .ok_or_else(|| experiment_not_found(&id))?;
    Ok(Json(build_results_response(&experiment)))
}

// POST /v1/experiments/{id}/stop
pub async fn handle_stop_experiment(
    State(state): State<HttpState>,
    Path(id): Path<String>,
) -> HandlerResult<Json<StopExperimentResponse>> {
    let mut registry = state.experiment_registry.lock().await;
    let status = registry
        .get(&id)
        .map(|experiment| experiment.status)
        .ok_or_else(|| experiment_not_found(&id))?;

    if !matches!(status, ExperimentStatus::Queued | ExperimentStatus::Running) {
        return Err(stop_conflict(status));
    }

    registry.stop(&id).map_err(internal_error)?;
    Ok(Json(StopExperimentResponse { id, stopping: true }))
}

fn completed_summary(result: Option<&ExperimentResult>) -> String {
    let Some(result) = result else {
        return "completed".to_string();
    };
    if let Some(summary) = stored_score_summary(result) {
        return summary;
    }

    let plans = pluralize(result.plans_generated, "plan", "plans");
    let proposals = pluralize(result.proposals_written.len(), "proposal", "proposals");
    format!(
        "{} {plans} generated, {} {proposals} written",
        result.plans_generated,
        result.proposals_written.len(),
    )
}

fn build_results_response(experiment: &Experiment) -> ExperimentResultsResponse {
    let Some(result) = experiment.result.as_ref() else {
        return empty_results_response(experiment);
    };

    ExperimentResultsResponse {
        id: experiment.id.clone(),
        status: experiment.status,
        score_summary: score_summary(experiment),
        leaders: build_result_leaders(result),
        tournament: None,
        plans_generated: result.plans_generated,
        proposals_written: result.proposals_written.clone(),
        branches_created: result.branches_created.clone(),
        skipped: result.skipped.clone(),
    }
}

fn build_result_leaders(result: &ExperimentResult) -> Vec<ExperimentLeader> {
    result
        .proposals_written
        .iter()
        .enumerate()
        .map(|(i, proposal)| ExperimentLeader {
            chain_id: format!("chain-{i}"),
            name: proposal.clone(),
            score: if result.plans_generated > 0 { 1.0 } else { 0.0 },
            risk: "low".to_string(),
        })
        .collect()
}

fn empty_results_response(experiment: &Experiment) -> ExperimentResultsResponse {
    ExperimentResultsResponse {
        id: experiment.id.clone(),
        status: experiment.status,
        score_summary: score_summary(experiment),
        leaders: Vec::new(),
        tournament: None,
        plans_generated: 0,
        proposals_written: Vec::new(),
        branches_created: Vec::new(),
        skipped: Vec::new(),
    }
}

fn spawn_experiment_execution(
    experiment_id: String,
    registry: Arc<tokio::sync::Mutex<crate::experiment_registry::ExperimentRegistry>>,
    provider: Arc<dyn fx_llm::CompletionProvider + Send + Sync>,
    data_dir: std::path::PathBuf,
    cancel_token: tokio_util::sync::CancellationToken,
) {
    tokio::spawn(async move {
        tracing::info!(experiment_id = %experiment_id, "Starting experiment execution");
        let result =
            await_experiment_result(cancel_token, run_experiment(provider.as_ref(), &data_dir))
                .await;
        record_experiment_outcome(&experiment_id, registry, result).await;
    });
}

async fn await_experiment_result<E>(
    cancel_token: tokio_util::sync::CancellationToken,
    run: impl std::future::Future<Output = Result<fx_improve::ImprovementRunResult, E>>,
) -> Result<fx_improve::ImprovementRunResult, String>
where
    E: std::fmt::Display,
{
    tokio::select! {
        biased;
        _ = cancel_token.cancelled() => Err("Cancelled by user".to_string()),
        result = run => result.map_err(|error| error.to_string()),
    }
}

async fn record_experiment_outcome(
    experiment_id: &str,
    registry: Arc<tokio::sync::Mutex<crate::experiment_registry::ExperimentRegistry>>,
    result: Result<fx_improve::ImprovementRunResult, String>,
) {
    let mut registry = registry.lock().await;
    if let Err(error) = apply_experiment_outcome(&mut registry, experiment_id, result) {
        tracing::warn!(
            experiment_id = %experiment_id,
            error = %error,
            "Failed to record experiment outcome"
        );
    }
}

fn apply_experiment_outcome(
    registry: &mut crate::experiment_registry::ExperimentRegistry,
    experiment_id: &str,
    result: Result<fx_improve::ImprovementRunResult, String>,
) -> Result<(), String> {
    match result {
        Ok(run_result) => {
            tracing::info!(
                experiment_id = %experiment_id,
                plans = run_result.plans_generated,
                "Experiment completed"
            );
            registry.complete(experiment_id, convert_run_result(run_result))
        }
        Err(error) => {
            if error == "Cancelled by user" {
                tracing::info!(experiment_id = %experiment_id, "Experiment cancelled");
                if registry
                    .get(experiment_id)
                    .is_some_and(|experiment| experiment.status == ExperimentStatus::Stopped)
                {
                    return Ok(());
                }
            } else {
                tracing::warn!(
                    experiment_id = %experiment_id,
                    error = %error,
                    "Experiment failed"
                );
            }
            registry.fail(experiment_id, error)
        }
    }
}

async fn run_experiment(
    provider: &dyn fx_llm::CompletionProvider,
    data_dir: &std::path::Path,
) -> Result<fx_improve::ImprovementRunResult, fx_improve::ImprovementError> {
    let signal_store = fx_memory::SignalStore::new(data_dir, "experiment")
        .map_err(|e| fx_improve::ImprovementError::Analysis(e.to_string()))?;
    let config = fx_improve::ImprovementConfig::default();
    let paths = fx_improve::CyclePaths {
        data_dir,
        repo_root: data_dir,
        proposals_dir: &data_dir.join("proposals"),
    };
    fx_improve::run_improvement_cycle(&signal_store, provider, &config, &paths).await
}

fn convert_run_result(
    result: fx_improve::ImprovementRunResult,
) -> crate::experiment_registry::ExperimentResult {
    crate::experiment_registry::ExperimentResult {
        plans_generated: result.plans_generated,
        proposals_written: result
            .proposals_written
            .into_iter()
            .map(|p| p.display().to_string())
            .collect(),
        branches_created: result.branches_created,
        score_summary: None,
        skipped: result
            .skipped
            .into_iter()
            .map(|(name, reason)| crate::experiment_registry::SkippedItem { name, reason })
            .collect(),
    }
}

fn experiment_not_found(id: &str) -> (StatusCode, Json<ErrorBody>) {
    (
        StatusCode::NOT_FOUND,
        Json(ErrorBody {
            error: format!("Experiment '{id}' not found"),
        }),
    )
}

fn experiment_summary(experiment: &Experiment) -> ExperimentSummary {
    ExperimentSummary {
        id: experiment.id.clone(),
        name: experiment.name.clone(),
        kind: experiment.kind,
        status: experiment.status,
        score_summary: score_summary(experiment),
        created_at: experiment.created_at,
    }
}

fn internal_error(error: String) -> (StatusCode, Json<ErrorBody>) {
    (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorBody { error }))
}

fn parse_kind(kind: Option<String>) -> Result<ExperimentKind, (StatusCode, Json<ErrorBody>)> {
    let raw = kind.ok_or_else(|| validation_error("kind is required"))?;
    let normalized = raw.trim();
    if normalized.is_empty() {
        return Err(validation_error("kind is required"));
    }
    ExperimentKind::from_str(normalized).map_err(validation_error)
}

fn parse_status_filter(
    status: Option<&str>,
) -> Result<Option<ExperimentStatus>, (StatusCode, Json<ErrorBody>)> {
    let Some(raw) = status else {
        return Ok(None);
    };
    let normalized = raw.trim();
    if normalized.is_empty() {
        return Ok(None);
    }
    ExperimentStatus::from_str(normalized)
        .map(Some)
        .map_err(validation_error)
}

fn pluralize<'a>(count: usize, singular: &'a str, plural: &'a str) -> &'a str {
    if count == 1 {
        singular
    } else {
        plural
    }
}

fn score_summary(experiment: &Experiment) -> String {
    match experiment.status {
        ExperimentStatus::Queued => "waiting to start".to_string(),
        ExperimentStatus::Running => running_summary(experiment),
        ExperimentStatus::Completed => completed_summary(experiment.result.as_ref()),
        ExperimentStatus::Stopped => "stopped".to_string(),
        ExperimentStatus::Failed => "failed".to_string(),
    }
}

fn running_summary(experiment: &Experiment) -> String {
    let Some(progress) = &experiment.progress else {
        return "running".to_string();
    };
    if progress.total_matches == 0 {
        return "running".to_string();
    }
    format!(
        "match {} of {}",
        progress.completed_matches, progress.total_matches
    )
}

fn stored_score_summary(result: &ExperimentResult) -> Option<String> {
    result
        .score_summary
        .as_deref()
        .map(str::trim)
        .filter(|summary| !summary.is_empty())
        .map(ToString::to_string)
}

fn concurrent_experiment_conflict() -> (StatusCode, Json<ErrorBody>) {
    validation_with_status(
        StatusCode::CONFLICT,
        "another experiment is already running".to_string(),
    )
}

fn stop_conflict(status: ExperimentStatus) -> (StatusCode, Json<ErrorBody>) {
    validation_with_status(
        StatusCode::CONFLICT,
        format!("experiment is not running (status: {status})"),
    )
}

fn validate_name(name: Option<String>) -> Result<String, (StatusCode, Json<ErrorBody>)> {
    let raw = name.ok_or_else(|| validation_error("name is required"))?;
    let normalized = raw.trim();
    if normalized.is_empty() {
        return Err(validation_error("name is required"));
    }
    if normalized.chars().count() > 200 {
        return Err(validation_error("name must be at most 200 characters"));
    }
    Ok(normalized.to_string())
}

fn validation_error(message: impl Into<String>) -> (StatusCode, Json<ErrorBody>) {
    validation_with_status(StatusCode::UNPROCESSABLE_ENTITY, message.into())
}

fn validation_with_status(status: StatusCode, error: String) -> (StatusCode, Json<ErrorBody>) {
    (status, Json(ErrorBody { error }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::devices::DeviceStore;
    use crate::engine::{AppEngine, ConfigManagerHandle, CycleResult};
    use crate::experiment_registry::ExperimentRegistry;
    use crate::pairing::PairingState;
    use crate::server_runtime::ServerRuntime;
    use crate::state::{build_channel_runtime, in_memory_telemetry, SharedReadState};
    use crate::types::{
        AuthProviderDto, ContextInfoDto, ErrorRecordDto, ModelInfoDto, ModelSwitchDto,
        SkillSummaryDto, ThinkingLevelDto,
    };
    use anyhow::anyhow;
    use async_trait::async_trait;
    use axum::{
        body::Body,
        http::Request,
        routing::{get, post},
        Router,
    };
    use fx_bus::SessionBus;
    use fx_core::types::InputSource;
    use fx_kernel::StreamCallback;
    use fx_llm::{DocumentAttachment, ImageAttachment, Message};
    use std::sync::Arc;
    use std::time::Instant;
    use tempfile::TempDir;
    use tokio::sync::Mutex;
    use tower::ServiceExt;

    struct TestApp;

    #[async_trait]
    impl AppEngine for TestApp {
        async fn process_message(
            &mut self,
            _input: &str,
            _images: Vec<ImageAttachment>,
            _documents: Vec<DocumentAttachment>,
            _source: InputSource,
            _callback: Option<StreamCallback>,
        ) -> Result<CycleResult, anyhow::Error> {
            Err(anyhow!("not used in experiments tests"))
        }

        async fn process_message_with_context(
            &mut self,
            _input: &str,
            _images: Vec<ImageAttachment>,
            _documents: Vec<DocumentAttachment>,
            _context: Vec<Message>,
            _source: InputSource,
            _callback: Option<StreamCallback>,
        ) -> Result<(CycleResult, Vec<Message>), anyhow::Error> {
            Err(anyhow!("not used in experiments tests"))
        }

        fn active_model(&self) -> &str {
            "test-model"
        }

        fn available_models(&self) -> Vec<ModelInfoDto> {
            Vec::new()
        }

        fn set_active_model(&mut self, selector: &str) -> Result<ModelSwitchDto, anyhow::Error> {
            Ok(ModelSwitchDto {
                previous_model: "test-model".to_string(),
                active_model: selector.to_string(),
                thinking_adjusted: None,
            })
        }

        fn thinking_level(&self) -> ThinkingLevelDto {
            ThinkingLevelDto {
                level: "medium".to_string(),
                budget_tokens: None,
                available: vec!["low".to_string(), "medium".to_string()],
            }
        }

        fn context_info(&self) -> ContextInfoDto {
            ContextInfoDto {
                used_tokens: 0,
                max_tokens: 0,
                percentage: 0.0,
                compaction_threshold: 0.0,
            }
        }

        fn context_info_for_messages(&self, _messages: &[Message]) -> ContextInfoDto {
            self.context_info()
        }

        fn set_thinking_level(&mut self, level: &str) -> Result<ThinkingLevelDto, anyhow::Error> {
            Ok(ThinkingLevelDto {
                level: level.to_string(),
                budget_tokens: None,
                available: vec![level.to_string()],
            })
        }

        fn skill_summaries(&self) -> Vec<SkillSummaryDto> {
            Vec::new()
        }

        fn auth_provider_statuses(&self) -> Vec<AuthProviderDto> {
            Vec::new()
        }

        fn config_manager(&self) -> Option<ConfigManagerHandle> {
            None
        }

        fn session_bus(&self) -> Option<&SessionBus> {
            None
        }

        fn recent_errors(&self, _limit: usize) -> Vec<ErrorRecordDto> {
            Vec::new()
        }
    }

    fn test_registry(data_dir: &std::path::Path) -> Arc<Mutex<ExperimentRegistry>> {
        let registry = ExperimentRegistry::new(data_dir).expect("registry");
        Arc::new(Mutex::new(registry))
    }

    fn test_state() -> (TempDir, HttpState) {
        let temp_dir = TempDir::new().expect("tempdir");
        let data_dir = temp_dir.path().to_path_buf();
        let app = TestApp;
        let state = HttpState {
            app: Arc::new(Mutex::new(app)),
            shared: Arc::new(SharedReadState::from_app(&TestApp)),
            config_manager: None,
            session_registry: None,
            start_time: Instant::now(),
            server_runtime: ServerRuntime::local(8400),
            tailscale_ip: None,
            bearer_token: "test-token".to_string(),
            pairing: Arc::new(Mutex::new(PairingState::new())),
            devices: Arc::new(Mutex::new(DeviceStore::new())),
            devices_path: None,
            channels: build_channel_runtime(None, vec![]),
            data_dir: data_dir.clone(),
            synthesis: Arc::new(crate::handlers::synthesis::SynthesisState::new(false)),
            oauth_flows: Arc::new(crate::handlers::oauth::OAuthFlowStore::new()),
            permission_prompts: Arc::new(fx_kernel::PermissionPromptState::new()),
            ripcord: None,
            fleet_manager: None,
            cron_store: None,
            experiment_registry: test_registry(&data_dir),
            improvement_provider: None,
            telemetry: in_memory_telemetry(),
        };
        (temp_dir, state)
    }

    fn experiment_router(state: HttpState) -> Router {
        Router::new()
            .route("/experiments/{id}", get(handle_get_experiment))
            .route(
                "/experiments/{id}/results",
                get(handle_get_experiment_results),
            )
            .route("/experiments/{id}/stop", post(handle_stop_experiment))
            .with_state(state)
    }

    async fn request_status(app: Router, method: &str, uri: &str) -> StatusCode {
        let request = Request::builder()
            .method(method)
            .uri(uri)
            .body(Body::empty())
            .expect("request");
        app.oneshot(request).await.expect("response").status()
    }

    #[test]
    fn create_request_deserializes_with_optional_config() {
        let json = r#"{"name":"test","kind":"proof_of_fitness","config":{}}"#;
        let request: CreateExperimentRequest = serde_json::from_str(json).expect("request");
        assert_eq!(request.kind.as_deref(), Some("proof_of_fitness"));
        assert!(request.config.is_some());
    }

    #[test]
    fn detail_response_serializes_full_experiment() {
        let response = ExperimentDetailResponse {
            id: "exp1".to_string(),
            name: "Test".to_string(),
            kind: ExperimentKind::ProofOfFitness,
            status: ExperimentStatus::Running,
            config: ExperimentConfig::default(),
            created_at: 1_700_000_000,
            started_at: Some(1_700_000_060),
            completed_at: None,
            fleet_nodes: vec!["node-1".to_string()],
            progress: Some(crate::experiment_registry::ExperimentProgress {
                completed_matches: 2,
                total_matches: 4,
            }),
            result: None,
            error: None,
        };

        let json = serde_json::to_value(response).expect("json");
        assert_eq!(json["progress"]["completed_matches"], 2);
    }

    #[test]
    fn results_response_serializes_empty_shape() {
        let response = ExperimentResultsResponse {
            id: "exp1".to_string(),
            status: ExperimentStatus::Queued,
            score_summary: "waiting to start".to_string(),
            leaders: Vec::new(),
            tournament: None,
            plans_generated: 0,
            proposals_written: Vec::new(),
            branches_created: Vec::new(),
            skipped: Vec::new(),
        };

        let json = serde_json::to_value(response).expect("json");
        assert!(json["leaders"].as_array().expect("leaders").is_empty());
        assert!(json["tournament"].is_null());
        assert_eq!(json["score_summary"], "waiting to start");
    }

    #[test]
    fn experiment_leader_serializes_name_and_risk() {
        let leader = ExperimentLeader {
            chain_id: "chain-a".to_string(),
            name: "Timeout budget increase".to_string(),
            score: 91.2,
            risk: "low".to_string(),
        };

        let json = serde_json::to_value(leader).expect("json");
        assert_eq!(json["name"], "Timeout budget increase");
        assert_eq!(json["risk"], "low");
    }

    #[tokio::test]
    async fn create_returns_created_and_list_includes_summary() {
        let (_temp_dir, state) = test_state();
        let request = CreateExperimentRequest {
            name: Some("Prompt tournament".to_string()),
            kind: Some("proof_of_fitness".to_string()),
            config: None,
        };

        let (status, Json(created)) = handle_create_experiment(State(state.clone()), Json(request))
            .await
            .expect("created experiment");
        assert_eq!(status, StatusCode::CREATED);
        assert!(created.created);
        assert_eq!(created.status, ExperimentStatus::Running);

        let Json(list) = handle_list_experiments(State(state), Query(ListParams::default()))
            .await
            .expect("list experiments");
        assert_eq!(list.total, 1);
        assert_eq!(list.experiments[0].score_summary, "running");
    }

    #[tokio::test]
    async fn await_experiment_result_returns_cancelled_when_token_is_cancelled() {
        let token = tokio_util::sync::CancellationToken::new();
        token.cancel();

        let result = await_experiment_result(
            token,
            std::future::pending::<Result<fx_improve::ImprovementRunResult, anyhow::Error>>(),
        )
        .await;

        assert!(matches!(result, Err(message) if message == "Cancelled by user"));
    }

    #[tokio::test]
    async fn list_uses_match_label_for_running_experiment() {
        let (_temp_dir, state) = test_state();
        {
            let mut registry = state.experiment_registry.lock().await;
            let experiment = registry
                .create(
                    "Tournament".to_string(),
                    ExperimentKind::Tournament,
                    ExperimentConfig::default(),
                )
                .expect("create experiment");
            registry.start(&experiment.id).expect("start experiment");
            registry
                .update_progress(
                    &experiment.id,
                    crate::experiment_registry::ExperimentProgress {
                        completed_matches: 2,
                        total_matches: 4,
                    },
                )
                .expect("update progress");
        }

        let Json(list) = handle_list_experiments(State(state), Query(ListParams::default()))
            .await
            .expect("list experiments");
        assert_eq!(list.experiments[0].score_summary, "match 2 of 4");
    }

    #[tokio::test]
    async fn list_uses_recorded_score_summary_for_completed_experiment() {
        let (_temp_dir, state) = test_state();
        {
            let mut registry = state.experiment_registry.lock().await;
            let experiment = registry
                .create(
                    "Tournament".to_string(),
                    ExperimentKind::Tournament,
                    ExperimentConfig::default(),
                )
                .expect("create experiment");
            registry.start(&experiment.id).expect("start experiment");
            registry
                .complete(
                    &experiment.id,
                    ExperimentResult {
                        plans_generated: 0,
                        proposals_written: Vec::new(),
                        branches_created: Vec::new(),
                        score_summary: Some("winner: control node".to_string()),
                        skipped: Vec::new(),
                    },
                )
                .expect("complete experiment");
        }

        let Json(list) = handle_list_experiments(State(state), Query(ListParams::default()))
            .await
            .expect("list experiments");
        assert_eq!(list.experiments[0].score_summary, "winner: control node");
    }

    #[tokio::test]
    async fn create_returns_conflict_when_experiment_is_running() {
        let (_temp_dir, state) = test_state();
        {
            let mut registry = state.experiment_registry.lock().await;
            let experiment = registry
                .create(
                    "Running".to_string(),
                    ExperimentKind::ProofOfFitness,
                    ExperimentConfig::default(),
                )
                .expect("create experiment");
            registry.start(&experiment.id).expect("start experiment");
        }

        let request = CreateExperimentRequest {
            name: Some("Prompt tournament".to_string()),
            kind: Some("proof_of_fitness".to_string()),
            config: None,
        };

        let error = handle_create_experiment(State(state), Json(request))
            .await
            .expect_err("conflict error");
        assert_eq!(error.0, StatusCode::CONFLICT);
        assert_eq!(error.1 .0.error, "another experiment is already running");
    }

    #[tokio::test]
    async fn create_rejects_blank_name() {
        let (_temp_dir, state) = test_state();
        let request = CreateExperimentRequest {
            name: Some("   ".to_string()),
            kind: Some("proof_of_fitness".to_string()),
            config: None,
        };

        let error = handle_create_experiment(State(state), Json(request))
            .await
            .expect_err("validation error");
        assert_eq!(error.0, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(error.1 .0.error, "name is required");
    }

    #[tokio::test]
    async fn create_returns_internal_error_when_registry_persist_fails() {
        let (_temp_dir, state) = test_state();
        let experiments_dir = state.data_dir.join("experiments");
        std::fs::remove_dir_all(&experiments_dir).expect("remove experiments dir");
        std::fs::write(&experiments_dir, "blocked").expect("create blocking file");

        let request = CreateExperimentRequest {
            name: Some("Prompt tournament".to_string()),
            kind: Some("proof_of_fitness".to_string()),
            config: None,
        };

        let error = handle_create_experiment(State(state), Json(request))
            .await
            .expect_err("internal error");
        assert_eq!(error.0, StatusCode::INTERNAL_SERVER_ERROR);
        assert!(error
            .1
             .0
            .error
            .starts_with("failed to persist experiments:"));
    }

    #[tokio::test]
    async fn get_returns_experiment_detail() {
        let (_temp_dir, state) = test_state();
        let id = {
            let mut registry = state.experiment_registry.lock().await;
            registry
                .create(
                    "Analysis pass".to_string(),
                    ExperimentKind::AnalysisOnly,
                    ExperimentConfig::default(),
                )
                .expect("create experiment")
                .id
        };

        let Json(detail) = handle_get_experiment(State(state), Path(id.clone()))
            .await
            .expect("detail response");
        assert_eq!(detail.id, id);
        assert_eq!(detail.name, "Analysis pass");
        assert_eq!(detail.kind, ExperimentKind::AnalysisOnly);
    }

    #[tokio::test]
    async fn results_return_empty_payload_for_existing_experiment() {
        let (_temp_dir, state) = test_state();
        let id = {
            let mut registry = state.experiment_registry.lock().await;
            registry
                .create(
                    "Tournament".to_string(),
                    ExperimentKind::Tournament,
                    ExperimentConfig::default(),
                )
                .expect("create experiment")
                .id
        };

        let Json(results) = handle_get_experiment_results(State(state), Path(id.clone()))
            .await
            .expect("results response");
        assert_eq!(results.id, id);
        assert_eq!(results.status, ExperimentStatus::Queued);
        assert_eq!(results.score_summary, "waiting to start");
        assert_eq!(results.plans_generated, 0);
        assert!(results.leaders.is_empty());
        assert!(results.tournament.is_none());
    }

    #[tokio::test]
    async fn results_return_recorded_payload_for_completed_experiment() {
        let (_temp_dir, state) = test_state();
        let id = {
            let mut registry = state.experiment_registry.lock().await;
            let experiment = registry
                .create(
                    "Tournament".to_string(),
                    ExperimentKind::Tournament,
                    ExperimentConfig::default(),
                )
                .expect("create experiment");
            registry.start(&experiment.id).expect("start");
            registry
                .complete(
                    &experiment.id,
                    ExperimentResult {
                        plans_generated: 2,
                        proposals_written: vec!["proposal-a.md".to_string()],
                        branches_created: vec!["feature/proposal-a".to_string()],
                        score_summary: Some("winner: proposal-a".to_string()),
                        skipped: vec![SkippedItem {
                            name: "candidate-a".to_string(),
                            reason: "not enough signal".to_string(),
                        }],
                    },
                )
                .expect("complete");
            experiment.id
        };

        let Json(results) = handle_get_experiment_results(State(state), Path(id))
            .await
            .expect("results response");
        assert_eq!(results.status, ExperimentStatus::Completed);
        assert_eq!(results.score_summary, "winner: proposal-a");
        assert_eq!(results.plans_generated, 2);
        assert_eq!(results.proposals_written, vec!["proposal-a.md".to_string()]);
        assert_eq!(
            results.branches_created,
            vec!["feature/proposal-a".to_string()]
        );
        assert_eq!(
            results.skipped,
            vec![SkippedItem {
                name: "candidate-a".to_string(),
                reason: "not enough signal".to_string(),
            }]
        );
    }

    #[tokio::test]
    async fn stop_returns_conflict_for_completed_experiment() {
        let (_temp_dir, state) = test_state();
        let id = {
            let mut registry = state.experiment_registry.lock().await;
            let experiment = registry
                .create(
                    "Completed".to_string(),
                    ExperimentKind::ProofOfFitness,
                    ExperimentConfig::default(),
                )
                .expect("create experiment");
            registry.start(&experiment.id).expect("start");
            registry
                .complete(
                    &experiment.id,
                    ExperimentResult {
                        plans_generated: 1,
                        proposals_written: vec!["proposal.md".to_string()],
                        branches_created: Vec::new(),
                        score_summary: None,
                        skipped: Vec::new(),
                    },
                )
                .expect("complete");
            experiment.id
        };

        let error = handle_stop_experiment(State(state), Path(id))
            .await
            .expect_err("conflict response");
        assert_eq!(error.0, StatusCode::CONFLICT);
        assert_eq!(
            error.1 .0.error,
            "experiment is not running (status: completed)"
        );
    }

    #[tokio::test]
    async fn get_returns_404_for_missing_id() {
        let (_temp, state) = test_state();
        let status = request_status(experiment_router(state), "GET", "/experiments/missing").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn results_returns_404_for_missing_id() {
        let (_temp, state) = test_state();
        let status = request_status(
            experiment_router(state),
            "GET",
            "/experiments/missing/results",
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn stop_returns_404_for_missing_id() {
        let (_temp, state) = test_state();
        let status = request_status(
            experiment_router(state),
            "POST",
            "/experiments/missing/stop",
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }
}
