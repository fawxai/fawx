use super::*;
use crate::budget::ActionCost;
use crate::conversation_compactor::estimate_text_tokens;
use async_trait::async_trait;
use fx_core::error::LlmError as CoreLlmError;
use fx_core::types::{InputSource, ScreenState, UserInput};
use fx_llm::{CompletionResponse, ContentBlock, Message, ProviderError, ToolCall, ToolDefinition};
use std::collections::VecDeque;
use std::sync::Mutex;

#[derive(Debug, Default)]
struct TestStubToolExecutor;

#[async_trait]
impl ToolExecutor for TestStubToolExecutor {
    async fn execute_tools(
        &self,
        calls: &[ToolCall],
        _cancel: Option<&CancellationToken>,
    ) -> Result<Vec<ToolResult>, crate::act::ToolExecutorError> {
        Ok(calls
            .iter()
            .map(|call| ToolResult {
                tool_call_id: call.id.clone(),
                tool_name: call.name.clone(),
                success: true,
                output: "ok".to_string(),
                failure_class: None,
            })
            .collect())
    }

    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        vec![ToolDefinition {
            name: "read_file".to_string(),
            description: "Read a file".to_string(),
            parameters: serde_json::json!({"type":"object"}),
        }]
    }
}

#[derive(Debug)]
struct MockLlm {
    responses: Mutex<VecDeque<CompletionResponse>>,
}

impl MockLlm {
    fn new(responses: Vec<CompletionResponse>) -> Self {
        Self {
            responses: Mutex::new(VecDeque::from(responses)),
        }
    }
}

#[async_trait]
impl LlmProvider for MockLlm {
    async fn generate(&self, _: &str, _: u32) -> Result<String, CoreLlmError> {
        Ok("summary".to_string())
    }

    async fn generate_streaming(
        &self,
        _: &str,
        _: u32,
        callback: Box<dyn Fn(String) + Send + 'static>,
    ) -> Result<String, CoreLlmError> {
        callback("summary".to_string());
        Ok("summary".to_string())
    }

    fn model_name(&self) -> &str {
        "mock"
    }

    async fn complete(&self, _: CompletionRequest) -> Result<CompletionResponse, ProviderError> {
        self.responses
            .lock()
            .expect("lock")
            .pop_front()
            .ok_or_else(|| ProviderError::Provider("no response".to_string()))
    }
}

fn default_engine() -> LoopEngine {
    engine_with_budget(crate::budget::BudgetConfig::default())
}

fn engine_with_budget(config: crate::budget::BudgetConfig) -> LoopEngine {
    LoopEngine::builder()
        .budget(BudgetTracker::new(config, 0, 0))
        .context(ContextCompactor::new(2048, 256))
        .max_iterations(3)
        .tool_executor(Arc::new(TestStubToolExecutor))
        .synthesis_instruction("Summarize tool output".to_string())
        .build()
        .expect("test engine build")
}

fn base_snapshot(text: &str) -> PerceptionSnapshot {
    PerceptionSnapshot {
        timestamp_ms: 1,
        screen: ScreenState {
            current_app: "terminal".to_string(),
            elements: Vec::new(),
            text_content: text.to_string(),
        },
        notifications: Vec::new(),
        active_app: "terminal".to_string(),
        user_input: Some(UserInput {
            text: text.to_string(),
            source: InputSource::Text,
            timestamp: 1,
            context_id: None,
            images: Vec::new(),
            documents: Vec::new(),
        }),
        sensor_data: None,
        conversation_history: vec![Message::user(text)],
        steer_context: None,
    }
}

fn processed_perception(text: &str) -> ProcessedPerception {
    ProcessedPerception {
        user_message: text.to_string(),
        images: Vec::new(),
        documents: Vec::new(),
        context_window: vec![Message::user(text)],
        active_goals: vec![format!("Help the user with: {text}")],
        budget_remaining: BudgetRemaining {
            llm_calls: 10,
            tool_invocations: 10,
            tokens: 10_000,
            cost_cents: 100,
            wall_time_ms: 10_000,
        },
        steer_context: None,
    }
}

fn repeated_blocked_signal(tool: &str, id: u64) -> Signal {
    Signal::new(
        LoopStep::Act,
        SignalKind::Blocked,
        format!("tool '{tool}' blocked: same call already failed permanently"),
        serde_json::json!({
            "tool": tool,
            "failure_class": "permanent",
            "permanent": true,
        }),
        id,
    )
    .with_id(id)
}

