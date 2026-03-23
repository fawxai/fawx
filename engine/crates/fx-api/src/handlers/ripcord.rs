use super::HandlerResult;
use crate::state::HttpState;
use crate::types::ErrorBody;
use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use fx_ripcord::{approve_ripcord, pull_ripcord, RipcordJournal, RipcordReport, RipcordStatus};
use serde::Serialize;
use std::sync::Arc;

#[derive(Debug, Serialize)]
pub struct RipcordStatusResponse {
    pub active: bool,
    pub tripwire_id: Option<String>,
    pub tripwire_description: Option<String>,
    pub entry_count: u64,
}

/// GET /v1/ripcord/status
pub async fn handle_status(
    State(state): State<HttpState>,
) -> HandlerResult<Json<RipcordStatusResponse>> {
    let journal = require_ripcord(&state)?;
    let status = journal.status().await;
    Ok(Json(map_status(status)))
}

/// GET /v1/ripcord/journal
pub async fn handle_journal(
    State(state): State<HttpState>,
) -> HandlerResult<Json<serde_json::Value>> {
    let journal = require_ripcord(&state)?;
    Ok(Json(journal_response(&journal).await))
}

/// POST /v1/ripcord/pull
pub async fn handle_pull(State(state): State<HttpState>) -> HandlerResult<Json<RipcordReport>> {
    let journal = require_ripcord(&state)?;
    Ok(Json(pull_ripcord(&journal).await))
}

/// POST /v1/ripcord/approve
pub async fn handle_approve(
    State(state): State<HttpState>,
) -> HandlerResult<Json<serde_json::Value>> {
    let journal = require_ripcord(&state)?;
    approve_ripcord(&journal).await;
    Ok(Json(serde_json::json!({ "cleared": true })))
}

fn require_ripcord(
    state: &HttpState,
) -> Result<Arc<RipcordJournal>, (StatusCode, Json<ErrorBody>)> {
    state.ripcord.clone().ok_or_else(ripcord_unavailable)
}

fn ripcord_unavailable() -> (StatusCode, Json<ErrorBody>) {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(ErrorBody {
            error: "Ripcord not available".to_string(),
        }),
    )
}

fn map_status(status: RipcordStatus) -> RipcordStatusResponse {
    RipcordStatusResponse {
        active: status.active,
        tripwire_id: status.tripwire_id,
        tripwire_description: status.tripwire_description,
        entry_count: status.entry_count,
    }
}

