//! Self-improvement tool interfaces: `analyze_signals` and `propose_improvement`.
//!
//! Wires the existing fx-analysis and fx-improve crate APIs into the tool
//! dispatch system so the LLM can self-initiate improvement cycles.
//!
//! Security invariants:
//! - `analyze_signals` is READ-ONLY — no file writes, no git ops.
//! - `propose_improvement` returns "pending_approval" — never auto-executes.
//! - Rate limits are non-configurable via tools.
//! - Tools only appear when `[improvement] enabled = true`.

use fx_analysis::{AnalysisEngine, AnalysisFinding, Confidence};
use fx_config::ImprovementToolsConfig;
use fx_improve::{ImprovementConfig, ImprovementDetector, ImprovementExecutor, ImprovementPlanner};
use fx_llm::{CompletionProvider, ToolDefinition};
use fx_memory::SignalStore;
use serde::Deserialize;
use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// Bundled state for improvement tools, stored as a single optional field
/// on `FawxToolExecutor` to keep cfg-gating minimal.
#[derive(Clone)]
pub struct ImprovementToolsState {
    pub(crate) signal_store: Arc<SignalStore>,
    pub(crate) llm_provider: Arc<dyn CompletionProvider + Send + Sync>,
    pub(crate) rate_limiter: Arc<Mutex<ImprovementRateLimiter>>,
    pub(crate) finding_cache: Arc<Mutex<FindingCache>>,
    pub(crate) config: ImprovementToolsConfig,
}

impl fmt::Debug for ImprovementToolsState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ImprovementToolsState")
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

impl ImprovementToolsState {
    pub fn new(
        signal_store: Arc<SignalStore>,
        llm_provider: Arc<dyn CompletionProvider + Send + Sync>,
        config: ImprovementToolsConfig,
    ) -> Self {
        let rate_limiter = ImprovementRateLimiter::from_config(&config);
        Self {
            signal_store,
            llm_provider,
            rate_limiter: Arc::new(Mutex::new(rate_limiter)),
            finding_cache: Arc::new(Mutex::new(FindingCache::new())),
            config,
        }
    }
}

// ── Rate limiter ──────────────────────────────────────────────────────

const ANALYSIS_WINDOW: std::time::Duration = std::time::Duration::from_secs(3600);
const PROPOSAL_WINDOW: std::time::Duration = std::time::Duration::from_secs(86400);

#[derive(Debug, Clone)]
pub(crate) struct ImprovementRateLimiter {
    analysis_timestamps: Vec<Instant>,
    proposal_timestamps: Vec<Instant>,
    max_analyses_per_hour: u32,
    max_proposals_per_day: u32,
}

impl ImprovementRateLimiter {
    pub(crate) fn from_config(config: &ImprovementToolsConfig) -> Self {
        Self {
            analysis_timestamps: Vec::new(),
            proposal_timestamps: Vec::new(),
            max_analyses_per_hour: config.max_analyses_per_hour,
            max_proposals_per_day: config.max_proposals_per_day,
        }
    }

    fn check_analysis(&mut self) -> Result<(), String> {
        let now = Instant::now();
        self.analysis_timestamps
            .retain(|ts| now.duration_since(*ts) < ANALYSIS_WINDOW);
        let max = self.max_analyses_per_hour;
        if self.analysis_timestamps.len() >= max as usize {
            return Err(format!(
                "Rate limit exceeded: maximum {max} analyses per hour"
            ));
        }
        self.analysis_timestamps.push(now);
        Ok(())
    }

    fn check_proposal(&mut self) -> Result<(), String> {
        let now = Instant::now();
        self.proposal_timestamps
            .retain(|ts| now.duration_since(*ts) < PROPOSAL_WINDOW);
        let max = self.max_proposals_per_day;
        if self.proposal_timestamps.len() >= max as usize {
            return Err(format!(
                "Rate limit exceeded: maximum {max} proposals per day"
            ));
        }
        self.proposal_timestamps.push(now);
        Ok(())
    }
}

