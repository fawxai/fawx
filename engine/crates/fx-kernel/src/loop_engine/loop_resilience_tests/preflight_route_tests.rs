use super::super::super::preflight_route::{
    build_route_plan, detect_route_resource, PlannedRoute, RouteFamily, RoutePlan,
    RouteRankingBasis, RouteResource,
};
use super::super::test_fixtures::{RecordingLlm, ScriptedRouteExecutor, ScriptedToolOutcome};
use super::*;
use crate::act::{HttpDiagnostics, ToolExecutionDiagnostics, ToolResult};
use crate::signals::SignalKind;
use crate::FailureClass;
use async_trait::async_trait;
use fx_core::runtime_info::{ConfigSummary, RuntimeInfo, SkillInfo};
use fx_core::tool_routing::{
    ArtifactStrategy, ResourceKind, RouteAdvisory, RouteAdvisoryOutcome, RouteAdvisorySource,
    RouteAuthMode, RouteOperation, ToolReadinessSummary, ToolRoutingMetadata, ToolRoutingSummary,
};
use fx_llm::{CompletionRequest, CompletionResponse, ProviderError, ToolCall, ToolDefinition};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, RwLock};

const TEST_ADVISORY_TIMESTAMP_MS: u64 = 1_700_000_000_000;

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

#[derive(Clone)]
struct RoutingSummarySpec {
    resource_kinds: Vec<ResourceKind>,
    operations: Vec<RouteOperation>,
    auth_mode: RouteAuthMode,
    artifact_strategy: ArtifactStrategy,
    fallback_rank: u16,
    available: bool,
    ready: bool,
}

impl Default for RoutingSummarySpec {
    fn default() -> Self {
        Self {
            resource_kinds: vec![ResourceKind::GenericUrl],
            operations: vec![RouteOperation::Fetch],
            auth_mode: RouteAuthMode::None,
            artifact_strategy: ArtifactStrategy::DirectFetch,
            fallback_rank: 100,
            available: true,
            ready: true,
        }
    }
}

impl RoutingSummarySpec {
    fn resource(mut self, resource_kind: ResourceKind) -> Self {
        self.resource_kinds = vec![resource_kind];
        self
    }

    fn operations<I>(mut self, operations: I) -> Self
    where
        I: IntoIterator<Item = RouteOperation>,
    {
        self.operations = operations.into_iter().collect();
        self
    }

    fn credential_required(mut self, key: &str) -> Self {
        self.auth_mode = RouteAuthMode::CredentialRequired {
            key: key.to_string(),
        };
        self
    }

    fn artifact_strategy(mut self, artifact_strategy: ArtifactStrategy) -> Self {
        self.artifact_strategy = artifact_strategy;
        self
    }

    fn fallback_rank(mut self, fallback_rank: u16) -> Self {
        self.fallback_rank = fallback_rank;
        self
    }

    fn available(mut self, available: bool) -> Self {
        self.available = available;
        self
    }

    fn ready(mut self, ready: bool) -> Self {
        self.ready = ready;
        self
    }
}

fn routing_summary(tool_name: &str, spec: RoutingSummarySpec) -> ToolRoutingSummary {
    ToolRoutingSummary {
        tool_name: tool_name.to_string(),
        metadata: ToolRoutingMetadata {
            resource_kinds: spec.resource_kinds,
            operations: spec.operations,
            auth_mode: spec.auth_mode,
            artifact_strategy: spec.artifact_strategy,
            fallback_rank: spec.fallback_rank,
        },
        readiness: ToolReadinessSummary {
            available: spec.available,
            ready: spec.ready,
            readiness_reason: (!spec.ready).then(|| "not ready".to_string()),
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
            tool_invocations_remaining: 0,
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
            RoutingSummarySpec::default()
                .resource(ResourceKind::GitHubPullRequest)
                .credential_required("github_token")
                .artifact_strategy(ArtifactStrategy::ProbeFirst)
                .fallback_rank(10),
        ),
        routing_summary(
            "list_pr_files",
            RoutingSummarySpec::default()
                .resource(ResourceKind::GitHubPullRequest)
                .operations([RouteOperation::List])
                .credential_required("github_token")
                .artifact_strategy(ArtifactStrategy::ProbeFirst)
                .fallback_rank(10),
        ),
        routing_summary(
            "view_pr_file_patch",
            RoutingSummarySpec::default()
                .resource(ResourceKind::GitHubPullRequest)
                .credential_required("github_token")
                .artifact_strategy(ArtifactStrategy::DirectFetch)
                .fallback_rank(20),
        ),
        routing_summary("web_fetch", RoutingSummarySpec::default()),
    ]
}