#[test]
fn system_prompt_includes_tool_use_guidance() {
    let prompt = build_reasoning_system_prompt(None, None);
    assert!(prompt.contains("Use tools when you need information not already in the conversation"));
    assert!(
            prompt.contains(
                "When the user's request relates to an available tool's purpose, prefer calling the tool"
            ),
            "system prompt should encourage proactive tool usage for matching requests"
        );
}

#[test]
fn system_prompt_prohibits_greeting_and_preamble() {
    let prompt = build_reasoning_system_prompt(None, None);
    assert!(
        prompt.contains("Never introduce yourself"),
        "system prompt must prohibit self-introduction (issue #959)"
    );
    assert!(
        prompt.contains("greet the user"),
        "system prompt must prohibit greeting (issue #959)"
    );
}

#[test]
fn system_prompt_without_memory_omits_persistent_memory_block() {
    let prompt = build_reasoning_system_prompt(None, None);
    assert!(
        !prompt.contains("You have persistent memory across sessions"),
        "system prompt without memory context should NOT include the persistent memory block"
    );
}

#[test]
fn system_prompt_omits_notify_guidance_without_notification_channel() {
    let prompt = build_reasoning_system_prompt(None, None);
    assert!(
        !prompt.contains("You have a `notify` tool"),
        "system prompt should omit notify guidance when no notification channel is active"
    );
}

#[test]
fn system_prompt_includes_notify_guidance_when_notification_channel_is_active() {
    let prompt = build_reasoning_system_prompt_with_notify_guidance(None, None, true, None);
    assert!(
        prompt.contains("You have a `notify` tool"),
        "system prompt should include notify guidance when notifications are available"
    );
}

#[test]
fn system_prompt_includes_agent_preferences_when_configured() {
    let prompt = build_reasoning_system_prompt_with_agent_preferences(
        "Personality:\nBe task-first.\n\nBehavioral:\nPrefer concrete next steps.",
    );

    assert!(prompt.contains("Configured agent preferences:"));
    assert!(prompt.contains("Personality:\nBe task-first."));
    assert!(prompt.contains("Behavioral:\nPrefer concrete next steps."));
}

#[test]
fn system_prompt_renders_skill_capabilities_in_input_order() {
    let summaries = vec![
        SkillPromptSummary::new(
            "git",
            "Inspect and manage git repositories, branches, merges, pushes, and PR creation.",
        ),
        SkillPromptSummary::new("github", "View and manage pull requests and issues."),
    ];

    let prompt =
        build_reasoning_system_prompt_with_notify_guidance(None, None, false, Some(&summaries));

    let git_index = prompt
        .find("- git: Inspect and manage git repositories, branches, merges, pushes, and PR creation.")
        .expect("prompt should include git summary");
    let github_index = prompt
        .find("- github: View and manage pull requests and issues.")
        .expect("prompt should include github summary");

    assert!(prompt.contains("Your capabilities:"));
    assert!(
        git_index < github_index,
        "summaries should render in input order"
    );
}

#[test]
fn system_prompt_skips_blank_skill_descriptions() {
    let summaries = vec![
        SkillPromptSummary::new("git", "   "),
        SkillPromptSummary::new("github", "View and manage pull requests and issues."),
    ];

    let prompt =
        build_reasoning_system_prompt_with_notify_guidance(None, None, false, Some(&summaries));

    assert!(
        !prompt.contains("- git:"),
        "blank skill descriptions should be omitted"
    );
    assert!(
        prompt.contains("- github: View and manage pull requests and issues."),
        "valid skill summaries should still render"
    );
}

#[test]
fn system_prompt_omits_skill_capabilities_when_no_usable_summaries_exist() {
    let blank_only = vec![
        SkillPromptSummary::new("git", " "),
        SkillPromptSummary::new("github", "\n\t"),
    ];

    let absent_prompt = build_reasoning_system_prompt_with_notify_guidance(None, None, false, None);
    let blank_prompt =
        build_reasoning_system_prompt_with_notify_guidance(None, None, false, Some(&blank_only));

    assert!(
        !absent_prompt.contains("Your capabilities:"),
        "capabilities section should be omitted when no summaries are provided"
    );
    assert!(
        !blank_prompt.contains("Your capabilities:"),
        "capabilities section should be omitted when all summaries are blank"
    );
}

