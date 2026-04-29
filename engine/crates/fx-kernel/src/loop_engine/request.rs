use crate::conversation_compactor::estimate_text_tokens;
use crate::perceive::ProcessedPerception;
use crate::signals::LoopStep;

use fx_llm::{
    CompletionRequest, ContentBlock, Message, MessageRole, PromptCacheAffinity, PromptCachePolicy,
    ToolDefinition,
};
use std::fmt;

use super::{
    message_content_to_text, message_to_text, ExecutionContext, DECOMPOSE_TOOL_DESCRIPTION,
    DECOMPOSE_TOOL_NAME, MEMORY_INSTRUCTION, NOTIFY_TOOL_GUIDANCE, REASONING_MAX_OUTPUT_TOKENS,
    REASONING_SYSTEM_PROMPT, REASONING_TEMPERATURE, SIMPLE_AGENT_FINAL_RESPONSE_DIRECTIVE,
    SIMPLE_AGENT_SYSTEM_PROMPT, TERMINAL_SYNTHESIS_DIRECTIVE, TERMINAL_SYNTHESIS_MAX_OUTPUT_TOKENS,
    TERMINAL_SYNTHESIS_SYSTEM_PROMPT, TOOL_CONTINUATION_DIRECTIVE,
};

const TERMINAL_SYNTHESIS_USER_TOKEN_BUDGET: usize = 2_500;
const TERMINAL_SYNTHESIS_ASSISTANT_TOKEN_BUDGET: usize = 1_500;
const TERMINAL_SYNTHESIS_TOOL_RESULT_TOKEN_BUDGET: usize = 10_000;
const TERMINAL_SYNTHESIS_USER_ENTRY_TOKEN_BUDGET: usize = 1_600;
const TERMINAL_SYNTHESIS_ASSISTANT_ENTRY_TOKEN_BUDGET: usize = 600;
const TERMINAL_SYNTHESIS_TOOL_RESULT_ENTRY_TOKEN_BUDGET: usize = 900;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct SkillPromptSummary {
    name: String,
    description: String,
}

impl SkillPromptSummary {
    pub(super) fn new(name: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
        }
    }

    fn is_usable(&self) -> bool {
        !self.name.trim().is_empty() && !self.description.trim().is_empty()
    }

    fn render_bullet(&self) -> String {
        format!("- {}: {}", self.name.trim(), self.description.trim())
    }
}

impl fmt::Display for SkillPromptSummary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.name.trim(), self.description.trim())
    }
}

#[derive(Clone)]
pub(super) struct RequestBuildContext<'a> {
    memory_context: Option<&'a str>,
    scratchpad_context: Option<&'a str>,
    execution_context: Option<&'a ExecutionContext>,
    agent_preferences: Option<&'a str>,
    thinking: Option<fx_llm::ThinkingConfig>,
    notify_tool_guidance_enabled: bool,
    skill_prompt_summaries: Option<&'a [SkillPromptSummary]>,
    signal_feedback_summary: Option<String>,
    prompt_cache_affinity: Option<PromptCacheAffinity>,
}

impl<'a> RequestBuildContext<'a> {
    pub(super) fn new(
        memory_context: Option<&'a str>,
        scratchpad_context: Option<&'a str>,
        thinking: Option<fx_llm::ThinkingConfig>,
        notify_tool_guidance_enabled: bool,
    ) -> Self {
        Self {
            memory_context,
            scratchpad_context,
            execution_context: None,
            agent_preferences: None,
            thinking,
            notify_tool_guidance_enabled,
            skill_prompt_summaries: None,
            signal_feedback_summary: None,
            prompt_cache_affinity: None,
        }
    }

    pub(super) fn with_skill_prompt_summaries(
        mut self,
        skill_prompt_summaries: &'a [SkillPromptSummary],
    ) -> Self {
        self.skill_prompt_summaries = Some(skill_prompt_summaries);
        self
    }

    pub(super) fn with_execution_context(
        mut self,
        execution_context: Option<&'a ExecutionContext>,
    ) -> Self {
        self.execution_context = execution_context;
        self
    }

    pub(super) fn with_agent_preferences(mut self, agent_preferences: Option<&'a str>) -> Self {
        self.agent_preferences = agent_preferences;
        self
    }