// ── Finding cache ─────────────────────────────────────────────────────

const FINDING_CACHE_TTL: std::time::Duration = std::time::Duration::from_secs(3600);

#[derive(Debug, Clone)]
pub(crate) struct FindingCache {
    entries: HashMap<String, CachedFinding>,
}

#[derive(Debug, Clone)]
struct CachedFinding {
    finding: AnalysisFinding,
    cached_at: Instant,
}

impl FindingCache {
    pub(crate) fn new() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }

    fn insert(&mut self, finding: AnalysisFinding) -> String {
        self.evict_expired();
        let id = uuid::Uuid::new_v4().to_string();
        self.entries.insert(
            id.clone(),
            CachedFinding {
                finding,
                cached_at: Instant::now(),
            },
        );
        id
    }

    fn get(&mut self, id: &str) -> Option<&AnalysisFinding> {
        self.evict_expired();
        let entry = self.entries.get(id)?;
        Some(&entry.finding)
    }

    fn evict_expired(&mut self) {
        self.entries
            .retain(|_, entry| entry.cached_at.elapsed() < FINDING_CACHE_TTL);
    }
}

// ── Tool definitions ──────────────────────────────────────────────────

pub(crate) fn improvement_tool_definitions() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            name: "analyze_signals".to_string(),
            description: "Analyze accumulated runtime signals to identify recurring patterns, \
                friction points, and improvement opportunities. Returns structured findings with \
                confidence levels and supporting evidence. Rate-limited to prevent excessive \
                analysis."
                .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    // TODO: Add scope filtering (recent/session/all) once
                    // AnalysisEngine supports evidence-timestamp-based ranges.
                    "min_confidence": {
                        "type": "string",
                        "enum": ["low", "medium", "high"],
                        "description": "Minimum confidence threshold for reported findings"
                    }
                },
                "required": []
            }),
        },
        ToolDefinition {
            name: "propose_improvement".to_string(),
            description: "Create a concrete improvement proposal from an analysis finding. \
                Generates a fix plan with specific file changes, creates a git branch, and \
                submits the proposal for approval. Requires an active finding from \
                analyze_signals. Rate-limited: maximum 3 self-proposals per day."
                .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "finding_id": {
                        "type": "string",
                        "description": "ID of the analysis finding to address (from analyze_signals output)"
                    },
                    "description": {
                        "type": "string",
                        "description": "Human-readable description of the proposed improvement"
                    }
                },
                "required": ["finding_id", "description"]
            }),
        },
    ]
}

// ── Argument types ────────────────────────────────────────────────────