fn github_resource() -> RouteResource {
    detect_route_resource("Review https://github.com/fawxai/fawx/pull/1753")
        .expect("github resource")
}

fn planned_route(
    family: RouteFamily,
    tool_names: &[&str],
    authenticated: bool,
    artifact_strategy: ArtifactStrategy,
    fallback_rank: u16,
) -> PlannedRoute {
    PlannedRoute {
        family,
        tool_names: tool_names.iter().map(|tool| (*tool).to_string()).collect(),
        reason: "test route".to_string(),
        ranking_basis: RouteRankingBasis::TypedPolicyOnly,
        advisory_influence: None,
        authenticated,
        artifact_strategy,
        fallback_rank,
    }
}

fn route_advisory(
    resource_kind: ResourceKind,
    tool_name: &str,
    outcome: RouteAdvisoryOutcome,
) -> RouteAdvisory {
    RouteAdvisory {
        resource_kind,
        tool_name: Some(tool_name.to_string()),
        outcome,
        source: RouteAdvisorySource::Journal,
        note: format!("{tool_name} {outcome:?}"),
        observed_at_ms: TEST_ADVISORY_TIMESTAMP_MS,
    }
}

fn reroute_test_http_diagnostics(status_code: u16) -> ToolExecutionDiagnostics {
    let mut headers = BTreeMap::new();
    headers.insert("content-type".to_string(), "application/json".to_string());
    ToolExecutionDiagnostics::Http(HttpDiagnostics {
        status_code: Some(status_code),
        headers,
        transport_error: None,
        body_snippet: Some(r#"{"message":"structured failure"}"#.to_string()),
        body_truncated: false,
        binary_body: false,
    })
}

#[test]
fn route_planner_prefers_github_routes_over_public_web_fallback() {
    let resource = detect_route_resource("Review https://github.com/fawxai/fawx/pull/1753/files")
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

    let plan = build_route_plan(&resource, &tools, &github_routing_tools(), &[])
        .expect("github route plan should exist");

    assert_eq!(plan.resource, resource);
    assert_eq!(plan.primary_route.family, RouteFamily::GitHub);
    assert!(plan.requires_probe);
    assert_eq!(
        plan.primary_route.ranking_basis,
        RouteRankingBasis::TypedPolicyOnly
    );
    assert!(plan.primary_route.advisory_influence.is_none());
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

#[test]
fn route_planner_handles_github_issue_and_repo_urls_when_typed_metadata_exists() {
    let tools = ["list_prs", "list_issues", "web_fetch"]
        .into_iter()
        .map(|tool_name| ToolDefinition {
            name: tool_name.to_string(),
            description: format!("{tool_name} tool"),
            parameters: serde_json::json!({"type":"object"}),
        })
        .collect::<Vec<_>>();
    let routing_tools = vec![
        routing_summary(
            "list_prs",
            RoutingSummarySpec::default()
                .resource(ResourceKind::GitHubRepository)
                .operations([RouteOperation::List])
                .credential_required("github_token")
                .fallback_rank(10),
        ),
        routing_summary(
            "list_issues",
            RoutingSummarySpec::default()
                .resource(ResourceKind::GitHubIssue)
                .operations([RouteOperation::List])
                .credential_required("github_token")
                .fallback_rank(10),
        ),
        routing_summary("web_fetch", RoutingSummarySpec::default()),
    ];

    let issue = detect_route_resource("Inspect https://github.com/fawxai/fawx/issues/1785")
        .expect("github issue resource");
    let issue_plan = build_route_plan(&issue, &tools, &routing_tools, &[])
        .expect("github issue route plan should exist");
    assert_eq!(issue_plan.primary_route.family, RouteFamily::GitHub);
    assert_eq!(
        issue_plan.primary_route.tool_names,
        vec!["list_issues".to_string()]
    );

    let repo = detect_route_resource("Inspect https://github.com/fawxai/fawx")
        .expect("github repo resource");
    let repo_plan =
        build_route_plan(&repo, &tools, &routing_tools, &[]).expect("repo route plan should exist");
    assert_eq!(repo_plan.primary_route.family, RouteFamily::GitHub);
    assert_eq!(
        repo_plan.primary_route.tool_names,
        vec!["list_prs".to_string()]
    );
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
async fn missing_typed_route_uses_degraded_public_web_fallback_without_broad_surface() {
    let mut engine = engine_with_preflight_routing(
        &["read_file", "run_command", "memory_read", "web_fetch"],
        Vec::new(),
    );
    let llm = RecordingLlm::ok(vec![text_response("done")]);
    let processed = engine
        .perceive(&test_snapshot(
            "Review this PR: https://github.com/fawxai/fawx/pull/1753",
        ))
        .await
        .expect("perceive");

    let plan = engine
        .preflight_route_plan
        .clone()
        .expect("degraded public-web fallback plan should exist");
    assert_eq!(plan.primary_route.family, RouteFamily::PublicWeb);
    assert_eq!(
        plan.primary_route.ranking_basis,
        RouteRankingBasis::DegradedFallback
    );
    assert_eq!(plan.primary_route.tool_names, vec!["web_fetch".to_string()]);

    let signals = engine.signals.signals();
    let no_route_signal = signals
        .iter()
        .find(|signal| {
            signal.kind == SignalKind::Trace
                && signal.message == "resource-bearing request has no ready typed preflight route"
        })
        .expect("no-route trace signal");
    assert_eq!(no_route_signal.metadata["routing_tool_count"], 0);
    assert_eq!(no_route_signal.metadata["fallback_mode"], "public_web");
    assert_eq!(
        no_route_signal.metadata["fallback_route"]["ranking_basis"],
        "degraded_fallback"
    );
    assert_eq!(
        no_route_signal.metadata["fallback_route"]["tool_names"][0],
        "web_fetch"
    );
    assert_eq!(no_route_signal.metadata["decision_kind"], "preflight_route");
    assert_eq!(no_route_signal.metadata["decision"], "no_ready_typed_route");

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
async fn unready_typed_routes_use_degraded_public_web_fallback_without_broad_surface() {
    let mut engine = engine_with_preflight_routing(
        &["view_pr", "list_pr_files", "read_file", "web_fetch"],
        vec![
            routing_summary(
                "view_pr",
                RoutingSummarySpec::default()
                    .resource(ResourceKind::GitHubPullRequest)
                    .credential_required("github_token")
                    .artifact_strategy(ArtifactStrategy::ProbeFirst)
                    .ready(false),
            ),
            routing_summary(
                "list_pr_files",
                RoutingSummarySpec::default()
                    .resource(ResourceKind::GitHubPullRequest)
                    .operations([RouteOperation::List])
                    .credential_required("github_token")
                    .artifact_strategy(ArtifactStrategy::ProbeFirst)
                    .available(false),
            ),
        ],
    );
    let llm = RecordingLlm::ok(vec![text_response("done")]);
    let processed = engine
        .perceive(&test_snapshot(
            "Review this PR: https://github.com/fawxai/fawx/pull/1753",
        ))
        .await
        .expect("perceive");

    let plan = engine
        .preflight_route_plan
        .clone()
        .expect("degraded public-web fallback plan should exist");
    assert_eq!(plan.primary_route.family, RouteFamily::PublicWeb);
    assert_eq!(
        plan.primary_route.ranking_basis,
        RouteRankingBasis::DegradedFallback
    );
    assert_eq!(plan.primary_route.tool_names, vec!["web_fetch".to_string()]);

    let signals = engine.signals.signals();
    let no_route_signal = signals
        .iter()
        .find(|signal| {
            signal.kind == SignalKind::Trace
                && signal.message == "resource-bearing request has no ready typed preflight route"
        })
        .expect("no-route trace signal");
    assert_eq!(no_route_signal.metadata["routing_tool_count"], 2);
    assert_eq!(
        no_route_signal.metadata["typed_route_tools"],
        serde_json::json!(["view_pr", "list_pr_files"])
    );
    assert_eq!(
        no_route_signal.metadata["ready_typed_route_tools"],
        serde_json::json!([])
    );
    assert_eq!(no_route_signal.metadata["fallback_mode"], "public_web");
    let unready_typed_route_tools = no_route_signal.metadata["unready_typed_route_tools"]
        .as_array()
        .expect("unready typed route tools");
    assert_eq!(unready_typed_route_tools.len(), 2);
    assert_eq!(unready_typed_route_tools[0]["tool_name"], "view_pr");
    assert_eq!(unready_typed_route_tools[1]["tool_name"], "list_pr_files");

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
async fn missing_typed_route_without_public_web_fallback_stays_unconstrained_but_traced() {
    let mut engine =
        engine_with_preflight_routing(&["read_file", "run_command", "memory_read"], Vec::new());
    let _ = engine
        .perceive(&test_snapshot(
            "Review this PR: https://github.com/fawxai/fawx/pull/1753",
        ))
        .await
        .expect("perceive");

    assert!(
        engine.preflight_route_plan.is_none(),
        "without typed routes or public-web fallback, the loop should preserve current behavior"
    );
    let signals = engine.signals.signals();
    let no_route_signal = signals
        .iter()
        .find(|signal| {
            signal.kind == SignalKind::Trace
                && signal.message == "resource-bearing request has no ready typed preflight route"
        })
        .expect("no-route trace signal");
    assert_eq!(no_route_signal.metadata["routing_tool_count"], 0);
    assert_eq!(no_route_signal.metadata["fallback_mode"], "unconstrained");
    assert!(no_route_signal.metadata["fallback_route"].is_null());
    assert_eq!(no_route_signal.metadata["decision_kind"], "preflight_route");
    assert_eq!(no_route_signal.metadata["decision"], "no_ready_typed_route");

    let tool_names: Vec<_> = engine
        .current_reasoning_tool_definitions(false)
        .into_iter()
        .map(|tool| tool.name)
        .collect();
    assert_eq!(tool_names, vec!["read_file", "run_command", "memory_read"]);
}

#[tokio::test]
async fn non_resource_request_keeps_normal_tool_surface() {
    let mut engine = engine_with_preflight_routing(
        &["read_file", "run_command", "web_fetch"],
        vec![routing_summary("web_fetch", RoutingSummarySpec::default())],
    );
    let _ = engine
        .perceive(&test_snapshot("Summarize the project state."))
        .await
        .expect("perceive");

    assert!(engine.preflight_route_plan.is_none());
    let signals = engine.signals.signals();
    assert!(!signals.iter().any(|signal| {
        signal.message == "resource-bearing request has no ready typed preflight route"
    }));
    let tool_names: Vec<_> = engine
        .current_reasoning_tool_definitions(false)
        .into_iter()
        .map(|tool| tool.name)
        .collect();
    assert_eq!(tool_names, vec!["read_file", "run_command", "web_fetch"]);
}

#[test]
fn advisory_memory_reorders_github_probe_tools_without_changing_route_contract() {
    let resource = detect_route_resource("Review https://github.com/fawxai/fawx/pull/1753/files")
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
    let advisories = vec![
        route_advisory(
            ResourceKind::GitHubPullRequest,
            "view_pr",
            RouteAdvisoryOutcome::Prefer,
        ),
        route_advisory(
            ResourceKind::GitHubPullRequest,
            "list_pr_files",
            RouteAdvisoryOutcome::Avoid,
        ),
    ];

    let plan = build_route_plan(&resource, &tools, &github_routing_tools(), &advisories)
        .expect("github route plan should exist");

    assert_eq!(plan.primary_route.family, RouteFamily::GitHub);
    assert!(plan.requires_probe);
    assert_eq!(
        plan.primary_route.artifact_strategy,
        ArtifactStrategy::ProbeFirst
    );
    assert_eq!(
        plan.primary_route.tool_names,
        vec!["view_pr".to_string(), "list_pr_files".to_string()]
    );
    assert_eq!(
        plan.primary_route.ranking_basis,
        RouteRankingBasis::TypedPolicyPlusAdvisory
    );
    let influence = plan
        .primary_route
        .advisory_influence
        .as_ref()
        .expect("advisory influence");
    assert_eq!(influence.source, RouteAdvisorySource::Journal);
    assert_eq!(influence.preferred_tools, vec!["view_pr".to_string()]);
    assert_eq!(influence.avoided_tools, vec!["list_pr_files".to_string()]);
    assert_eq!(
        influence.summary,
        "journal advisories (2 matches) preferred view_pr and deprioritized list_pr_files"
    );
}

#[test]
fn advisory_memory_generalizes_to_generic_url_route_tie_breaks() {
    let resource =
        detect_route_resource("Summarize https://example.com/guide").expect("generic resource");
    let tools = ["web_fetch", "alt_web_fetch", "web_screenshot"]
        .into_iter()
        .map(|tool_name| ToolDefinition {
            name: tool_name.to_string(),
            description: format!("{tool_name} tool"),
            parameters: serde_json::json!({"type":"object"}),
        })
        .collect::<Vec<_>>();
    let routing_tools = vec![
        routing_summary("web_fetch", RoutingSummarySpec::default()),
        routing_summary("alt_web_fetch", RoutingSummarySpec::default()),
        routing_summary(
            "web_screenshot",
            RoutingSummarySpec::default().artifact_strategy(ArtifactStrategy::ProbeFirst),
        ),
    ];
    let advisories = vec![route_advisory(
        ResourceKind::GenericUrl,
        "alt_web_fetch",
        RouteAdvisoryOutcome::Prefer,
    )];

    let plan = build_route_plan(&resource, &tools, &routing_tools, &advisories)
        .expect("generic url plan should exist");

    assert_eq!(plan.primary_route.family, RouteFamily::PublicWeb);
    assert_eq!(
        plan.primary_route.tool_names,
        vec!["alt_web_fetch".to_string(), "web_fetch".to_string()]
    );
    assert_eq!(
        plan.primary_route.ranking_basis,
        RouteRankingBasis::TypedPolicyPlusAdvisory
    );
    assert!(!plan.requires_probe);
}

#[test]
fn neutral_advisory_preserves_tool_order_but_stays_traceable() {
    let resource = detect_route_resource("Review https://github.com/fawxai/fawx/pull/1753/files")
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
    let advisories = vec![route_advisory(
        ResourceKind::GitHubPullRequest,
        "view_pr",
        RouteAdvisoryOutcome::Neutral,
    )];

    let plan = build_route_plan(&resource, &tools, &github_routing_tools(), &advisories)
        .expect("github route plan should exist");

    assert_eq!(plan.primary_route.family, RouteFamily::GitHub);
    assert_eq!(
        plan.primary_route.tool_names,
        vec!["list_pr_files".to_string(), "view_pr".to_string()]
    );
    assert_eq!(
        plan.primary_route.ranking_basis,
        RouteRankingBasis::TypedPolicyPlusAdvisory
    );
    let influence = plan
        .primary_route
        .advisory_influence
        .as_ref()
        .expect("neutral advisory influence");
    assert_eq!(influence.matched_entries, 1);
    assert!(influence.preferred_tools.is_empty());
    assert!(influence.avoided_tools.is_empty());
    assert_eq!(
        influence.summary,
        "journal matched 1 advisory memories without changing tool rank"
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
            routing_summary("web_fetch", RoutingSummarySpec::default().ready(true)),
            routing_summary(
                "web_screenshot",
                RoutingSummarySpec::default().artifact_strategy(ArtifactStrategy::ProbeFirst),
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

#[test]
fn advisory_memory_cannot_make_unready_route_win() {
    let resource = detect_route_resource("Review https://github.com/fawxai/fawx/pull/1753")
        .expect("github pr resource");
    let tools = ["view_pr", "web_fetch"]
        .into_iter()
        .map(|tool_name| ToolDefinition {
            name: tool_name.to_string(),
            description: format!("{tool_name} tool"),
            parameters: serde_json::json!({"type":"object"}),
        })
        .collect::<Vec<_>>();
    let routing_tools = vec![
        routing_summary(
            "view_pr",
            RoutingSummarySpec::default()
                .resource(ResourceKind::GitHubPullRequest)
                .credential_required("github_token")
                .artifact_strategy(ArtifactStrategy::ProbeFirst)
                .ready(false),
        ),
        routing_summary("web_fetch", RoutingSummarySpec::default()),
    ];
    let advisories = vec![route_advisory(
        ResourceKind::GitHubPullRequest,
        "view_pr",
        RouteAdvisoryOutcome::Prefer,
    )];

    let plan = build_route_plan(&resource, &tools, &routing_tools, &advisories)
        .expect("fallback route should exist");

    assert_eq!(plan.primary_route.family, RouteFamily::PublicWeb);
    assert_eq!(plan.primary_route.tool_names, vec!["web_fetch".to_string()]);
    assert_eq!(
        plan.primary_route.ranking_basis,
        RouteRankingBasis::TypedPolicyOnly
    );
}

#[test]
fn advisory_memory_cannot_bypass_probe_first_requirements() {
    let resource = detect_route_resource("Review https://github.com/fawxai/fawx/pull/1753/files")
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
    let advisories = vec![route_advisory(
        ResourceKind::GitHubPullRequest,
        "view_pr_file_patch",
        RouteAdvisoryOutcome::Prefer,
    )];

    let plan = build_route_plan(&resource, &tools, &github_routing_tools(), &advisories)
        .expect("github route plan should exist");

    assert_eq!(plan.primary_route.family, RouteFamily::GitHub);
    assert_eq!(
        plan.primary_route.artifact_strategy,
        ArtifactStrategy::ProbeFirst
    );
    assert_eq!(
        plan.primary_route.tool_names,
        vec!["list_pr_files".to_string(), "view_pr".to_string()]
    );
    assert_eq!(
        plan.primary_route.ranking_basis,
        RouteRankingBasis::TypedPolicyOnly
    );
    assert_eq!(
        plan.fallback_routes[0].ranking_basis,
        RouteRankingBasis::TypedPolicyPlusAdvisory
    );
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

    let signals = engine.signals.signals();
    let route_signal = signals
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
    assert_eq!(
        route_signal.metadata["primary_route"]["ranking_basis"],
        "typed_policy_only"
    );
    assert_eq!(route_signal.metadata["requires_probe"], true);
    assert_eq!(route_signal.metadata["decision_kind"], "preflight_route");
    assert_eq!(route_signal.metadata["decision"], "planned");
}

#[tokio::test]
async fn advisory_route_influence_is_visible_in_trace_signals() {
    let mut engine = engine_with_preflight_routing(
        &[
            "view_pr",
            "list_pr_files",
            "view_pr_file_patch",
            "web_fetch",
        ],
        github_routing_tools(),
    );
    engine.set_route_advisories(vec![route_advisory(
        ResourceKind::GitHubPullRequest,
        "view_pr",
        RouteAdvisoryOutcome::Prefer,
    )]);

    let _ = engine
        .perceive(&test_snapshot(
            "Inspect https://github.com/fawxai/fawx/pull/1753 before reviewing it.",
        ))
        .await
        .expect("perceive");

    let signals = engine.signals.signals();
    let route_signal = signals
        .iter()
        .find(|signal| {
            signal.kind == SignalKind::Trace
                && signal.message == "planned preflight external resource route"
        })
        .expect("route trace signal");

    assert_eq!(
        route_signal.metadata["primary_route"]["ranking_basis"],
        "typed_policy_plus_advisory"
    );
    assert_eq!(
        route_signal.metadata["primary_route"]["advisory_influence"]["source"],
        "journal"
    );
    assert_eq!(
        route_signal.metadata["primary_route"]["advisory_influence"]["preferred_tools"][0],
        "view_pr"
    );
}

#[tokio::test]
async fn same_turn_reroute_advances_to_next_planned_route_and_preserves_diagnostics() {
    let executor = Arc::new(ScriptedRouteExecutor::new(
        &["web_fetch", "view_pr"],
        vec![
            ScriptedToolOutcome::failure(
                "web_fetch",
                "generic route could not access the resource",
                FailureClass::VisibilityMismatch,
            )
            .with_diagnostics(reroute_test_http_diagnostics(404)),
            ScriptedToolOutcome::success("view_pr", r#"{"number":1753}"#),
        ],
    ));
    let mut engine = mixed_tool_engine_with_executor(BudgetConfig::default(), executor.clone());
    engine.set_runtime_info(runtime_info_with_routing_tools(github_routing_tools()));
    let prompt = "Review https://github.com/fawxai/fawx/pull/1753";
    let processed = engine
        .perceive(&test_snapshot(prompt))
        .await
        .expect("perceive");
    engine.preflight_route_plan = Some(RoutePlan {
        resource: github_resource(),
        primary_route: planned_route(
            RouteFamily::PublicWeb,
            &["web_fetch"],
            false,
            ArtifactStrategy::DirectFetch,
            50,
        ),
        fallback_routes: vec![planned_route(
            RouteFamily::GitHub,
            &["view_pr"],
            true,
            ArtifactStrategy::DirectFetch,
            10,
        )],
        requires_probe: false,
        active_route_index: 0,
    });

    let llm = RecordingLlm::ok(vec![
        tool_use_response(vec![ToolCall {
            id: "call-1".to_string(),
            name: "web_fetch".to_string(),
            arguments: serde_json::json!({}),
        }]),
        tool_use_response(vec![ToolCall {
            id: "call-2".to_string(),
            name: "view_pr".to_string(),
            arguments: serde_json::json!({}),
        }]),
        text_response("done after reroute"),
    ]);
    let response = engine
        .reason(&processed, &llm, CycleStream::disabled())
        .await
        .expect("reason");
    let decision = engine.decide(&response).await.expect("decide");
    let action = engine
        .act(
            &decision,
            &llm,
            &processed.context_window,
            CycleStream::disabled(),
        )
        .await
        .expect("act");

    match action.next_step {
        ActionNextStep::Finish(ActionTerminal::Complete { response }) => {
            assert_eq!(response, "done after reroute");
        }
        ActionNextStep::Continue(continuation) => {
            assert_eq!(
                continuation.partial_response, None,
                "intermediate reroute output should stay out of user-visible partial text"
            );
        }
        other => panic!("expected reroute continuation, got {other:?}"),
    }
    assert_eq!(executor.calls(), vec!["web_fetch", "view_pr"]);
    assert!(engine.preflight_route_plan.is_none());

    let signals = engine.signals.signals();
    let reroute_signal = signals
        .iter()
        .find(|signal| signal.message == "rerouted preflight external resource route")
        .expect("reroute signal");
    assert_eq!(
        reroute_signal.metadata["failure_class"],
        "visibility_mismatch"
    );
    assert_eq!(
        reroute_signal.metadata["next_route"]["tool_names"][0],
        "view_pr"
    );

    let failed_tool_signal = signals
        .iter()
        .find(|signal| signal.kind == SignalKind::Friction && signal.message == "tool web_fetch")
        .expect("failed tool signal");
    assert_eq!(
        failed_tool_signal.metadata["failure_class"],
        "visibility_mismatch"
    );
    assert_eq!(failed_tool_signal.metadata["diagnostics"]["kind"], "http");
    assert_eq!(
        failed_tool_signal.metadata["diagnostics"]["status_code"],
        404
    );
}

#[tokio::test]
async fn route_exhaustion_happens_only_after_all_planned_fallbacks_are_tried() {
    let executor = Arc::new(ScriptedRouteExecutor::new(
        &["web_fetch", "view_pr"],
        vec![
            ScriptedToolOutcome::failure(
                "web_fetch",
                "generic route blocked",
                FailureClass::VisibilityMismatch,
            ),
            ScriptedToolOutcome::failure(
                "view_pr",
                "authenticated route rejected the request",
                FailureClass::InvalidRequest,
            ),
        ],
    ));
    let mut engine = mixed_tool_engine_with_executor(BudgetConfig::default(), executor.clone());
    engine.set_runtime_info(runtime_info_with_routing_tools(github_routing_tools()));
    let prompt = "Review https://github.com/fawxai/fawx/pull/1753";
    let processed = engine
        .perceive(&test_snapshot(prompt))
        .await
        .expect("perceive");
    engine.preflight_route_plan = Some(RoutePlan {
        resource: github_resource(),
        primary_route: planned_route(
            RouteFamily::PublicWeb,
            &["web_fetch"],
            false,
            ArtifactStrategy::DirectFetch,
            50,
        ),
        fallback_routes: vec![planned_route(
            RouteFamily::GitHub,
            &["view_pr"],
            true,
            ArtifactStrategy::DirectFetch,
            10,
        )],
        requires_probe: false,
        active_route_index: 0,
    });

    let llm = RecordingLlm::ok(vec![
        tool_use_response(vec![ToolCall {
            id: "call-1".to_string(),
            name: "web_fetch".to_string(),
            arguments: serde_json::json!({}),
        }]),
        tool_use_response(vec![ToolCall {
            id: "call-2".to_string(),
            name: "view_pr".to_string(),
            arguments: serde_json::json!({}),
        }]),
        text_response("all planned routes failed"),
    ]);
    let response = engine
        .reason(&processed, &llm, CycleStream::disabled())
        .await
        .expect("reason");
    let decision = engine.decide(&response).await.expect("decide");
    let _action = engine
        .act(
            &decision,
            &llm,
            &processed.context_window,
            CycleStream::disabled(),
        )
        .await
        .expect("act");

    assert_eq!(executor.calls(), vec!["web_fetch", "view_pr"]);
    let signals = engine.signals.signals();
    let exhausted_signal = signals
        .iter()
        .find(|signal| signal.message == "preflight external resource route exhausted")
        .expect("exhausted signal");
    assert_eq!(
        exhausted_signal.metadata["failure_class"],
        "invalid_request"
    );
    assert!(engine.preflight_route_plan.is_none());
}

#[tokio::test]
async fn not_found_reroute_is_bounded_and_skips_equivalent_routes() {
    let executor = Arc::new(ScriptedRouteExecutor::new(
        &["view_pr", "view_pr_file_patch", "web_fetch"],
        vec![
            ScriptedToolOutcome::failure("view_pr", "resource missing", FailureClass::NotFound),
            ScriptedToolOutcome::success("web_fetch", "public mirror"),
        ],
    ));
    let mut engine = mixed_tool_engine_with_executor(BudgetConfig::default(), executor.clone());
    engine.set_runtime_info(runtime_info_with_routing_tools(github_routing_tools()));
    let prompt = "Review https://github.com/fawxai/fawx/pull/1753";
    let processed = engine
        .perceive(&test_snapshot(prompt))
        .await
        .expect("perceive");
    engine.preflight_route_plan = Some(RoutePlan {
        resource: github_resource(),
        primary_route: planned_route(
            RouteFamily::GitHub,
            &["view_pr"],
            true,
            ArtifactStrategy::DirectFetch,
            10,
        ),
        fallback_routes: vec![
            planned_route(
                RouteFamily::GitHub,
                &["view_pr_file_patch"],
                true,
                ArtifactStrategy::DirectFetch,
                20,
            ),
            planned_route(
                RouteFamily::PublicWeb,
                &["web_fetch"],
                false,
                ArtifactStrategy::DirectFetch,
                30,
            ),
        ],
        requires_probe: false,
        active_route_index: 0,
    });

    let llm = RecordingLlm::ok(vec![
        tool_use_response(vec![ToolCall {
            id: "call-1".to_string(),
            name: "view_pr".to_string(),
            arguments: serde_json::json!({}),
        }]),
        tool_use_response(vec![ToolCall {
            id: "call-2".to_string(),
            name: "web_fetch".to_string(),
            arguments: serde_json::json!({}),
        }]),
        text_response("bounded fallback"),
    ]);
    let response = engine
        .reason(&processed, &llm, CycleStream::disabled())
        .await
        .expect("reason");
    let decision = engine.decide(&response).await.expect("decide");
    let _action = engine
        .act(
            &decision,
            &llm,
            &processed.context_window,
            CycleStream::disabled(),
        )
        .await
        .expect("act");

    assert_eq!(executor.calls(), vec!["view_pr", "web_fetch"]);
    assert!(
        !executor.calls().contains(&"view_pr_file_patch".to_string()),
        "not_found should skip equivalent same-family fallback"
    );
}

#[tokio::test]
async fn transient_transport_keeps_same_family_fallback_before_public_web() {
    let executor = Arc::new(ScriptedRouteExecutor::new(
        &["view_pr", "view_pr_file_patch", "web_fetch"],
        vec![
            ScriptedToolOutcome::failure(
                "view_pr",
                "connection reset by peer",
                FailureClass::TransientTransport,
            ),
            ScriptedToolOutcome::success("view_pr_file_patch", "patch contents"),
        ],
    ));
    let mut engine = mixed_tool_engine_with_executor(BudgetConfig::default(), executor.clone());
    engine.set_runtime_info(runtime_info_with_routing_tools(github_routing_tools()));
    let prompt = "Review https://github.com/fawxai/fawx/pull/1753";
    let processed = engine
        .perceive(&test_snapshot(prompt))
        .await
        .expect("perceive");

    let llm = RecordingLlm::ok(vec![
        tool_use_response(vec![ToolCall {
            id: "call-1".to_string(),
            name: "view_pr".to_string(),
            arguments: serde_json::json!({}),
        }]),
        tool_use_response(vec![ToolCall {
            id: "call-2".to_string(),
            name: "view_pr_file_patch".to_string(),
            arguments: serde_json::json!({}),
        }]),
        text_response("same-family fallback succeeded"),
    ]);
    let response = engine
        .reason(&processed, &llm, CycleStream::disabled())
        .await
        .expect("reason");
    let decision = engine.decide(&response).await.expect("decide");
    let _action = engine
        .act(
            &decision,
            &llm,
            &processed.context_window,
            CycleStream::disabled(),
        )
        .await
        .expect("act");

    assert_eq!(executor.calls(), vec!["view_pr", "view_pr_file_patch"]);
    assert!(
        !executor.calls().contains(&"web_fetch".to_string()),
        "transient transport should retry the next same-family route before public web"
    );
}