    pub(super) fn with_signal_feedback_summary(mut self, summary: Option<String>) -> Self {
        self.signal_feedback_summary = summary;
        self
    }

    pub(super) fn with_prompt_cache_affinity(
        mut self,
        affinity: Option<PromptCacheAffinity>,
    ) -> Self {
        self.prompt_cache_affinity = affinity;
        self
    }

    fn skill_summaries_slice(&self) -> Option<&[SkillPromptSummary]> {
        self.skill_prompt_summaries
            .filter(|summaries| !summaries.is_empty())
    }
}

pub(super) struct ToolRequestConfig {
    tool_definitions: Vec<ToolDefinition>,
    decompose_enabled: bool,
}

impl ToolRequestConfig {
    pub(super) fn new(tool_definitions: Vec<ToolDefinition>, decompose_enabled: bool) -> Self {
        Self {
            tool_definitions,
            decompose_enabled,
        }
    }

    fn into_tools(self) -> Vec<ToolDefinition> {
        if self.tool_definitions.is_empty() {
            return Vec::new();
        }
        if self.decompose_enabled {
            return tool_definitions_with_decompose(self.tool_definitions);
        }
        self.tool_definitions
    }
}

pub(super) struct ContinuationRequestParams<'a> {
    context_messages: &'a [Message],
    model: &'a str,
    tool_config: ToolRequestConfig,
    context: RequestBuildContext<'a>,
}

impl<'a> ContinuationRequestParams<'a> {
    pub(super) fn new(
        context_messages: &'a [Message],
        model: &'a str,
        tool_config: ToolRequestConfig,
        context: RequestBuildContext<'a>,
    ) -> Self {
        Self {
            context_messages,
            model,
            tool_config,
            context,
        }
    }
}

pub(super) struct ForcedSynthesisRequestParams<'a> {
    context_messages: &'a [Message],
    model: &'a str,
    memory_context: Option<&'a str>,
    scratchpad_context: Option<&'a str>,
    execution_context: Option<&'a ExecutionContext>,
    agent_preferences: Option<&'a str>,
    notify_tool_guidance_enabled: bool,
}

impl<'a> ForcedSynthesisRequestParams<'a> {
    pub(super) fn new(
        context_messages: &'a [Message],
        model: &'a str,
        memory_context: Option<&'a str>,
        scratchpad_context: Option<&'a str>,
        execution_context: Option<&'a ExecutionContext>,
        agent_preferences: Option<&'a str>,
        notify_tool_guidance_enabled: bool,
    ) -> Self {
        Self {
            context_messages,
            model,
            memory_context,
            scratchpad_context,
            execution_context,
            agent_preferences,
            notify_tool_guidance_enabled,
        }
    }
}

pub(super) struct TruncationContinuationRequestParams<'a> {
    model: &'a str,
    continuation_messages: &'a [Message],
    tool_config: ToolRequestConfig,
    context: RequestBuildContext<'a>,
    step: LoopStep,
}

impl<'a> TruncationContinuationRequestParams<'a> {
    pub(super) fn new(
        model: &'a str,
        continuation_messages: &'a [Message],
        tool_config: ToolRequestConfig,
        context: RequestBuildContext<'a>,
        step: LoopStep,
    ) -> Self {
        Self {
            model,
            continuation_messages,
            tool_config,
            context,
            step,
        }
    }
}

pub(super) struct ReasoningRequestParams<'a> {
    perception: &'a ProcessedPerception,
    model: &'a str,
    tool_config: ToolRequestConfig,
    context: RequestBuildContext<'a>,
}

impl<'a> ReasoningRequestParams<'a> {
    pub(super) fn new(
        perception: &'a ProcessedPerception,
        model: &'a str,
        tool_config: ToolRequestConfig,
        context: RequestBuildContext<'a>,
    ) -> Self {
        Self {
            perception,
            model,
            tool_config,
            context,
        }
    }
}

pub(super) struct SimpleAgentRequestParams<'a> {
    perception: &'a ProcessedPerception,
    model: &'a str,
    tools: Vec<ToolDefinition>,
    context: RequestBuildContext<'a>,
    final_response: bool,
}

