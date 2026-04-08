use super::super::super::preflight_route::{
    build_route_plan, detect_route_resource, RouteFamily, RouteResource,
};
use super::super::test_fixtures::RecordingLlm;
use super::*;
use crate::act::ToolResult;
use crate::signals::SignalKind;
use async_trait::async_trait;
use fx_core::runtime_info::{ConfigSummary, RuntimeInfo, SkillInfo};
use fx_core::tool_routing::{
    ArtifactStrategy, ResourceKind, RouteAuthMode, RouteOperation, ToolReadinessSummary,
    ToolRoutingMetadata, ToolRoutingSummary,
};
use fx_llm::{CompletionRequest, CompletionResponse, ProviderError, ToolCall, ToolDefinition};
use std::sync::{Arc, Mutex, RwLock};

#[derive(Debug)]
struct PreflightObservationExecutor {
    tools: Vec<ToolDefinition>,
}

impl PreflightObservationExecutor {
    fn new(tool_names: &[&str]) -> Self {
        Self {
            tools: tool_names
                .iter()
                .map(|tool_name| ToolDefinition {
                    name: (*tool_name).to_string(),
                    description: format!("{tool_name} test tool"),
                    parameters: serde_json::json!({"type":"object"}),
                })
                .collect(),
        }
    }
}

#[async_trait]
impl ToolExecutor for PreflightObservationExecutor {
    async fn execute_tools(
        &self,
        calls: &[ToolCall],
        _cancel: Option<&CancellationToken>,
    ) -> Result<Vec<ToolResult>, crate::act::ToolExecutorError> {
        Ok(calls
            .iter()
            .map(|call| ToolResult::success(call.id.clone(), call.name.clone(), "ok"))
            .collect())
    }

    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        self.tools.clone()
    }
}

#[derive(Debug, Default)]
struct FirstAdvertisedToolLlm {
    requests: Mutex<Vec<CompletionRequest>>,
}

impl FirstAdvertisedToolLlm {
    fn requests(&self) -> Vec<CompletionRequest> {
        self.requests.lock().expect("lock").clone()
    }
}

#[async_trait]
impl LlmProvider for FirstAdvertisedToolLlm {
    async fn generate(&self, _: &str, _: u32) -> Result<String, fx_core::error::LlmError> {
        Ok("summary".to_string())
    }

    async fn generate_streaming(
        &self,
        _: &str,
        _: u32,
        callback: Box<dyn Fn(String) + Send + 'static>,
    ) -> Result<String, fx_core::error::LlmError> {
        callback("summary".to_string());
        Ok("summary".to_string())
    }

    fn model_name(&self) -> &str {
        "first-advertised"
    }

    async fn complete(
        &self,
        request: CompletionRequest,
    ) -> Result<CompletionResponse, ProviderError> {
        self.requests.lock().expect("lock").push(request.clone());
        let selected = request
            .tools
            .first()
            .expect("request should advertise at least one tool");
        Ok(tool_use_response(vec![ToolCall {
            id: "call-1".to_string(),
            name: selected.name.clone(),
            arguments: serde_json::json!({}),
        }]))
    }
}

fn routing_summary(
    tool_name: &str,
    resource_kinds: Vec<ResourceKind>,
    operations: Vec<RouteOperation>,
    auth_mode: RouteAuthMode,
    artifact_strategy: ArtifactStrategy,
    fallback_rank: u16,
    ready: bool,
) -> ToolRoutingSummary {
    ToolRoutingSummary {
        tool_name: tool_name.to_string(),
        metadata: ToolRoutingMetadata {
            resource_kinds,
            operations,
            auth_mode,
            artifact_strategy,
            fallback_rank,
        },
        readiness: ToolReadinessSummary {
            available: true,
            ready,
            readiness_reason: (!ready).then(|| "not ready".to_string()),
        },
    }
}

fn runtime_info_with_routing_tools(
    routing_tools: Vec<ToolRoutingSummary>,
) -> Arc<RwLock<RuntimeInfo>> {
    Arc::new(RwLock::new(RuntimeInfo {
        active_model: "test-model".to_string(),
        provider: "test-provider".to_string(),
        skills: vec![SkillInfo {
            name: "routing-skill".to_string(),
            description: Some("typed routing metadata".to_string()),
            tool_names: routing_tools
                .iter()
                .map(|tool| tool.tool_name.clone())
                .collect(),
            routing_tools,
            capabilities: Vec::new(),
            version: None,
            source: None,
            revision_hash: None,
            manifest_hash: None,
            activated_at_ms: None,
            signature_status: None,
            stale_source: None,
        }],
        config_summary: ConfigSummary {
            max_iterations: 3,
            max_history: 20,
            memory_enabled: false,
        },
        authority: None,
        version: "test".to_string(),
    }))
}

