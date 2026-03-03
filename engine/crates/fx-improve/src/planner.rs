use crate::detector::ImprovementCandidate;
use crate::error::ImprovementError;
use fx_llm::{CompletionProvider, CompletionRequest, Message, ToolCall, ToolDefinition};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::{Path, PathBuf};

const PLAN_TOOL_NAME: &str = "generate_fix_plan";

const PLANNER_SYSTEM_PROMPT: &str = "\
You are a code improvement planner. Given an analysis finding with evidence, \
generate a concrete fix plan by calling the generate_fix_plan tool. \
Assess risk conservatively: Low for test/config/doc changes, Medium for \
behavioral changes with tests, High for architectural or kernel changes.";

/// Risk level for a proposed fix.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RiskLevel {
    /// Safe: test-only, config, or documentation change.
    Low,
    /// Moderate: behavioral change with test coverage.
    Medium,
    /// High: architectural change, kernel modification, or untestable.
    High,
}

impl fmt::Display for RiskLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Low => write!(f, "low"),
            Self::Medium => write!(f, "medium"),
            Self::High => write!(f, "high"),
        }
    }
}

/// A single file change proposed by the planner.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileChange {
    /// Path to the file being changed.
    pub path: PathBuf,
    /// Description of the change.
    pub description: String,
    /// Proposed new content (None if human must implement).
    pub content: Option<String>,
}

/// A concrete fix plan generated from an improvement candidate.
#[derive(Debug, Clone)]
pub struct FixPlan {
    /// The candidate this plan addresses.
    pub candidate: ImprovementCandidate,
    /// Target files to modify.
    pub target_files: Vec<PathBuf>,
    /// Natural language description of the fix.
    pub fix_description: String,
    /// Concrete code changes (None if human judgment required).
    pub code_changes: Option<Vec<FileChange>>,
    /// Risk assessment.
    pub risk: RiskLevel,
}

/// Generates fix plans from improvement candidates using an LLM.
pub struct ImprovementPlanner;

impl ImprovementPlanner {
    /// Generate fix plans for the given candidates.
    ///
    /// Each candidate is sent to the LLM independently. Any provider or parse
    /// failure returns an explicit error immediately.
    pub async fn plan(
        candidates: &[ImprovementCandidate],
        provider: &dyn CompletionProvider,
        repo_root: &Path,
    ) -> Result<Vec<FixPlan>, ImprovementError> {
        let mut plans = Vec::with_capacity(candidates.len());
        for candidate in candidates {
            let plan = plan_single(candidate, provider, repo_root)
                .await
                .map_err(|error| annotate_planning_failure(candidate, error))?;
            plans.push(plan);
        }
        Ok(plans)
    }
}

fn annotate_planning_failure(
    candidate: &ImprovementCandidate,
    error: ImprovementError,
) -> ImprovementError {
    ImprovementError::Planning(format!(
        "candidate '{}' planning failed: {error}",
        candidate.finding.pattern_name
    ))
}

/// Plan a single candidate via LLM tool call.
async fn plan_single(
    candidate: &ImprovementCandidate,
    provider: &dyn CompletionProvider,
    repo_root: &Path,
) -> Result<FixPlan, ImprovementError> {
    let prompt = build_planning_prompt(candidate, repo_root);
    let request = CompletionRequest {
        model: String::new(),
        messages: vec![Message::user(prompt)],
        tools: vec![plan_tool_definition()],
        temperature: None,
        max_tokens: Some(4096),
        system_prompt: Some(PLANNER_SYSTEM_PROMPT.to_string()),
    };

    let response = provider.complete(request).await?;
    parse_plan_from_tool_calls(&response.tool_calls, candidate)
}