impl<'a> SimpleAgentRequestParams<'a> {
    pub(super) fn new(
        perception: &'a ProcessedPerception,
        model: &'a str,
        tools: Vec<ToolDefinition>,
        context: RequestBuildContext<'a>,
    ) -> Self {
        Self {
            perception,
            model,
            tools,
            context,
            final_response: false,
        }
    }

    pub(super) fn final_response(mut self) -> Self {
        self.final_response = true;
        self
    }
}

pub(super) fn completion_request_to_prompt(request: &CompletionRequest) -> String {
    let system = request
        .system_prompt
        .as_deref()
        .map(|prompt| format!("System:\n{prompt}\n\n"))
        .unwrap_or_default();
    let messages = request
        .messages
        .iter()
        .map(message_to_text)
        .collect::<Vec<_>>()
        .join("\n");

    format!("{system}{messages}")
}

pub(super) fn build_continuation_request(
    params: ContinuationRequestParams<'_>,
) -> CompletionRequest {
    let system_prompt = build_tool_continuation_system_prompt_with_signal_feedback(
        params.context.memory_context,
        params.context.scratchpad_context,
        params.context.notify_tool_guidance_enabled,
        params.context.skill_summaries_slice(),
        params.context.execution_context,
        params.context.agent_preferences,
        params.context.signal_feedback_summary.as_deref(),
    );
    CompletionRequest {
        model: params.model.to_string(),
        messages: params.context_messages.to_vec(),
        tools: params.tool_config.into_tools(),
        temperature: Some(REASONING_TEMPERATURE),
        max_tokens: Some(REASONING_MAX_OUTPUT_TOKENS),
        system_prompt: Some(system_prompt),
        prompt_cache: PromptCachePolicy::Ephemeral,
        cache_affinity: params.context.prompt_cache_affinity.clone(),
        thinking: params.context.thinking,
    }
}

pub(super) fn build_forced_synthesis_request(
    params: ForcedSynthesisRequestParams<'_>,
) -> CompletionRequest {
    // Forced synthesis is a recovery path, so it keeps the base reasoning
    // prompt shape without runtime skill routing summaries.
    let system_prompt = build_forced_synthesis_system_prompt(
        params.context_messages,
        params.memory_context,
        params.scratchpad_context,
        params.execution_context,
        params.agent_preferences,
        params.notify_tool_guidance_enabled,
    );

    CompletionRequest {
        model: params.model.to_string(),
        messages: terminal_synthesis_messages(params.context_messages),
        tools: vec![],
        temperature: Some(0.3),
        max_tokens: Some(TERMINAL_SYNTHESIS_MAX_OUTPUT_TOKENS),
        system_prompt: Some(system_prompt),
        prompt_cache: PromptCachePolicy::Ephemeral,
        cache_affinity: None,
        thinking: None,
    }
}

pub(super) fn build_truncation_continuation_request(
    params: TruncationContinuationRequestParams<'_>,
) -> CompletionRequest {
    let system_prompt = build_reasoning_system_prompt_with_signal_feedback(
        params.context.memory_context,
        params.context.scratchpad_context,
        params.context.notify_tool_guidance_enabled,
        params.context.skill_summaries_slice(),
        params.context.execution_context,
        params.context.agent_preferences,
        params.context.signal_feedback_summary.as_deref(),
    );

    CompletionRequest {
        model: params.model.to_string(),
        messages: params.continuation_messages.to_vec(),
        tools: continuation_tools_for_step(params.step, params.tool_config.into_tools()),
        temperature: Some(REASONING_TEMPERATURE),
        max_tokens: Some(REASONING_MAX_OUTPUT_TOKENS),
        system_prompt: Some(system_prompt),
        prompt_cache: PromptCachePolicy::Ephemeral,
        cache_affinity: params.context.prompt_cache_affinity.clone(),
        thinking: params.context.thinking,
    }
}