async fn journal_response(journal: &Arc<RipcordJournal>) -> serde_json::Value {
    let entries = journal.entries().await;
    serde_json::json!({ "entries": entries })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build_router;
    use crate::devices::DeviceStore;
    use crate::engine::{AppEngine, CycleResult, ResultKind};
    use crate::pairing::PairingState;
    use crate::server_runtime::ServerRuntime;
    use crate::state::{build_channel_runtime, in_memory_telemetry, SharedReadState};
    use crate::types::{
        AuthProviderDto, ContextInfoDto, ErrorRecordDto, ModelInfoDto, ModelSwitchDto,
        SkillSummaryDto, ThinkingLevelDto,
    };
    use async_trait::async_trait;
    use axum::body::Body;
    use fx_core::types::InputSource;
    use fx_llm::{DocumentAttachment, ImageAttachment, Message};
    use http_body_util::BodyExt;
    use hyper::Request;
    use std::sync::Arc;
    use std::time::Instant;
    use tempfile::TempDir;
    use tokio::sync::Mutex;
    use tower::ServiceExt;

    const TEST_TOKEN: &str = "test-token";

    struct TestApp;

    #[async_trait]
    impl AppEngine for TestApp {
        async fn process_message(
            &mut self,
            _input: &str,
            _images: Vec<ImageAttachment>,
            _documents: Vec<DocumentAttachment>,
            _source: InputSource,
            _callback: Option<fx_kernel::StreamCallback>,
        ) -> Result<CycleResult, anyhow::Error> {
            Ok(CycleResult {
                response: "ok".to_string(),
                model: self.active_model().to_string(),
                iterations: 0,
                result_kind: ResultKind::Complete,
            })
        }

        async fn process_message_with_context(
            &mut self,
            _input: &str,
            _images: Vec<ImageAttachment>,
            _documents: Vec<DocumentAttachment>,
            context: Vec<Message>,
            _source: InputSource,
            _callback: Option<fx_kernel::StreamCallback>,
        ) -> Result<(CycleResult, Vec<Message>), anyhow::Error> {
            Ok((
                CycleResult {
                    response: "ok".to_string(),
                    model: self.active_model().to_string(),
                    iterations: 0,
                    result_kind: ResultKind::Complete,
                },
                context,
            ))
        }

        fn active_model(&self) -> &str {
            "test-model"
        }
        fn available_models(&self) -> Vec<ModelInfoDto> {
            Vec::new()
        }
        fn set_active_model(&mut self, _selector: &str) -> Result<ModelSwitchDto, anyhow::Error> {
            anyhow::bail!("unused")
        }
        fn thinking_level(&self) -> ThinkingLevelDto {
            ThinkingLevelDto {
                level: "off".to_string(),
                budget_tokens: None,
                available: vec!["off".to_string()],
            }
        }
        fn context_info(&self) -> ContextInfoDto {
            ContextInfoDto {
                used_tokens: 0,
                max_tokens: 0,
                percentage: 0.0,
                compaction_threshold: 0.8,
            }
        }
        fn context_info_for_messages(&self, _messages: &[Message]) -> ContextInfoDto {
            self.context_info()
        }
        fn set_thinking_level(&mut self, _level: &str) -> Result<ThinkingLevelDto, anyhow::Error> {
            Ok(self.thinking_level())
        }
        fn skill_summaries(&self) -> Vec<SkillSummaryDto> {
            Vec::new()
        }
        fn auth_provider_statuses(&self) -> Vec<AuthProviderDto> {
            Vec::new()
        }
        fn config_manager(&self) -> Option<crate::engine::ConfigManagerHandle> {
            None
        }
        fn session_bus(&self) -> Option<&fx_bus::SessionBus> {
            None
        }
        fn recent_errors(&self, _limit: usize) -> Vec<ErrorRecordDto> {
            Vec::new()
        }
    }

    fn ripcord_router() -> axum::Router {
        let temp_dir = TempDir::new().expect("tempdir");
        let journal = Arc::new(RipcordJournal::new(temp_dir.path()));
        let app = TestApp;
        let state = HttpState {
            app: Arc::new(Mutex::new(app)),
            shared: Arc::new(SharedReadState::from_app(&TestApp)),
            config_manager: None,
            session_registry: None,
            start_time: Instant::now(),
            server_runtime: ServerRuntime::local(8400),
            tailscale_ip: None,
            bearer_token: TEST_TOKEN.to_string(),
            pairing: Arc::new(Mutex::new(PairingState::new())),
            devices: Arc::new(Mutex::new(DeviceStore::new())),
            devices_path: None,
            channels: build_channel_runtime(None, vec![]),
            data_dir: std::env::temp_dir(),
            synthesis: Arc::new(crate::handlers::synthesis::SynthesisState::new(false)),
            oauth_flows: Arc::new(crate::handlers::oauth::OAuthFlowStore::new()),
            permission_prompts: Arc::new(fx_kernel::PermissionPromptState::new()),
            ripcord: Some(journal),
            fleet_manager: None,
            cron_store: None,
            experiment_registry: {
                let registry = crate::experiment_registry::ExperimentRegistry::new(
                    std::env::temp_dir().as_path(),
                )
                .expect("registry");
                Arc::new(tokio::sync::Mutex::new(registry))
            },
            improvement_provider: None,
            telemetry: in_memory_telemetry(),
        };
        build_router(state, None)
    }

    async fn response_json(response: axum::response::Response) -> serde_json::Value {
        let body = response
            .into_body()
            .collect()
            .await
            .expect("body")
            .to_bytes();
        serde_json::from_slice(&body).expect("json")
    }

    fn authed_request(method: &str, uri: &str) -> Request<Body> {
        Request::builder()
            .method(method)
            .uri(uri)
            .header("authorization", format!("Bearer {TEST_TOKEN}"))
            .body(Body::empty())
            .expect("request")
    }

    #[tokio::test]
    async fn status_returns_inactive_by_default() {
        let response = ripcord_router()
            .oneshot(authed_request("GET", "/v1/ripcord/status"))
            .await
            .expect("response");

        let json = response_json(response).await;
        assert_eq!(json["active"], false);
        assert_eq!(json["entry_count"], 0);
    }

    #[tokio::test]
    async fn journal_returns_empty_entries() {
        let response = ripcord_router()
            .oneshot(authed_request("GET", "/v1/ripcord/journal"))
            .await
            .expect("response");

        let json = response_json(response).await;
        assert_eq!(json["entries"], serde_json::json!([]));
    }

    #[tokio::test]
    async fn pull_returns_empty_report() {
        let response = ripcord_router()
            .oneshot(authed_request("POST", "/v1/ripcord/pull"))
            .await
            .expect("response");

        let json = response_json(response).await;
        assert_eq!(json["total"], 0);
        assert_eq!(json["reverted"], serde_json::json!([]));
        assert_eq!(json["skipped"], serde_json::json!([]));
    }

    #[tokio::test]
    async fn approve_returns_cleared() {
        let response = ripcord_router()
            .oneshot(authed_request("POST", "/v1/ripcord/approve"))
            .await
            .expect("response");

        let json = response_json(response).await;
        assert_eq!(json["cleared"], true);
    }
}