fn engine_with_preflight_routing(
    tool_names: &[&str],
    routing_tools: Vec<ToolRoutingSummary>,
) -> LoopEngine {
    let mut engine = mixed_tool_engine_with_executor(
        BudgetConfig::default(),
        Arc::new(PreflightObservationExecutor::new(tool_names)),
    );
    engine.set_runtime_info(runtime_info_with_routing_tools(routing_tools));
    engine
}

fn github_routing_tools() -> Vec<ToolRoutingSummary> {
    vec![
        routing_summary(
            "view_pr",
            vec![ResourceKind::GitHubPullRequest],
            vec![RouteOperation::Fetch],
            RouteAuthMode::CredentialRequired {
                key: "github_token".to_string(),
            },
            ArtifactStrategy::ProbeFirst,
            10,
            true,
        ),
        routing_summary(
            "list_pr_files",
            vec![ResourceKind::GitHubPullRequest],
            vec![RouteOperation::List],
            RouteAuthMode::CredentialRequired {
                key: "github_token".to_string(),
            },
            ArtifactStrategy::ProbeFirst,
            10,
            true,
        ),
        routing_summary(
            "view_pr_file_patch",
            vec![ResourceKind::GitHubPullRequest],
            vec![RouteOperation::Fetch],
            RouteAuthMode::CredentialRequired {
                key: "github_token".to_string(),
            },
            ArtifactStrategy::DirectFetch,
            20,
            true,
        ),
        routing_summary(
            "web_fetch",
            vec![ResourceKind::GenericUrl],
            vec![RouteOperation::Fetch],
            RouteAuthMode::None,
            ArtifactStrategy::DirectFetch,
            100,
            true,
        ),
    ]
}

#[test]
fn route_planner_prefers_github_routes_over_public_web_fallback() {
    let resource = detect_route_resource("Review https://github.com/fawxai/fawx/pull/1753")
        .expect("github pr resource");
    let tools = [
        "view_pr",
        "list_pr_files",
        "view_pr_file_patch",
        "web_fetch",
    ]
    .into_iter()
    .map(|tool_name| ToolDefinition {
        name: tool_name.to_string(),
        description: format!("{tool_name} tool"),
        parameters: serde_json::json!({"type":"object"}),
    })
    .collect::<Vec<_>>();

    let plan = build_route_plan(&resource, &tools, &github_routing_tools())
        .expect("github route plan should exist");

    assert_eq!(plan.resource, resource);
    assert_eq!(plan.primary_route.family, RouteFamily::GitHub);
    assert!(plan.requires_probe);
    assert_eq!(
        plan.primary_route.tool_names,
        vec!["list_pr_files".to_string(), "view_pr".to_string()]
    );
    assert_eq!(plan.fallback_routes[0].family, RouteFamily::GitHub);
    assert_eq!(
        plan.fallback_routes[0].tool_names,
        vec!["view_pr_file_patch".to_string()]
    );
    assert_eq!(plan.fallback_routes[1].family, RouteFamily::PublicWeb);
}

#[tokio::test]
async fn github_pr_reasoning_surface_avoids_web_fetch_on_first_route() {
    let mut engine = engine_with_preflight_routing(
        &[
            "view_pr",
            "list_pr_files",
            "view_pr_file_patch",
            "web_fetch",
        ],
        github_routing_tools(),
    );
    let llm = RecordingLlm::ok(vec![text_response("done")]);
    let processed = engine
        .perceive(&test_snapshot(
            "Review this PR: https://github.com/fawxai/fawx/pull/1753",
        ))
        .await
        .expect("perceive");

    assert!(engine.preflight_route_plan.is_some());

    let _ = engine
        .reason(&processed, &llm, CycleStream::disabled())
        .await
        .expect("reason");

    let requests = llm.requests();
    assert_eq!(requests.len(), 1);
    let tool_names: Vec<_> = requests[0]
        .tools
        .iter()
        .map(|tool| tool.name.as_str())
        .collect();
    assert_eq!(tool_names, vec!["list_pr_files", "view_pr"]);
    assert!(
        !tool_names.contains(&"web_fetch"),
        "github PR first route should not advertise web_fetch"
    );
    assert!(
        !tool_names.contains(&DECOMPOSE_TOOL_NAME),
        "preflight route should disable broad decompose before the first tool round"
    );
}