pub(super) fn build_reasoning_request(params: ReasoningRequestParams<'_>) -> CompletionRequest {
    let system_prompt = build_reasoning_system_prompt_with_signal_feedback(
        params.context.memory_context,
        params.context.scratchpad_context,
        params.context.notify_tool_guidance_enabled,
        params.context.skill_summaries_slice(),
        params.context.execution_context,
        params.context.agent_preferences,
        params.context.signal_feedback_summary.as_deref(),
    );

    CompletionRequest {
        model: params.model.to_string(),
        messages: build_reasoning_messages(params.perception),
        tools: params.tool_config.into_tools(),
        temperature: Some(REASONING_TEMPERATURE),
        max_tokens: Some(REASONING_MAX_OUTPUT_TOKENS),
        system_prompt: Some(system_prompt),
        prompt_cache: PromptCachePolicy::Ephemeral,
        cache_affinity: params.context.prompt_cache_affinity.clone(),
        thinking: params.context.thinking,
    }
}

pub(super) fn build_simple_agent_request(
    params: SimpleAgentRequestParams<'_>,
) -> CompletionRequest {
    let mut system_prompt = build_simple_agent_system_prompt(&params.context);
    if params.final_response {
        system_prompt.push_str("\n\n");
        system_prompt.push_str(SIMPLE_AGENT_FINAL_RESPONSE_DIRECTIVE);
    }

    CompletionRequest {
        model: params.model.to_string(),
        messages: build_simple_agent_messages(params.perception),
        tools: if params.final_response {
            Vec::new()
        } else {
            params.tools
        },
        temperature: Some(REASONING_TEMPERATURE),
        max_tokens: Some(REASONING_MAX_OUTPUT_TOKENS),
        system_prompt: Some(system_prompt),
        prompt_cache: PromptCachePolicy::Ephemeral,
        cache_affinity: params.context.prompt_cache_affinity.clone(),
        thinking: params.context.thinking,
    }
}

pub(super) fn build_reasoning_messages(perception: &ProcessedPerception) -> Vec<Message> {
    let user_prompt = reasoning_user_prompt(perception);
    [
        perception.context_window.clone(),
        vec![build_processed_perception_message(perception, &user_prompt)],
    ]
    .concat()
}

pub(super) fn build_simple_agent_messages(perception: &ProcessedPerception) -> Vec<Message> {
    let mut prompt = format!("User request:\n{}", perception.user_message);
    if let Some(steer) = perception.steer_context.as_deref() {
        prompt.push_str(&format!("\n\nLatest user steer:\n{steer}"));
    }
    [
        perception.context_window.clone(),
        vec![build_processed_perception_message(perception, &prompt)],
    ]
    .concat()
}

pub(super) fn reasoning_user_prompt(perception: &ProcessedPerception) -> String {
    let mut prompt = format!(
        "Active goals:\n- {}\n\nBudget remaining: {} tokens, {} llm calls, {} tool calls\n\nUser message:\n{}",
        perception.active_goals.join("\n- "),
        perception.budget_remaining.tokens,
        perception.budget_remaining.llm_calls,
        perception.budget_remaining.tool_invocations,
        perception.user_message,
    );

    if let Some(steer) = perception.steer_context.as_deref() {
        prompt.push_str(&format!("\nUser steer (latest): {steer}"));
    }

    prompt
}

fn build_simple_agent_system_prompt(context: &RequestBuildContext<'_>) -> String {
    let mut sections = vec![SIMPLE_AGENT_SYSTEM_PROMPT.to_string()];

    if let Some(capabilities) = render_skill_capabilities(context.skill_summaries_slice()) {
        sections.push(capabilities);
    }
    if let Some(execution_context) = render_execution_context(context.execution_context) {
        sections.push(execution_context);
    }
    if let Some(preferences) = render_agent_preferences(context.agent_preferences) {
        sections.push(preferences);
    }
    if context.notify_tool_guidance_enabled {
        sections.push(trim_leading_newlines(NOTIFY_TOOL_GUIDANCE).to_string());
    }
    if let Some(scratchpad_context) = context.scratchpad_context {
        sections.push(trim_section_newlines(scratchpad_context).to_string());
    }
    if let Some(memory_context) = context.memory_context {
        let trimmed = trim_section_newlines(memory_context);
        if !trimmed.trim().is_empty() {
            sections.push(format!(
                "{}\n\n{}",
                trimmed,
                trim_leading_newlines(MEMORY_INSTRUCTION)
            ));
        }
    }

    sections.join("\n\n")
}