#[test]
fn reasoning_and_tool_continuation_prompts_share_skill_capability_rendering() {
    let summaries = vec![
        SkillPromptSummary::new("git", "Inspect and manage git repositories."),
        SkillPromptSummary::new("github", "View and manage pull requests and issues."),
    ];

    let reasoning_prompt =
        build_reasoning_system_prompt_with_notify_guidance(None, None, false, Some(&summaries));
    let continuation_prompt = build_tool_continuation_system_prompt_with_notify_guidance(
        None,
        None,
        false,
        Some(&summaries),
    );

    assert!(reasoning_prompt.contains("Your capabilities:"));
    assert!(continuation_prompt.contains("Your capabilities:"));
    assert!(reasoning_prompt.contains("- git: Inspect and manage git repositories."));
    assert!(continuation_prompt.contains("- github: View and manage pull requests and issues."));
}

#[test]
fn skill_capabilities_do_not_introduce_triple_blank_lines() {
    let summaries = vec![SkillPromptSummary::new(
        "git",
        "Inspect and manage git repositories, branches, merges, pushes, and PR creation.",
    )];

    let prompt = build_reasoning_system_prompt_with_notify_guidance(
        Some("Persistent memory about the user"),
        Some("Scratchpad context"),
        true,
        Some(&summaries),
    );

    assert!(
        !prompt.contains("\n\n\n"),
        "system prompt should not contain triple blank lines"
    );
    assert!(prompt.contains("Your capabilities:"));
    assert!(prompt.contains("Persistent memory about the user"));
    assert!(prompt.contains("Scratchpad context"));
}

#[test]
fn system_prompt_with_memory_includes_memory_instruction() {
    let prompt = build_reasoning_system_prompt(Some("user prefers dark mode"), None);
    assert!(
        prompt.contains("memory_write"),
        "system prompt with memory context should mention memory_write via MEMORY_INSTRUCTION"
    );
    assert!(
        prompt.contains("user prefers dark mode"),
        "system prompt should include the memory context"
    );
}

/// Regression test: tool definitions must NOT appear as text in the system
/// prompt. They are already provided via the structured `tools` field of
/// `CompletionRequest`. Duplicating them in the system prompt caused 9×
/// token bloat on OpenAI and broke multi-step instruction following.
#[test]
fn system_prompt_does_not_contain_tool_descriptions() {
    let prompt = build_reasoning_system_prompt(None, None);
    assert!(
        !prompt.contains("Available tools:"),
        "system prompt must not contain 'Available tools:' text — \
             tool definitions belong in the structured tools field, not the prompt"
    );

    // Also verify with memory context (second code path).
    let prompt_with_memory = build_reasoning_system_prompt(Some("user likes cats"), None);
    assert!(
        !prompt_with_memory.contains("Available tools:"),
        "system prompt with memory must not contain 'Available tools:' text"
    );
}

#[test]
fn tool_continuation_prompt_prioritizes_answering_from_existing_results() {
    let prompt = build_tool_continuation_system_prompt(None, None);
    assert!(
        prompt.contains("Treat successful tool results as the primary evidence"),
        "tool continuation prompt should prioritize existing tool results"
    );
    assert!(
        prompt.contains("answer immediately instead of calling more tools"),
        "tool continuation prompt should prefer answering once results suffice"
    );
    assert!(
        prompt.contains("Never repeat an identical successful tool call in the same cycle"),
        "tool continuation prompt should discourage redundant tool retries"
    );
    assert!(
        prompt.contains("one short live work narration update"),
        "tool continuation prompt should ask for live narration before more tools"
    );
}

#[test]
fn reasoning_prompt_separates_live_narration_from_final_answers() {
    let prompt = build_reasoning_system_prompt(None, None);
    assert!(
        prompt.contains("assistant text immediately before tool calls"),
        "reasoning prompt should define the live narration channel"
    );
    assert!(
        prompt.contains("In final answers, never narrate what tools you used"),
        "reasoning prompt should keep process narration out of final answers"
    );
}

#[test]
fn continuation_request_includes_tool_continuation_directive_once() {
    let skill_summaries = vec![SkillPromptSummary::new(
        "git",
        "Inspect and manage git repositories.",
    )];
    let request = build_continuation_request(ContinuationRequestParams::new(
        &[Message::assistant("intermediate")],
        "mock-model",
        ToolRequestConfig::new(vec![], true),
        RequestBuildContext::new(None, None, None, false)
            .with_skill_prompt_summaries(&skill_summaries),
    ));
    let prompt = request
        .system_prompt
        .expect("continuation request should include a system prompt");
    assert!(
        prompt.contains("Your capabilities:"),
        "continuation request should include the capability summary section"
    );
    assert_eq!(
        prompt.matches(TOOL_CONTINUATION_DIRECTIVE).count(),
        1,
        "continuation request should include the tool continuation directive exactly once"
    );
}