/// Build the user prompt describing the candidate for planning.
fn build_planning_prompt(candidate: &ImprovementCandidate, repo_root: &Path) -> String {
    let finding = &candidate.finding;
    let evidence_summary = finding
        .evidence
        .iter()
        .map(|evidence| {
            format!(
                "  - [{}] {}: {}",
                evidence.signal_kind, evidence.session_id, evidence.message
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    let suggested = finding
        .suggested_action
        .as_deref()
        .unwrap_or("(no suggestion)");

    format!(
        "Repository root: {repo}\n\n\
         Pattern: {name}\n\
         Description: {description}\n\
         Confidence: {confidence:?}\n\
         Evidence ({count} signals):\n{evidence}\n\n\
         Suggested action: {suggested}\n\n\
         Call the generate_fix_plan tool with a concrete plan.",
        repo = repo_root.display(),
        name = finding.pattern_name,
        description = finding.description,
        confidence = finding.confidence,
        count = finding.evidence.len(),
        evidence = evidence_summary,
    )
}

/// Tool definition for the LLM to call.
fn plan_tool_definition() -> ToolDefinition {
    ToolDefinition {
        name: PLAN_TOOL_NAME.to_string(),
        description: "Generate a fix plan for an improvement candidate.".to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "additionalProperties": false,
            "required": ["target_files", "fix_description", "risk"],
            "properties": {
                "target_files": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "File paths to modify"
                },
                "fix_description": {
                    "type": "string",
                    "description": "Natural language description of the fix"
                },
                "risk": {
                    "type": "string",
                    "enum": ["low", "medium", "high"],
                    "description": "Risk assessment"
                },
                "code_changes": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "additionalProperties": false,
                        "required": ["path", "description"],
                        "properties": {
                            "path": { "type": "string" },
                            "description": { "type": "string" },
                            "content": { "type": "string" }
                        }
                    },
                    "description": "Concrete file changes (optional)"
                }
            }
        }),
    }
}

/// Parsed tool call arguments.
#[derive(Debug, Deserialize)]
struct PlanToolArgs {
    target_files: Vec<String>,
    fix_description: String,
    risk: RiskLevel,
    code_changes: Option<Vec<FileChangeArgs>>,
}

#[derive(Debug, Deserialize)]
struct FileChangeArgs {
    path: String,
    description: String,
    content: Option<String>,
}