#[cfg(test)]
pub(super) fn build_reasoning_system_prompt(
    memory_context: Option<&str>,
    scratchpad_context: Option<&str>,
) -> String {
    build_reasoning_system_prompt_with_notify_guidance(
        memory_context,
        scratchpad_context,
        false,
        None,
    )
}

#[cfg(test)]
pub(super) fn build_reasoning_system_prompt_with_notify_guidance(
    memory_context: Option<&str>,
    scratchpad_context: Option<&str>,
    notify_tool_guidance_enabled: bool,
    skill_prompt_summaries: Option<&[SkillPromptSummary]>,
) -> String {
    build_reasoning_system_prompt_with_signal_feedback(
        memory_context,
        scratchpad_context,
        notify_tool_guidance_enabled,
        skill_prompt_summaries,
        None,
        None,
        None,
    )
}

#[cfg(test)]
pub(super) fn build_reasoning_system_prompt_with_agent_preferences(
    agent_preferences: &str,
) -> String {
    build_reasoning_system_prompt_with_signal_feedback(
        None,
        None,
        false,
        None,
        None,
        Some(agent_preferences),
        None,
    )
}

fn build_reasoning_system_prompt_with_signal_feedback(
    memory_context: Option<&str>,
    scratchpad_context: Option<&str>,
    notify_tool_guidance_enabled: bool,
    skill_prompt_summaries: Option<&[SkillPromptSummary]>,
    execution_context: Option<&ExecutionContext>,
    agent_preferences: Option<&str>,
    signal_feedback_summary: Option<&str>,
) -> String {
    build_system_prompt(
        memory_context,
        scratchpad_context,
        None,
        notify_tool_guidance_enabled,
        skill_prompt_summaries,
        execution_context,
        agent_preferences,
        signal_feedback_summary,
    )
}

fn build_forced_synthesis_system_prompt(
    context_messages: &[Message],
    memory_context: Option<&str>,
    scratchpad_context: Option<&str>,
    execution_context: Option<&ExecutionContext>,
    agent_preferences: Option<&str>,
    _notify_tool_guidance_enabled: bool,
) -> String {
    let mut sections = vec![TERMINAL_SYNTHESIS_SYSTEM_PROMPT.to_string()];

    if let Some(context) = render_terminal_execution_context(execution_context) {
        sections.push(context);
    }
    if let Some(preferences) = render_agent_preferences(agent_preferences) {
        sections.push(preferences);
    }
    if let Some(scratchpad_context) = scratchpad_context {
        sections.push(trim_section_newlines(scratchpad_context).to_string());
    }
    if let Some(memory_context) = memory_context {
        let trimmed = trim_section_newlines(memory_context);
        if !trimmed.trim().is_empty() {
            sections.push(trimmed.to_string());
        }
    }

    let directives = system_messages_to_prompt_directives(context_messages);
    if !directives.is_empty() {
        let mut runtime_directives = String::from("Additional runtime directives:");
        for directive in directives {
            runtime_directives.push_str("\n- ");
            runtime_directives.push_str(&directive);
        }
        sections.push(runtime_directives);
    }
    sections.push(trim_leading_newlines(TERMINAL_SYNTHESIS_DIRECTIVE).to_string());
    sections.join("\n\n")
}

#[cfg(test)]
pub(super) fn build_tool_continuation_system_prompt(
    memory_context: Option<&str>,
    scratchpad_context: Option<&str>,
) -> String {
    build_tool_continuation_system_prompt_with_notify_guidance(
        memory_context,
        scratchpad_context,
        false,
        None,
    )
}

#[cfg(test)]
pub(super) fn build_tool_continuation_system_prompt_with_notify_guidance(
    memory_context: Option<&str>,
    scratchpad_context: Option<&str>,
    notify_tool_guidance_enabled: bool,
    skill_prompt_summaries: Option<&[SkillPromptSummary]>,
) -> String {
    // This test-only helper keeps legacy prompt tests focused on the old
    // notify/capability surface; production paths pass execution context via
    // RequestBuildContext.
    build_tool_continuation_system_prompt_with_signal_feedback(
        memory_context,
        scratchpad_context,
        notify_tool_guidance_enabled,
        skill_prompt_summaries,
        None,
        None,
        None,
    )
}