#[test]
fn simple_agent_request_omits_legacy_harness_contracts() {
    let perception = processed_perception("please fix these code review issues");
    let request = build_simple_agent_request(SimpleAgentRequestParams::new(
        &perception,
        "mock-model",
        vec![ToolDefinition {
            name: "edit_file".to_string(),
            description: "Edit a file".to_string(),
            parameters: serde_json::json!({"type":"object"}),
        }],
        RequestBuildContext::new(None, None, None, false),
    ));

    let system = request.system_prompt.clone().expect("system prompt");
    assert!(system.contains("recommendations are not a resolution"));
    assert!(!system.contains("Task lifecycle"));
    assert!(!system.contains("Root turn completion contract"));
    assert!(!system.contains("decompose first"));

    let prompt = completion_request_to_prompt(&request);
    assert!(prompt.contains("User request:\nplease fix these code review issues"));
    assert!(!prompt.contains("Active goals:"));
    assert!(!prompt.contains("Budget remaining:"));
}

#[test]
fn reasoning_request_injects_signal_feedback_at_prompt_seam() {
    let request = build_reasoning_request(ReasoningRequestParams::new(
        &processed_perception("Fix the failing command"),
        "mock-model",
        ToolRequestConfig::new(vec![], true),
        RequestBuildContext::new(None, None, None, false).with_signal_feedback_summary(Some(
            "Recent signal guidance:\n- Avoid retrying `run_command`; it has failed repeatedly with permanent errors.".to_string(),
        )),
    ));
    let prompt = request.system_prompt.expect("system prompt");

    assert!(prompt.contains("Recent signal guidance:"));
    assert!(prompt.contains("Avoid retrying `run_command`"));
}

#[test]
fn engine_history_injects_signal_feedback_when_actionable() {
    let mut engine = default_engine();
    engine.recent_signal_cycles.push_back(vec![
        repeated_blocked_signal("run_command", 1),
        repeated_blocked_signal("run_command", 2),
    ]);
    engine.iteration_count = 1;

    let request = build_reasoning_request(ReasoningRequestParams::new(
        &processed_perception("Fix the failing command"),
        "mock-model",
        ToolRequestConfig::new(vec![], true),
        engine.request_build_context(),
    ));
    let prompt = request.system_prompt.expect("system prompt");

    assert!(prompt.contains("Recent signal guidance:"));
    assert!(prompt.contains("Avoid retrying `run_command`"));
}

#[test]
fn engine_history_can_disable_signal_feedback_by_config() {
    let mut config = crate::budget::BudgetConfig::default();
    config.signal_feedback.enabled = false;
    let mut engine = engine_with_budget(config);
    engine.recent_signal_cycles.push_back(vec![
        repeated_blocked_signal("run_command", 1),
        repeated_blocked_signal("run_command", 2),
    ]);
    engine.iteration_count = 1;

    let request = build_reasoning_request(ReasoningRequestParams::new(
        &processed_perception("Fix the failing command"),
        "mock-model",
        ToolRequestConfig::new(vec![], true),
        engine.request_build_context(),
    ));
    let prompt = request.system_prompt.expect("system prompt");

    assert!(!prompt.contains("Recent signal guidance:"));
}