#[derive(Deserialize)]
pub(crate) struct AnalyzeSignalsArgs {
    min_confidence: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct ProposeImprovementArgs {
    finding_id: String,
    description: String,
}

// ── Handler implementations ───────────────────────────────────────────

/// Analyze accumulated runtime signals. READ-ONLY — no side effects.
pub(crate) async fn handle_analyze_signals(
    state: &ImprovementToolsState,
    args: &serde_json::Value,
) -> Result<String, String> {
    let parsed: AnalyzeSignalsArgs =
        serde_json::from_value(args.clone()).map_err(|e| e.to_string())?;

    // Rate limit check
    {
        let mut limiter = state
            .rate_limiter
            .lock()
            .map_err(|e| format!("rate limiter lock: {e}"))?;
        limiter.check_analysis()?;
    }

    let engine = AnalysisEngine::new(&state.signal_store);
    let findings = engine
        .analyze(state.llm_provider.as_ref())
        .await
        .map_err(|e| format!("analysis failed: {e}"))?;

    let min_confidence = parse_confidence(parsed.min_confidence.as_deref());
    let filtered = filter_findings(findings, min_confidence);

    // Cache findings and assign IDs
    let results = cache_findings(&state.finding_cache, filtered)?;

    serde_json::to_string_pretty(&results).map_err(|e| e.to_string())
}

/// Validate proposal arguments: check rate limit and resolve the cached finding.
fn validate_proposal_request(
    args: &serde_json::Value,
    rate_limiter: &Arc<Mutex<ImprovementRateLimiter>>,
    cache: &Arc<Mutex<FindingCache>>,
) -> Result<(ProposeImprovementArgs, AnalysisFinding), String> {
    let parsed: ProposeImprovementArgs =
        serde_json::from_value(args.clone()).map_err(|e| e.to_string())?;

    {
        let mut limiter = rate_limiter
            .lock()
            .map_err(|e| format!("rate limiter lock: {e}"))?;
        limiter.check_proposal()?;
    }

    let finding = {
        let mut guard = cache
            .lock()
            .map_err(|e| format!("finding cache lock: {e}"))?;
        guard.get(&parsed.finding_id).cloned().ok_or_else(|| {
            format!(
                "Finding '{}' not found or expired. Run analyze_signals first.",
                parsed.finding_id
            )
        })?
    };

    Ok((parsed, finding))
}

/// Run the detect → plan → execute pipeline for a single finding.
async fn run_improvement_pipeline(
    finding: &AnalysisFinding,
    llm_provider: &dyn CompletionProvider,
    working_dir: &std::path::Path,
) -> Result<fx_improve::ImprovementRunResult, String> {
    let data_dir = working_dir.join(".fawx");
    let improve_config = ImprovementConfig::default();

    let mut detector = ImprovementDetector::new(improve_config.clone(), &data_dir)
        .map_err(|e| format!("detector init: {e}"))?;
    let candidates = detector.detect(std::slice::from_ref(finding));

    if candidates.is_empty() {
        return Ok(fx_improve::ImprovementRunResult::empty());
    }

    let planning = ImprovementPlanner::plan(&candidates, llm_provider, working_dir).await;
    let proposals_dir = data_dir.join("proposals");
    let executor =
        ImprovementExecutor::new(improve_config, proposals_dir, working_dir.to_path_buf());
    let execution = executor
        .execute(&planning.plans, &mut detector)
        .map_err(|e| format!("execution failed: {e}"))?;

    Ok(planning.into_run_result(execution))
}

/// Create an improvement proposal from a cached finding. Returns "pending_approval".
pub(crate) async fn handle_propose_improvement(
    state: &ImprovementToolsState,
    args: &serde_json::Value,
    working_dir: &std::path::Path,
) -> Result<String, String> {
    let (parsed, finding) =
        validate_proposal_request(args, &state.rate_limiter, &state.finding_cache)?;
    let result =
        run_improvement_pipeline(&finding, state.llm_provider.as_ref(), working_dir).await?;

    let response = if result.proposals_written.is_empty() && result.branches_created.is_empty() {
        skipped_proposal_response(&result)
    } else {
        pending_proposal_response(&parsed, &result)
    };

    serde_json::to_string_pretty(&response).map_err(|e| e.to_string())
}

fn pending_proposal_response(
    parsed: &ProposeImprovementArgs,
    result: &fx_improve::ImprovementRunResult,
) -> serde_json::Value {
    serde_json::json!({
        "status": "pending_approval",
        "proposal_id": parsed.finding_id,
        "description": parsed.description,
        "plans_generated": result.plans_generated,
        "proposals_written": result.proposals_written.iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>(),
        "branches_created": result.branches_created,
        "skipped_candidates": serialize_skipped_candidates(&result.skipped_candidates),
        "message": "Proposal created. Run /approve to execute the changes."
    })
}

fn skipped_proposal_response(result: &fx_improve::ImprovementRunResult) -> serde_json::Value {
    if result.skipped_candidates.is_empty() {
        return serde_json::json!({
            "status": "skipped",
            "reason": "Finding did not produce actionable candidates or plans"
        });
    }

    serde_json::json!({
        "status": "skipped",
        "reason": "All actionable candidates were skipped during planning",
        "summary": fx_improve::skipped_candidate_summary(&result.skipped_candidates),
        "skipped_candidates": serialize_skipped_candidates(&result.skipped_candidates),
    })
}

fn serialize_skipped_candidates(
    skipped_candidates: &[fx_improve::SkippedCandidate],
) -> Vec<serde_json::Value> {
    skipped_candidates
        .iter()
        .map(|candidate| {
            serde_json::json!({
                "name": candidate.name,
                "reason": candidate.reason,
            })
        })
        .collect()
}

// ── Helpers ───────────────────────────────────────────────────────────

fn parse_confidence(value: Option<&str>) -> Option<Confidence> {
    match value {
        Some("high") => Some(Confidence::High),
        Some("medium") => Some(Confidence::Medium),
        Some("low") => Some(Confidence::Low),
        _ => None,
    }
}

fn confidence_rank(c: Confidence) -> u8 {
    match c {
        Confidence::High => 3,
        Confidence::Medium => 2,
        Confidence::Low => 1,
    }
}

fn filter_findings(
    findings: Vec<AnalysisFinding>,
    min_confidence: Option<Confidence>,
) -> Vec<AnalysisFinding> {
    let Some(min) = min_confidence else {
        return findings;
    };
    findings
        .into_iter()
        .filter(|f| confidence_rank(f.confidence) >= confidence_rank(min))
        .collect()
}

fn cache_findings(
    cache: &Arc<Mutex<FindingCache>>,
    findings: Vec<AnalysisFinding>,
) -> Result<Vec<serde_json::Value>, String> {
    let mut guard = cache.lock().map_err(|e| format!("cache lock: {e}"))?;
    let mut results = Vec::with_capacity(findings.len());
    for finding in findings {
        let id = guard.insert(finding.clone());
        results.push(serde_json::json!({
            "finding_id": id,
            "pattern_name": finding.pattern_name,
            "description": finding.description,
            "confidence": format!("{:?}", finding.confidence).to_lowercase(),
            "evidence_count": finding.evidence.len(),
            "suggested_action": finding.suggested_action,
        }));
    }
    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use fx_core::signals::{LoopStep, Signal, SignalKind};
    use fx_llm::{
        CompletionRequest, CompletionResponse, CompletionStream, ProviderCapabilities,
        ProviderError, ToolCall as LlmToolCall,
    };
    use std::collections::VecDeque;
    use std::sync::Mutex;
    use tempfile::TempDir;

    // ── Mock provider ─────────────────────────────────────────────────

    #[derive(Debug)]
    struct MockProvider {
        responses: Mutex<VecDeque<Result<CompletionResponse, ProviderError>>>,
    }

    impl MockProvider {
        fn with_findings(findings: Vec<serde_json::Value>) -> Self {
            Self::with_responses(vec![Ok(report_findings_response(findings))])
        }

        fn with_responses(responses: Vec<Result<CompletionResponse, ProviderError>>) -> Self {
            Self {
                responses: Mutex::new(VecDeque::from(responses)),
            }
        }

        fn empty() -> Self {
            Self::with_responses((0..64).map(|_| Ok(empty_response())).collect())
        }
    }

    #[async_trait]
    impl CompletionProvider for MockProvider {
        async fn complete(
            &self,
            _request: CompletionRequest,
        ) -> Result<CompletionResponse, ProviderError> {
            self.responses
                .lock()
                .expect("mock provider lock")
                .pop_front()
                .expect("mock response")
        }

        async fn complete_stream(
            &self,
            _request: CompletionRequest,
        ) -> Result<CompletionStream, ProviderError> {
            Err(ProviderError::Provider("not supported".to_string()))
        }

        fn name(&self) -> &str {
            "mock"
        }

        fn supported_models(&self) -> Vec<String> {
            vec!["mock".to_string()]
        }

        fn capabilities(&self) -> ProviderCapabilities {
            ProviderCapabilities {
                supports_temperature: true,
                requires_streaming: false,
            }
        }
    }

    // ── Test helpers ──────────────────────────────────────────────────

    fn report_findings_response(findings: Vec<serde_json::Value>) -> CompletionResponse {
        CompletionResponse {
            content: Vec::new(),
            tool_calls: vec![LlmToolCall {
                id: "call-1".to_string(),
                name: "report_findings".to_string(),
                arguments: serde_json::json!({ "findings": findings }),
            }],
            usage: None,
            stop_reason: Some("tool_use".to_string()),
        }
    }

    fn empty_response() -> CompletionResponse {
        CompletionResponse {
            content: Vec::new(),
            tool_calls: Vec::new(),
            usage: None,
            stop_reason: Some("end_turn".to_string()),
        }
    }

    fn sample_finding_json(name: &str, confidence: &str) -> serde_json::Value {
        sample_finding_json_with_evidence(name, confidence, 1)
    }

    fn sample_finding_json_with_evidence(
        name: &str,
        confidence: &str,
        evidence_count: usize,
    ) -> serde_json::Value {
        let evidence = (0..evidence_count)
            .map(|index| {
                serde_json::json!({
                    "session_id": format!("sess-{}", index + 1),
                    "signal_kind": "friction",
                    "message": format!("test signal {}", index + 1),
                    "timestamp_ms": 1000 + index as u64,
                })
            })
            .collect::<Vec<_>>();

        serde_json::json!({
            "pattern_name": name,
            "description": format!("Description for {name}"),
            "confidence": confidence,
            "evidence": evidence,
            "suggested_action": "Fix the issue"
        })
    }

    fn build_test_state(
        tmp: &TempDir,
        provider: MockProvider,
    ) -> (ImprovementToolsState, std::path::PathBuf) {
        let data_dir = tmp.path().join("data");
        let store = SignalStore::new(&data_dir, "test-session").expect("signal store");
        let config = ImprovementToolsConfig {
            enabled: true,
            ..ImprovementToolsConfig::default()
        };
        let state = ImprovementToolsState::new(Arc::new(store), Arc::new(provider), config);
        (state, data_dir)
    }

    fn build_state_with_signals(
        tmp: &TempDir,
        provider: MockProvider,
    ) -> (ImprovementToolsState, std::path::PathBuf) {
        let data_dir = tmp.path().join("data");
        let store = SignalStore::new(&data_dir, "test-session").expect("signal store");
        store
            .persist(&[Signal {
                step: LoopStep::Act,
                kind: SignalKind::Friction,
                message: "test friction".to_string(),
                metadata: serde_json::json!({}),
                timestamp_ms: 1000,
            }])
            .expect("persist signal");

        let config = ImprovementToolsConfig {
            enabled: true,
            ..ImprovementToolsConfig::default()
        };
        let state = ImprovementToolsState::new(Arc::new(store), Arc::new(provider), config);
        (state, data_dir)
    }

    // ── Tests ─────────────────────────────────────────────────────────

    #[tokio::test]
    async fn analyze_signals_returns_findings() {
        let tmp = TempDir::new().expect("tempdir");
        let provider =
            MockProvider::with_findings(vec![sample_finding_json("Timeout loop", "high")]);
        let (state, _) = build_state_with_signals(&tmp, provider);

        let result = handle_analyze_signals(&state, &serde_json::json!({}))
            .await
            .expect("analyze should succeed");

        let parsed: Vec<serde_json::Value> = serde_json::from_str(&result).expect("valid json");
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0]["pattern_name"], "Timeout loop");
        assert!(parsed[0]["finding_id"].as_str().is_some());
    }

    #[tokio::test]
    async fn analyze_signals_ignores_unknown_fields() {
        // Scope is not currently a parameter (see TODO in tool definition).
        // Verify that unknown fields in the args are silently ignored.
        let tmp = TempDir::new().expect("tempdir");
        let provider =
            MockProvider::with_findings(vec![sample_finding_json("Recent issue", "medium")]);
        let (state, _) = build_state_with_signals(&tmp, provider);

        let result = handle_analyze_signals(&state, &serde_json::json!({"scope": "recent"}))
            .await
            .expect("analyze should succeed with unknown field");

        let parsed: Vec<serde_json::Value> = serde_json::from_str(&result).expect("valid json");
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0]["pattern_name"], "Recent issue");
    }

    #[tokio::test]
    async fn analyze_signals_respects_confidence_filter() {
        let tmp = TempDir::new().expect("tempdir");
        let provider = MockProvider::with_findings(vec![
            sample_finding_json("High conf", "high"),
            sample_finding_json("Low conf", "low"),
        ]);
        let (state, _) = build_state_with_signals(&tmp, provider);

        let result = handle_analyze_signals(&state, &serde_json::json!({"min_confidence": "high"}))
            .await
            .expect("analyze should succeed");

        let parsed: Vec<serde_json::Value> = serde_json::from_str(&result).expect("valid json");
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0]["pattern_name"], "High conf");
    }

    #[tokio::test]
    async fn analyze_signals_rate_limited() {
        let tmp = TempDir::new().expect("tempdir");
        let provider = MockProvider::empty();
        let (state, _) = build_test_state(&tmp, provider);

        // Fill up rate limit (uses default config value)
        for _ in 0..state.config.max_analyses_per_hour {
            let _ = handle_analyze_signals(&state, &serde_json::json!({})).await;
        }

        // 11th call should fail
        let result = handle_analyze_signals(&state, &serde_json::json!({})).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.contains("Rate limit exceeded"),
            "expected rate limit error, got: {err}"
        );
    }

    #[tokio::test]
    async fn propose_improvement_requires_valid_finding_id() {
        let tmp = TempDir::new().expect("tempdir");
        let provider = MockProvider::empty();
        let (state, _) = build_test_state(&tmp, provider);
        let working_dir = tmp.path().join("repo");
        std::fs::create_dir_all(&working_dir).expect("create repo dir");

        let result = handle_propose_improvement(
            &state,
            &serde_json::json!({
                "finding_id": "nonexistent-id",
                "description": "Fix something"
            }),
            &working_dir,
        )
        .await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.contains("not found or expired"),
            "expected not found error, got: {err}"
        );
    }

    #[tokio::test]
    async fn propose_improvement_rate_limited() {
        let tmp = TempDir::new().expect("tempdir");
        let provider = MockProvider::empty();
        let (state, _) = build_test_state(&tmp, provider);
        let working_dir = tmp.path().join("repo");
        std::fs::create_dir_all(&working_dir).expect("create repo dir");

        // Fill up proposal rate limit
        {
            let mut limiter = state.rate_limiter.lock().expect("lock");
            for _ in 0..state.config.max_proposals_per_day {
                limiter.check_proposal().expect("should succeed");
            }
        }

        let result = handle_propose_improvement(
            &state,
            &serde_json::json!({
                "finding_id": "any-id",
                "description": "Fix something"
            }),
            &working_dir,
        )
        .await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.contains("Rate limit exceeded"),
            "expected rate limit error, got: {err}"
        );
    }

    #[tokio::test]
    async fn propose_improvement_skipped_path() {
        // This test exercises the "skipped" path: the finding has only 1
        // evidence but the detector requires min_evidence_count=3, so
        // the pipeline short-circuits before planning. This verifies
        // that validate_proposal_request succeeds and the detector
        // correctly rejects insufficient evidence.
        //
        // TODO(#1209): Add a separate test with ≥3 mock signals that
        // exercises the full detect → plan → execute → pending_approval
        // flow. Requires mocking the planner/executor returns.
        let tmp = TempDir::new().expect("tempdir");
        let provider =
            MockProvider::with_findings(vec![sample_finding_json("Fixable issue", "high")]);
        let (state, _) = build_state_with_signals(&tmp, provider);

        let analysis_result = handle_analyze_signals(&state, &serde_json::json!({}))
            .await
            .expect("analyze should succeed");

        let findings: Vec<serde_json::Value> =
            serde_json::from_str(&analysis_result).expect("valid json");
        let finding_id = findings[0]["finding_id"]
            .as_str()
            .expect("finding_id")
            .to_string();

        let working_dir = tmp.path().join("repo");
        std::fs::create_dir_all(&working_dir).expect("create repo dir");

        let result = handle_propose_improvement(
            &state,
            &serde_json::json!({
                "finding_id": finding_id,
                "description": "Fix the timeout issue"
            }),
            &working_dir,
        )
        .await
        .expect("propose should succeed (returning skipped)");

        let parsed: serde_json::Value = serde_json::from_str(&result).expect("valid json");
        assert_eq!(
            parsed["status"], "skipped",
            "expected skipped due to insufficient evidence, got: {}",
            parsed["status"]
        );
    }

    #[tokio::test]
    async fn propose_improvement_returns_planning_skips_when_all_plans_fail() {
        let tmp = TempDir::new().expect("tempdir");
        let provider = MockProvider::with_responses(vec![
            Ok(report_findings_response(vec![
                sample_finding_json_with_evidence("Fixable issue", "high", 3),
            ])),
            Ok(empty_response()),
        ]);
        let (state, _) = build_state_with_signals(&tmp, provider);

        let analysis_result = handle_analyze_signals(&state, &serde_json::json!({}))
            .await
            .expect("analyze should succeed");
        let findings: Vec<serde_json::Value> =
            serde_json::from_str(&analysis_result).expect("valid json");
        let finding_id = findings[0]["finding_id"]
            .as_str()
            .expect("finding_id")
            .to_string();

        let working_dir = tmp.path().join("repo");
        std::fs::create_dir_all(&working_dir).expect("create repo dir");

        let result = handle_propose_improvement(
            &state,
            &serde_json::json!({
                "finding_id": finding_id,
                "description": "Fix the timeout issue"
            }),
            &working_dir,
        )
        .await
        .expect("propose should return skipped details");

        let parsed: serde_json::Value = serde_json::from_str(&result).expect("valid json");
        assert_eq!(parsed["status"], "skipped");
        assert_eq!(
            parsed["reason"],
            "All actionable candidates were skipped during planning"
        );
        assert_eq!(
            parsed["summary"],
            "1 candidate skipped (model did not produce a plan)"
        );
        assert_eq!(
            parsed["skipped_candidates"].as_array().map(Vec::len),
            Some(1)
        );
        assert_eq!(
            parsed["skipped_candidates"][0]["reason"],
            "model did not produce a plan"
        );
    }

    #[test]
    fn finding_cache_expires() {
        let mut cache = FindingCache::new();
        let finding = AnalysisFinding {
            pattern_name: "test".to_string(),
            description: "test desc".to_string(),
            confidence: Confidence::High,
            evidence: Vec::new(),
            suggested_action: None,
        };
        let id = cache.insert(finding);

        // Should be accessible immediately
        assert!(cache.get(&id).is_some());

        // Manually set cached_at to >1 hour ago
        if let Some(entry) = cache.entries.get_mut(&id) {
            entry.cached_at = Instant::now() - std::time::Duration::from_secs(3601);
        }

        // Should be expired now
        assert!(cache.get(&id).is_none());
    }

    #[test]
    fn finding_cache_assigns_unique_ids() {
        let mut cache = FindingCache::new();
        let finding = AnalysisFinding {
            pattern_name: "test".to_string(),
            description: "test desc".to_string(),
            confidence: Confidence::High,
            evidence: Vec::new(),
            suggested_action: None,
        };

        let id1 = cache.insert(finding.clone());
        let id2 = cache.insert(finding);

        assert_ne!(id1, id2, "cache must assign unique IDs");
        assert!(cache.get(&id1).is_some());
        assert!(cache.get(&id2).is_some());
    }

    #[test]
    fn tools_disabled_when_not_enabled() {
        // When config.enabled is false, tool_definitions should not
        // include improvement tools. This is tested at the FawxToolExecutor
        // level — see tools.rs tests. Here we verify the definitions exist
        // as expected when called directly.
        let defs = improvement_tool_definitions();
        assert_eq!(defs.len(), 2);

        let names: Vec<&str> = defs.iter().map(|d| d.name.as_str()).collect();
        assert!(names.contains(&"analyze_signals"));
        assert!(names.contains(&"propose_improvement"));
    }
}