fn build_tool_continuation_system_prompt_with_signal_feedback(
    memory_context: Option<&str>,
    scratchpad_context: Option<&str>,
    notify_tool_guidance_enabled: bool,
    skill_prompt_summaries: Option<&[SkillPromptSummary]>,
    execution_context: Option<&ExecutionContext>,
    agent_preferences: Option<&str>,
    signal_feedback_summary: Option<&str>,
) -> String {
    // Keep this visible to sibling test modules so both prompt paths can
    // exercise the same renderer without duplicating setup.
    build_system_prompt(
        memory_context,
        scratchpad_context,
        Some(TOOL_CONTINUATION_DIRECTIVE),
        notify_tool_guidance_enabled,
        skill_prompt_summaries,
        execution_context,
        agent_preferences,
        signal_feedback_summary,
    )
}

#[allow(clippy::too_many_arguments)]
fn build_system_prompt(
    memory_context: Option<&str>,
    scratchpad_context: Option<&str>,
    extra_directive: Option<&str>,
    notify_tool_guidance_enabled: bool,
    skill_prompt_summaries: Option<&[SkillPromptSummary]>,
    execution_context: Option<&ExecutionContext>,
    agent_preferences: Option<&str>,
    signal_feedback_summary: Option<&str>,
) -> String {
    let mut sections = vec![REASONING_SYSTEM_PROMPT.to_string()];

    if let Some(capabilities) = render_skill_capabilities(skill_prompt_summaries) {
        sections.push(capabilities);
    }
    if let Some(context) = render_execution_context(execution_context) {
        sections.push(context);
    }
    if let Some(preferences) = render_agent_preferences(agent_preferences) {
        sections.push(preferences);
    }
    if let Some(summary) = render_signal_feedback(signal_feedback_summary) {
        sections.push(summary);
    }

    if notify_tool_guidance_enabled {
        sections.push(trim_leading_newlines(NOTIFY_TOOL_GUIDANCE).to_string());
    }
    if let Some(extra_directive) = extra_directive {
        sections.push(trim_leading_newlines(extra_directive).to_string());
    }
    if let Some(scratchpad_context) = scratchpad_context {
        sections.push(trim_section_newlines(scratchpad_context).to_string());
    }
    if let Some(memory_context) = memory_context {
        sections.push(format!(
            "{}\n\n{}",
            trim_section_newlines(memory_context),
            trim_leading_newlines(MEMORY_INSTRUCTION)
        ));
    }

    sections.join("\n\n")
}

fn render_agent_preferences(agent_preferences: Option<&str>) -> Option<String> {
    let trimmed = agent_preferences?.trim();
    (!trimmed.is_empty()).then(|| format!("Configured agent preferences:\n{trimmed}"))
}

fn render_skill_capabilities(
    skill_prompt_summaries: Option<&[SkillPromptSummary]>,
) -> Option<String> {
    let skill_prompt_summaries = skill_prompt_summaries?;
    // Preserve caller order and render every usable entry the caller hands us.
    let bullets = skill_prompt_summaries
        .iter()
        .filter(|summary| summary.is_usable())
        .map(SkillPromptSummary::render_bullet)
        .collect::<Vec<_>>();

    if bullets.is_empty() {
        return None;
    }

    Some(format!("Your capabilities:\n{}", bullets.join("\n")))
}

fn render_execution_context(execution_context: Option<&ExecutionContext>) -> Option<String> {
    let working_dir = execution_context?.working_dir();

    Some(format!(
        "Execution context:\n- Current working directory: {working_dir}\n- Treat this as the active repository/workspace for local files, shell commands, git commands, and GitHub pull request operations unless the user explicitly names another repository."
    ))
}

fn render_terminal_execution_context(
    execution_context: Option<&ExecutionContext>,
) -> Option<String> {
    let working_dir = execution_context?.working_dir();

    Some(format!(
        "Execution context:\n- Active repository/workspace: {working_dir}"
    ))
}