/// Parse the LLM tool call response into a `FixPlan`.
fn parse_plan_from_tool_calls(
    tool_calls: &[ToolCall],
    candidate: &ImprovementCandidate,
) -> Result<FixPlan, ImprovementError> {
    let call = tool_calls
        .iter()
        .find(|tool_call| tool_call.name == PLAN_TOOL_NAME)
        .ok_or_else(|| {
            ImprovementError::Planning("LLM did not call generate_fix_plan tool".to_string())
        })?;

    let args: PlanToolArgs = serde_json::from_value(call.arguments.clone())
        .map_err(|error| ImprovementError::Planning(format!("parse plan args: {error}")))?;

    let code_changes = args.code_changes.map(|changes| {
        changes
            .into_iter()
            .map(|change| FileChange {
                path: PathBuf::from(change.path),
                description: change.description,
                content: change.content,
            })
            .collect()
    });

    Ok(FixPlan {
        candidate: candidate.clone(),
        target_files: args.target_files.into_iter().map(PathBuf::from).collect(),
        fix_description: args.fix_description,
        code_changes,
        risk: args.risk,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use fx_analysis::{AnalysisFinding, Confidence, SignalEvidence};
    use fx_core::signals::SignalKind;
    use fx_llm::{CompletionResponse, CompletionStream, ProviderCapabilities, ProviderError};

    fn mk_candidate(name: &str) -> ImprovementCandidate {
        ImprovementCandidate {
            finding: AnalysisFinding {
                pattern_name: name.to_string(),
                description: "test description".to_string(),
                confidence: Confidence::High,
                evidence: vec![SignalEvidence {
                    session_id: "s1".to_string(),
                    signal_kind: SignalKind::Friction,
                    message: "timeout".to_string(),
                    timestamp_ms: 1,
                }],
                suggested_action: Some("fix it".to_string()),
            },
            fingerprint: format!("fp-{name}"),
        }
    }

    #[derive(Debug)]
    struct MockProvider {
        response: Result<CompletionResponse, ProviderError>,
    }

    #[async_trait]
    impl CompletionProvider for MockProvider {
        async fn complete(
            &self,
            _req: CompletionRequest,
        ) -> Result<CompletionResponse, ProviderError> {
            self.response.clone()
        }

        async fn complete_stream(
            &self,
            _req: CompletionRequest,
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

    fn mock_with_tool_call(args: serde_json::Value) -> MockProvider {
        MockProvider {
            response: Ok(CompletionResponse {
                content: vec![],
                tool_calls: vec![ToolCall {
                    id: "call-1".to_string(),
                    name: PLAN_TOOL_NAME.to_string(),
                    arguments: args,
                }],
                usage: None,
                stop_reason: Some("tool_use".to_string()),
            }),
        }
    }

    #[tokio::test]
    async fn plan_with_no_candidates_returns_empty() {
        let provider = MockProvider {
            response: Ok(CompletionResponse {
                content: vec![],
                tool_calls: vec![],
                usage: None,
                stop_reason: None,
            }),
        };

        let plans = ImprovementPlanner::plan(&[], &provider, Path::new("/repo"))
            .await
            .unwrap();
        assert!(plans.is_empty());
    }

    #[tokio::test]
    async fn plan_generates_fix_plan_from_tool_call() {
        let provider = mock_with_tool_call(serde_json::json!({
            "target_files": ["src/main.rs"],
            "fix_description": "Increase timeout to 30s",
            "risk": "low",
            "code_changes": [{
                "path": "src/main.rs",
                "description": "change timeout constant",
                "content": "const TIMEOUT_MS: u64 = 30000;"
            }]
        }));

        let candidates = vec![mk_candidate("timeout-loop")];
        let plans = ImprovementPlanner::plan(&candidates, &provider, Path::new("/repo"))
            .await
            .unwrap();

        assert_eq!(plans.len(), 1);
        assert_eq!(plans[0].fix_description, "Increase timeout to 30s");
        assert_eq!(plans[0].risk, RiskLevel::Low);
        assert_eq!(plans[0].target_files, vec![PathBuf::from("src/main.rs")]);

        let changes = plans[0].code_changes.as_ref().unwrap();
        assert_eq!(changes.len(), 1);
        assert_eq!(
            changes[0].content.as_deref(),
            Some("const TIMEOUT_MS: u64 = 30000;")
        );
    }

    #[tokio::test]
    async fn plan_without_code_changes() {
        let provider = mock_with_tool_call(serde_json::json!({
            "target_files": ["src/config.rs"],
            "fix_description": "Refactor config",
            "risk": "medium"
        }));

        let candidates = vec![mk_candidate("config-issue")];
        let plans = ImprovementPlanner::plan(&candidates, &provider, Path::new("/repo"))
            .await
            .unwrap();

        assert_eq!(plans.len(), 1);
        assert!(plans[0].code_changes.is_none());
        assert_eq!(plans[0].risk, RiskLevel::Medium);
    }

    #[tokio::test]
    async fn plan_returns_error_on_llm_error() {
        let provider = MockProvider {
            response: Err(ProviderError::Provider("boom".to_string())),
        };
        let candidates = vec![mk_candidate("error-case")];

        let error = ImprovementPlanner::plan(&candidates, &provider, Path::new("/repo"))
            .await
            .expect_err("planning should fail loudly on provider errors");

        assert!(
            matches!(error, ImprovementError::Planning(message) if message.contains("error-case"))
        );
    }

    #[tokio::test]
    async fn plan_returns_error_when_tool_call_missing() {
        let provider = MockProvider {
            response: Ok(CompletionResponse {
                content: vec![],
                tool_calls: vec![],
                usage: None,
                stop_reason: Some("end_turn".to_string()),
            }),
        };
        let candidates = vec![mk_candidate("missing-tool-call")];

        let error = ImprovementPlanner::plan(&candidates, &provider, Path::new("/repo"))
            .await
            .expect_err("missing tool call must fail");

        assert!(
            matches!(error, ImprovementError::Planning(message) if message.contains("generate_fix_plan"))
        );
    }

    #[tokio::test]
    async fn plan_multiple_candidates() {
        let provider = mock_with_tool_call(serde_json::json!({
            "target_files": ["src/lib.rs"],
            "fix_description": "General fix",
            "risk": "high"
        }));

        let candidates = vec![mk_candidate("a"), mk_candidate("b")];
        let plans = ImprovementPlanner::plan(&candidates, &provider, Path::new("/repo"))
            .await
            .unwrap();

        assert_eq!(plans.len(), 2);
        assert_eq!(plans[0].risk, RiskLevel::High);
        assert_eq!(plans[0].candidate.finding.pattern_name, "a");
        assert_eq!(plans[1].candidate.finding.pattern_name, "b");
    }
}