#[test]
fn forced_synthesis_request_flattens_observed_tool_results_into_text_evidence() {
    let messages = vec![
        Message {
            role: fx_llm::MessageRole::Assistant,
            content: vec![ContentBlock::ToolUse {
                id: "call-1".to_string(),
                provider_id: None,
                name: "read_file".to_string(),
                input: serde_json::json!({"path":"README.md"}),
            }],
        },
        Message {
            role: fx_llm::MessageRole::Tool,
            content: vec![ContentBlock::ToolResult {
                tool_use_id: "call-1".to_string(),
                content: serde_json::json!({
                    "success": true,
                    "output": "README evidence"
                }),
            }],
        },
    ];

    let request = build_forced_synthesis_request(ForcedSynthesisRequestParams::new(
        &messages,
        "mock-model",
        None,
        None,
        None,
        None,
        false,
    ));

    assert!(request.tools.is_empty());
    assert!(
        request
            .messages
            .iter()
            .all(|message| message.role != fx_llm::MessageRole::Tool),
        "terminal synthesis must not continue the provider tool protocol"
    );
    let flattened = request
        .messages
        .iter()
        .flat_map(|message| message.content.iter())
        .map(|block| match block {
            ContentBlock::Text { text } => text.as_str(),
            other => panic!("terminal synthesis should contain only text blocks, got {other:?}"),
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(!flattened.contains("Tool request: read_file"));
    assert!(flattened.contains("README evidence"));
    let prompt = request.system_prompt.as_deref().unwrap_or_default();
    assert!(
        prompt.contains("terminal answer mode"),
        "forced synthesis should use a terminal-only system prompt"
    );
    assert!(
        !prompt.contains("Use tools when you need information"),
        "forced synthesis must not inherit the normal tool-using prompt"
    );
}

#[test]
fn forced_synthesis_request_excludes_unexecuted_tool_requests_from_evidence() {
    let messages = vec![Message {
        role: fx_llm::MessageRole::Assistant,
        content: vec![ContentBlock::ToolUse {
            id: "call-1".to_string(),
            provider_id: None,
            name: "run_command".to_string(),
            input: serde_json::json!({
                "command": "grep -n \"reduceStream\\|StreamReduction\\|activity\\|narration\" /Users/joseph/fawx/app/Fawx/ViewModels/ChatViewModel.swift | head -50",
                "shell": true
            }),
        }],
    }];

    let request = build_forced_synthesis_request(ForcedSynthesisRequestParams::new(
        &messages,
        "mock-model",
        None,
        None,
        None,
        None,
        false,
    ));

    let flattened = request
        .messages
        .iter()
        .flat_map(|message| message.content.iter())
        .map(|block| match block {
            ContentBlock::Text { text } => text.as_str(),
            other => panic!("terminal synthesis should contain only text blocks, got {other:?}"),
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(!flattened.contains("Tool request:"));
    assert!(!flattened.contains("run_command"));
    assert!(!flattened.contains("grep -n"));
    assert!(!flattened.contains("reduceStream"));
}

#[test]
fn forced_synthesis_request_compacts_terminal_evidence_pack() {
    let mut messages = vec![Message::user(
        "Inspect the transcript UI and return what was inspected, the architecture, and recommendations.",
    )];
    for index in 0..24 {
        messages.push(Message {
            role: fx_llm::MessageRole::Tool,
            content: vec![ContentBlock::ToolResult {
                tool_use_id: format!("call-{index}"),
                content: serde_json::json!({
                    "success": true,
                    "output": format!("evidence-{index}\n{}", "large terminal synthesis evidence ".repeat(700))
                }),
            }],
        });
    }

    let request = build_forced_synthesis_request(ForcedSynthesisRequestParams::new(
        &messages,
        "mock-model",
        None,
        None,
        None,
        None,
        false,
    ));

    let flattened = request
        .messages
        .iter()
        .flat_map(|message| message.content.iter())
        .map(|block| match block {
            ContentBlock::Text { text } => text.as_str(),
            other => panic!("terminal synthesis should contain only text blocks, got {other:?}"),
        })
        .collect::<Vec<_>>()
        .join("\n");

    assert!(
        flattened.contains("[older tool-result evidence omitted:"),
        "terminal synthesis should make evidence compaction explicit"
    );
    assert!(
        !flattened.contains("evidence-0"),
        "oldest oversized evidence should be omitted rather than starving final synthesis"
    );
    assert!(
        flattened.contains("evidence-23"),
        "latest evidence should remain available for the final answer"
    );
    assert!(
        estimate_text_tokens(&flattened) <= 13_000,
        "terminal evidence pack should stay bounded, got {} estimated tokens",
        estimate_text_tokens(&flattened)
    );
}

#[test]
fn low_budget_suppresses_signal_feedback_injection() {
    let mut config = crate::budget::BudgetConfig::default();
    config.max_llm_calls = 4;
    let mut engine = engine_with_budget(config.clone());
    engine.recent_signal_cycles.push_back(vec![
        repeated_blocked_signal("run_command", 1),
        repeated_blocked_signal("run_command", 2),
    ]);
    engine.budget.record(&ActionCost {
        llm_calls: config.max_llm_calls,
        ..ActionCost::default()
    });
    engine.iteration_count = 1;

    let request = build_reasoning_request(ReasoningRequestParams::new(
        &processed_perception("Fix the failing command"),
        "mock-model",
        ToolRequestConfig::new(vec![], true),
        engine.request_build_context(),
    ));
    let prompt = request.system_prompt.expect("system prompt");

    assert_eq!(engine.budget.state(), BudgetState::Low);
    assert!(!prompt.contains("Recent signal guidance:"));
}

#[test]
fn zero_lookback_clears_signal_feedback_history() {
    let mut config = crate::budget::BudgetConfig::default();
    config.signal_feedback.lookback_cycles = 0;
    let mut engine = engine_with_budget(config);
    engine
        .recent_signal_cycles
        .push_back(vec![repeated_blocked_signal("run_command", 1)]);

    engine.record_signal_feedback_cycle(&[repeated_blocked_signal("run_command", 2)]);

    assert!(engine.recent_signal_cycles.is_empty());
}

#[test]
fn tool_synthesis_prompt_content_is_complete() {
    let results = vec![ToolResult {
        tool_call_id: "call-1".to_string(),
        tool_name: "current_time".to_string(),
        output: "2026-02-28T14:00:00Z".to_string(),
        success: true,
        failure_class: None,
    }];
    let prompt = tool_synthesis_prompt(&results, "Tell the user the time.");
    assert!(
        prompt.contains("You are Fawx"),
        "synthesis prompt must include assistant identity"
    );
    assert!(
        prompt.contains("Answer the user's question using these tool results"),
        "synthesis prompt must instruct direct answering"
    );
    assert!(
        prompt.contains("Do NOT describe what tools were called"),
        "synthesis prompt must block meta-narration"
    );
    assert!(
        prompt.contains(
            "If the user asked for a specific format or value type, preserve that exact format."
        ),
        "synthesis prompt must preserve requested output formats"
    );
    assert!(
            prompt.contains(
                "Do not convert timestamps to human-readable, counts to lists, or raw values to prose unless the user explicitly asked for that."
            ),
            "synthesis prompt must forbid format rewriting"
        );
    assert!(
        prompt.contains("Tell the user the time."),
        "synthesis prompt must include the instruction"
    );
    assert!(
        prompt.contains("current_time: 2026-02-28T14:00:00Z"),
        "synthesis prompt must include tool results"
    );
}

#[test]
fn tool_synthesis_prompt_explicitly_prohibits_intro_and_greeting() {
    let prompt = tool_synthesis_prompt(&[], "Combine outputs");
    assert!(
        prompt.contains("Never introduce yourself, greet the user, or add preamble"),
        "synthesis prompt should mirror no-intro guidance from reasoning prompt"
    );
}

#[test]
fn synthesis_includes_all_results() {
    let results = vec![
        ToolResult {
            tool_call_id: "call-1".to_string(),
            tool_name: "read_file".to_string(),
            output: "alpha".to_string(),
            success: true,
            failure_class: None,
        },
        ToolResult {
            tool_call_id: "call-2".to_string(),
            tool_name: "search".to_string(),
            output: "beta".to_string(),
            success: true,
            failure_class: None,
        },
    ];

    let prompt = tool_synthesis_prompt(&results, "Combine outputs");

    assert!(prompt.contains("read_file: alpha"));
    assert!(prompt.contains("search: beta"));

    let tool_results_section = prompt
        .split("Tool results:\n")
        .nth(1)
        .expect("prompt should include tool results section");
    let result_count = tool_results_section
        .lines()
        .take_while(|line| !line.trim().is_empty())
        .filter(|line| line.starts_with("- "))
        .count();
    assert_eq!(
        result_count, 2,
        "prompt should include exactly 2 tool results"
    );
}

#[test]
fn synthesis_includes_failed_tool_results() {
    let results = vec![
        ToolResult {
            tool_call_id: "call-1".to_string(),
            tool_name: "read_file".to_string(),
            output: "alpha".to_string(),
            success: true,
            failure_class: None,
        },
        ToolResult {
            tool_call_id: "call-2".to_string(),
            tool_name: "run_command".to_string(),
            output: "permission denied".to_string(),
            success: false,
            failure_class: None,
        },
    ];

    let prompt = tool_synthesis_prompt(&results, "Combine outputs");

    assert!(prompt.contains("read_file: alpha"));
    assert!(prompt.contains("run_command: permission denied"));
}

#[test]
fn synthesis_prompt_includes_error_relay_instruction_when_tool_failed() {
    let results = vec![ToolResult {
        tool_call_id: "call-1".to_string(),
        tool_name: "read_file".to_string(),
        output: "file not found: /foo/bar".to_string(),
        success: false,
        failure_class: None,
    }];

    let prompt = tool_synthesis_prompt(&results, "Combine outputs");

    assert!(prompt.contains("If any tool returned an error, tell the user exactly what went wrong: include the actual error message. Do not soften, hedge, or paraphrase errors."));
}

#[test]
fn synthesis_prompt_omits_error_relay_when_all_tools_succeed() {
    let results = vec![ToolResult {
        tool_call_id: "call-1".to_string(),
        tool_name: "read_file".to_string(),
        output: "alpha".to_string(),
        success: true,
        failure_class: None,
    }];

    let prompt = tool_synthesis_prompt(&results, "Combine outputs");

    assert!(!prompt.contains("If any tool returned an error, tell the user exactly what went wrong: include the actual error message. Do not soften, hedge, or paraphrase errors."));
}

#[test]
fn synthesis_prompt_error_relay_with_mixed_results() {
    let results = vec![
        ToolResult {
            tool_call_id: "call-1".to_string(),
            tool_name: "read_file".to_string(),
            output: "alpha".to_string(),
            success: true,
            failure_class: None,
        },
        ToolResult {
            tool_call_id: "call-2".to_string(),
            tool_name: "run_command".to_string(),
            output: "permission denied".to_string(),
            success: false,
            failure_class: None,
        },
    ];

    let prompt = tool_synthesis_prompt(&results, "Combine outputs");

    assert!(prompt.contains("If any tool returned an error, tell the user exactly what went wrong: include the actual error message. Do not soften, hedge, or paraphrase errors."));
}

#[test]
fn synthesis_prompt_handles_empty_tool_results() {
    let prompt = tool_synthesis_prompt(&[], "Combine outputs");

    assert!(!prompt.contains("If any tool returned an error, tell the user exactly what went wrong: include the actual error message. Do not soften, hedge, or paraphrase errors."));
    assert!(prompt.contains("Tool results:\n"));
}

#[tokio::test]
async fn reason_returns_completion_response_with_tool_calls() {
    let mut engine = default_engine();
    let llm = MockLlm::new(vec![CompletionResponse {
        content: Vec::new(),
        tool_calls: vec![ToolCall {
            id: "1".to_string(),
            name: "read_file".to_string(),
            arguments: serde_json::json!({"path":"Cargo.toml"}),
        }],
        usage: None,
        stop_reason: None,
    }]);

    let perception = engine
        .perceive(&base_snapshot("read"))
        .await
        .expect("perceive");
    let response = engine
        .reason(&perception, &llm, CycleStream::disabled())
        .await
        .expect("reason");
    assert_eq!(response.tool_calls.len(), 1);
}

#[tokio::test]
async fn decide_maps_text_response_to_respond_decision() {
    let mut engine = default_engine();
    let response = CompletionResponse {
        content: vec![ContentBlock::Text {
            text: "hello".to_string(),
        }],
        tool_calls: Vec::new(),
        usage: None,
        stop_reason: None,
    };
    let decision = engine.decide(&response).await.expect("decision");
    assert!(matches!(decision, Decision::Respond(text) if text == "hello"));
}

#[tokio::test]
async fn decide_extracts_single_tool_call() {
    let mut engine = default_engine();
    let response = CompletionResponse {
        content: vec![ContentBlock::Text {
            text: "ignore me".to_string(),
        }],
        tool_calls: vec![ToolCall {
            id: "1".to_string(),
            name: "read_file".to_string(),
            arguments: serde_json::json!({"path":"Cargo.toml"}),
        }],
        usage: None,
        stop_reason: None,
    };
    let decision = engine.decide(&response).await.expect("decision");
    assert!(matches!(decision, Decision::UseTools(calls) if calls.len() == 1));
}

#[tokio::test]
async fn decide_no_tool_calls_returns_empty_response() {
    let mut engine = default_engine();
    let response = CompletionResponse {
        content: Vec::new(),
        tool_calls: Vec::new(),
        usage: None,
        stop_reason: None,
    };
    let decision = engine.decide(&response).await.expect("decision");
    assert!(matches!(decision, Decision::Respond(text) if text.is_empty()));
}