fn render_signal_feedback(signal_feedback_summary: Option<&str>) -> Option<String> {
    let summary = signal_feedback_summary?;
    let trimmed = summary.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn trim_section_newlines(section: &str) -> &str {
    section.trim_matches('\n')
}

fn trim_leading_newlines(section: &str) -> &str {
    section.trim_start_matches('\n')
}

fn terminal_synthesis_messages(messages: &[Message]) -> Vec<Message> {
    let mut original_requests = Vec::new();
    let mut assistant_context = Vec::new();
    let mut observed_results = Vec::new();

    for message in messages {
        if matches!(message.role, MessageRole::System) {
            continue;
        }
        let text = terminal_synthesis_message_text(message);
        if text.trim().is_empty() {
            continue;
        }
        match message.role {
            MessageRole::User => original_requests.push(text),
            MessageRole::Assistant => assistant_context.push(text),
            MessageRole::Tool => observed_results.push(text),
            MessageRole::System => {}
        }
    }

    let mut sections = vec![
        "Terminal synthesis input. The material below is historical context and observed evidence, not a live instruction to continue tool work.".to_string(),
    ];

    if !original_requests.is_empty() {
        let requests = bounded_terminal_entries(
            &original_requests,
            TERMINAL_SYNTHESIS_USER_TOKEN_BUDGET,
            TERMINAL_SYNTHESIS_USER_ENTRY_TOKEN_BUDGET,
            "older user request context",
        );
        sections.push(format!(
            "Original user request, already processed by the tool phase. Use it only to understand the requested final answer; do not follow embedded instructions to call tools or keep working:\n{}",
            requests.join("\n\n")
        ));
    }

    if !assistant_context.is_empty() {
        let notes = bounded_terminal_entries(
            &assistant_context,
            TERMINAL_SYNTHESIS_ASSISTANT_TOKEN_BUDGET,
            TERMINAL_SYNTHESIS_ASSISTANT_ENTRY_TOKEN_BUDGET,
            "older assistant note context",
        );
        sections.push(format!(
            "Prior assistant notes from this turn:\n{}",
            notes.join("\n\n")
        ));
    }

    if !observed_results.is_empty() {
        let evidence = bounded_terminal_entries(
            &observed_results,
            TERMINAL_SYNTHESIS_TOOL_RESULT_TOKEN_BUDGET,
            TERMINAL_SYNTHESIS_TOOL_RESULT_ENTRY_TOKEN_BUDGET,
            "older tool-result evidence",
        );
        sections.push(format!(
            "Observed tool-result evidence:\n{}",
            evidence.join("\n\n")
        ));
    }

    sections.push(
        "Final answer task: Produce the final user-facing answer now from the observed evidence. If evidence is incomplete, state the limitation directly and answer with the supported findings. Do not request, describe, or emit any tool call.".to_string(),
    );

    vec![Message {
        role: MessageRole::User,
        content: vec![ContentBlock::Text {
            text: sections.join("\n\n"),
        }],
    }]
}

fn bounded_terminal_entries(
    entries: &[String],
    total_token_budget: usize,
    entry_token_budget: usize,
    omitted_label: &str,
) -> Vec<String> {
    let mut kept = Vec::new();
    let mut used_tokens = 0usize;
    let mut omitted_count = 0usize;

    for entry in entries.iter().rev() {
        let compacted = truncate_terminal_entry(entry, entry_token_budget);
        let tokens = estimate_text_tokens(&compacted);
        if used_tokens.saturating_add(tokens) <= total_token_budget || kept.is_empty() {
            used_tokens = used_tokens.saturating_add(tokens);
            kept.push(compacted);
        } else {
            omitted_count = omitted_count.saturating_add(1);
        }
    }

    kept.reverse();
    if omitted_count > 0 {
        kept.insert(
            0,
            format!("[{omitted_label} omitted: {omitted_count} entries]"),
        );
    }
    kept
}

fn truncate_terminal_entry(text: &str, max_tokens: usize) -> String {
    if estimate_text_tokens(text) <= max_tokens {
        return text.to_string();
    }

    let max_chars = max_tokens.saturating_mul(4).max(120);
    let head_chars = max_chars.saturating_mul(2) / 3;
    let tail_chars = max_chars.saturating_sub(head_chars).max(40);
    let head = take_chars(text, head_chars);
    let tail = take_last_chars(text, tail_chars);
    let omitted_tokens = estimate_text_tokens(text).saturating_sub(max_tokens);

    format!("{head}\n[... omitted approximately {omitted_tokens} tokens from this evidence item ...]\n{tail}")
}

fn take_chars(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }

    let end = text
        .char_indices()
        .nth(max_chars)
        .map(|(index, _)| index)
        .unwrap_or(text.len());
    text[..end].to_string()
}