#[tokio::test]
async fn large_github_pr_review_chooses_probe_capable_path_first() {
    let mut engine = engine_with_preflight_routing(
        &[
            "view_pr",
            "list_pr_files",
            "view_pr_file_patch",
            "web_fetch",
        ],
        github_routing_tools(),
    );
    let llm = FirstAdvertisedToolLlm::default();
    let processed = engine
        .perceive(&test_snapshot(
            "Review the large PR at https://github.com/fawxai/fawx/pull/1753 and call the best first tool.",
        ))
        .await
        .expect("perceive");

    let response = engine
        .reason(&processed, &llm, CycleStream::disabled())
        .await
        .expect("reason");

    let first_call = response.tool_calls.first().expect("first tool call");
    assert!(
        matches!(first_call.name.as_str(), "list_pr_files" | "view_pr"),
        "expected a probe-capable GitHub tool first, got {}",
        first_call.name
    );
    let requests = llm.requests();
    let advertised_tools: Vec<_> = requests[0]
        .tools
        .iter()
        .map(|tool| tool.name.as_str())
        .collect();
    assert_eq!(advertised_tools, vec!["list_pr_files", "view_pr"]);
}

#[tokio::test]
async fn generic_url_without_better_route_uses_public_web_fetch() {
    let mut engine = engine_with_preflight_routing(
        &["web_fetch", "web_screenshot"],
        vec![
            routing_summary(
                "web_fetch",
                vec![ResourceKind::GenericUrl],
                vec![RouteOperation::Fetch],
                RouteAuthMode::None,
                ArtifactStrategy::DirectFetch,
                100,
                true,
            ),
            routing_summary(
                "web_screenshot",
                vec![ResourceKind::GenericUrl],
                vec![RouteOperation::Fetch],
                RouteAuthMode::None,
                ArtifactStrategy::ProbeFirst,
                100,
                true,
            ),
        ],
    );
    let llm = RecordingLlm::ok(vec![text_response("done")]);
    let processed = engine
        .perceive(&test_snapshot(
            "Summarize https://example.com/guide for me.",
        ))
        .await
        .expect("perceive");

    let plan = engine
        .preflight_route_plan
        .clone()
        .expect("generic url plan should exist");
    assert_eq!(
        plan.resource,
        RouteResource::GenericUrl {
            url: "https://example.com/guide".to_string(),
        }
    );
    assert_eq!(plan.primary_route.family, RouteFamily::PublicWeb);
    assert_eq!(plan.primary_route.tool_names, vec!["web_fetch".to_string()]);
    assert!(!plan.requires_probe);

    let _ = engine
        .reason(&processed, &llm, CycleStream::disabled())
        .await
        .expect("reason");
    let requests = llm.requests();
    let tool_names: Vec<_> = requests[0]
        .tools
        .iter()
        .map(|tool| tool.name.as_str())
        .collect();
    assert_eq!(tool_names, vec!["web_fetch"]);
}

#[tokio::test]
async fn preflight_route_choice_is_visible_in_trace_signals() {
    let mut engine = engine_with_preflight_routing(
        &[
            "view_pr",
            "list_pr_files",
            "view_pr_file_patch",
            "web_fetch",
        ],
        github_routing_tools(),
    );
    let _ = engine
        .perceive(&test_snapshot(
            "Inspect https://github.com/fawxai/fawx/pull/1753 before reviewing it.",
        ))
        .await
        .expect("perceive");

    let route_signal = engine
        .signals
        .signals()
        .iter()
        .find(|signal| {
            signal.kind == SignalKind::Trace
                && signal.message == "planned preflight external resource route"
        })
        .expect("route trace signal");

    assert_eq!(
        route_signal.metadata["resource"]["kind"],
        "github_pull_request"
    );
    assert_eq!(route_signal.metadata["primary_route"]["family"], "github");
    assert_eq!(
        route_signal.metadata["primary_route"]["artifact_strategy"],
        "probe_first"
    );
    assert_eq!(route_signal.metadata["requires_probe"], true);
}