fn take_last_chars(text: &str, max_chars: usize) -> String {
    let char_count = text.chars().count();
    if char_count <= max_chars {
        return text.to_string();
    }

    let start_char = char_count.saturating_sub(max_chars);
    let start = text
        .char_indices()
        .nth(start_char)
        .map(|(index, _)| index)
        .unwrap_or(0);
    text[start..].to_string()
}

fn terminal_synthesis_message_text(message: &Message) -> String {
    message
        .content
        .iter()
        .filter_map(terminal_synthesis_content_text)
        .collect::<Vec<_>>()
        .join("\n")
}

fn terminal_synthesis_content_text(block: &ContentBlock) -> Option<String> {
    match block {
        ContentBlock::Text { text } => Some(text.clone()).filter(|text| !text.trim().is_empty()),
        // ToolUse is an intent to gather evidence, not evidence itself. Forced
        // synthesis must only see observed results; otherwise models can parrot
        // pending tool requests as final answers or try to continue executing them.
        ContentBlock::ToolUse { .. } => None,
        ContentBlock::ToolResult {
            tool_use_id,
            content,
        } => Some(format!(
            "Tool result {tool_use_id}: {}",
            terminal_synthesis_tool_result_text(content)
        )),
        ContentBlock::Image { .. } | ContentBlock::Document { .. } => None,
    }
}

fn terminal_synthesis_tool_result_text(content: &serde_json::Value) -> String {
    if let Some(output) = content.get("output").and_then(serde_json::Value::as_str) {
        return output.to_string();
    }
    if let Some(text) = content.as_str() {
        return text.to_string();
    }
    content.to_string()
}

fn system_messages_to_prompt_directives(messages: &[Message]) -> Vec<String> {
    messages
        .iter()
        .filter(|message| message.role == MessageRole::System)
        .map(message_content_to_text)
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())
        .collect()
}

fn build_processed_perception_message(perception: &ProcessedPerception, text: &str) -> Message {
    if perception.images.is_empty() && perception.documents.is_empty() {
        return Message::user(text);
    }
    Message::user_with_attachments(
        text,
        perception.images.clone(),
        perception.documents.clone(),
    )
}

fn continuation_tools_for_step(step: LoopStep, tools: Vec<ToolDefinition>) -> Vec<ToolDefinition> {
    match step {
        LoopStep::Reason => tools,
        _ => Vec::new(),
    }
}

pub(super) fn tool_definitions_with_decompose(
    mut tool_definitions: Vec<ToolDefinition>,
) -> Vec<ToolDefinition> {
    let has_decompose = tool_definitions
        .iter()
        .any(|tool| tool.name == DECOMPOSE_TOOL_NAME);
    if !has_decompose {
        tool_definitions.push(decompose_tool_definition());
    }
    tool_definitions
}

pub(super) fn decompose_tool_definition() -> ToolDefinition {
    ToolDefinition {
        name: DECOMPOSE_TOOL_NAME.to_string(),
        description: DECOMPOSE_TOOL_DESCRIPTION.to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "sub_goals": {
                    "type": "array",
                    "minItems": 1,
                    "maxItems": 5,
                    "items": {
                        "type": "object",
                        "additionalProperties": false,
                        "properties": {
                            "description": {"type": "string", "description": "What this sub-goal should accomplish"},
                            "required_tools": {"type": "array", "items": {"type": "string"}, "description": "Tools needed for this sub-goal"},
                            "expected_output": {"type": "string", "description": "What the result should look like"},
                            "complexity_hint": {
                                "type": "string",
                                "enum": ["Trivial", "Moderate", "Complex"],
                                "description": "Optional complexity hint to guide budget allocation"
                            }
                        },
                        "required": ["description"]
                    },
                    "description": "List of concrete sub-goals to execute."
                },
                "strategy": {
                    "type": "string",
                    "enum": ["Sequential", "Parallel"],
                    "description": "Execution strategy for the listed sub-goals."
                }
            },
            "required": ["sub_goals"]
        }),
    }
}
