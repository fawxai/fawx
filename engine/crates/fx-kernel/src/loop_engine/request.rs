use crate::perceive::ProcessedPerception;
use crate::signals::LoopStep;

use fx_llm::{CompletionRequest, Message, MessageRole, ToolDefinition};
use std::fmt;

use super::{
    message_content_to_text, message_to_text, BUDGET_EXHAUSTED_SYNTHESIS_DIRECTIVE,
    DECOMPOSE_TOOL_DESCRIPTION, DECOMPOSE_TOOL_NAME, MEMORY_INSTRUCTION, NOTIFY_TOOL_GUIDANCE,
    REASONING_MAX_OUTPUT_TOKENS, REASONING_SYSTEM_PROMPT, REASONING_TEMPERATURE,
    TOOL_CONTINUATION_DIRECTIVE,
};

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
    thinking: Option<fx_llm::ThinkingConfig>,
    notify_tool_guidance_enabled: bool,
    skill_prompt_summaries: Option<&'a [SkillPromptSummary]>,
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
            thinking,
            notify_tool_guidance_enabled,
            skill_prompt_summaries: None,
        }
    }

    pub(super) fn with_skill_prompt_summaries(
        mut self,
        skill_prompt_summaries: &'a [SkillPromptSummary],
    ) -> Self {
        self.skill_prompt_summaries = Some(skill_prompt_summaries);
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
    notify_tool_guidance_enabled: bool,
}

impl<'a> ForcedSynthesisRequestParams<'a> {
    pub(super) fn new(
        context_messages: &'a [Message],
        model: &'a str,
        memory_context: Option<&'a str>,
        scratchpad_context: Option<&'a str>,
        notify_tool_guidance_enabled: bool,
    ) -> Self {
        Self {
            context_messages,
            model,
            memory_context,
            scratchpad_context,
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
    let system_prompt = build_tool_continuation_system_prompt_with_notify_guidance(
        params.context.memory_context,
        params.context.scratchpad_context,
        params.context.notify_tool_guidance_enabled,
        params.context.skill_summaries_slice(),
    );
    CompletionRequest {
        model: params.model.to_string(),
        messages: params.context_messages.to_vec(),
        tools: params.tool_config.into_tools(),
        temperature: Some(REASONING_TEMPERATURE),
        max_tokens: Some(REASONING_MAX_OUTPUT_TOKENS),
        system_prompt: Some(system_prompt),
        thinking: params.context.thinking,
    }
}

pub(super) fn build_forced_synthesis_request(
    params: ForcedSynthesisRequestParams<'_>,
) -> CompletionRequest {
    // Forced synthesis is a recovery path, so it keeps the base reasoning
    // prompt shape without runtime skill routing summaries.
    let system_prompt = build_forced_synthesis_system_prompt_with_notify_guidance(
        params.context_messages,
        params.memory_context,
        params.scratchpad_context,
        params.notify_tool_guidance_enabled,
    );

    CompletionRequest {
        model: params.model.to_string(),
        messages: strip_system_messages(params.context_messages),
        tools: vec![],
        temperature: Some(0.3),
        max_tokens: Some(2048),
        system_prompt: Some(system_prompt),
        thinking: None,
    }
}

pub(super) fn build_truncation_continuation_request(
    params: TruncationContinuationRequestParams<'_>,
) -> CompletionRequest {
    let system_prompt = build_reasoning_system_prompt_with_notify_guidance(
        params.context.memory_context,
        params.context.scratchpad_context,
        params.context.notify_tool_guidance_enabled,
        params.context.skill_summaries_slice(),
    );

    CompletionRequest {
        model: params.model.to_string(),
        messages: params.continuation_messages.to_vec(),
        tools: continuation_tools_for_step(params.step, params.tool_config.into_tools()),
        temperature: Some(REASONING_TEMPERATURE),
        max_tokens: Some(REASONING_MAX_OUTPUT_TOKENS),
        system_prompt: Some(system_prompt),
        thinking: params.context.thinking,
    }
}

pub(super) fn build_reasoning_request(params: ReasoningRequestParams<'_>) -> CompletionRequest {
    let system_prompt = build_reasoning_system_prompt_with_notify_guidance(
        params.context.memory_context,
        params.context.scratchpad_context,
        params.context.notify_tool_guidance_enabled,
        params.context.skill_summaries_slice(),
    );

    CompletionRequest {
        model: params.model.to_string(),
        messages: build_reasoning_messages(params.perception),
        tools: params.tool_config.into_tools(),
        temperature: Some(REASONING_TEMPERATURE),
        max_tokens: Some(REASONING_MAX_OUTPUT_TOKENS),
        system_prompt: Some(system_prompt),
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

pub(super) fn reasoning_user_prompt(perception: &ProcessedPerception) -> String {
    let mut prompt = format!(
        "Active goals:\n- {}\n\nBudget remaining: {} tokens, {} llm calls\n\nUser message:\n{}",
        perception.active_goals.join("\n- "),
        perception.budget_remaining.tokens,
        perception.budget_remaining.llm_calls,
        perception.user_message,
    );

    if let Some(steer) = perception.steer_context.as_deref() {
        prompt.push_str(&format!("\nUser steer (latest): {steer}"));
    }

    prompt
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

pub(super) fn build_reasoning_system_prompt_with_notify_guidance(
    memory_context: Option<&str>,
    scratchpad_context: Option<&str>,
    notify_tool_guidance_enabled: bool,
    skill_prompt_summaries: Option<&[SkillPromptSummary]>,
) -> String {
    build_system_prompt(
        memory_context,
        scratchpad_context,
        None,
        notify_tool_guidance_enabled,
        skill_prompt_summaries,
    )
}

fn build_forced_synthesis_system_prompt_with_notify_guidance(
    context_messages: &[Message],
    memory_context: Option<&str>,
    scratchpad_context: Option<&str>,
    notify_tool_guidance_enabled: bool,
) -> String {
    let mut system_prompt = build_reasoning_system_prompt_with_notify_guidance(
        memory_context,
        scratchpad_context,
        notify_tool_guidance_enabled,
        None,
    );
    let directives = system_messages_to_prompt_directives(context_messages);
    if !directives.is_empty() {
        system_prompt.push_str("\n\nAdditional runtime directives:\n");
        for directive in directives {
            system_prompt.push_str("- ");
            system_prompt.push_str(&directive);
            system_prompt.push('\n');
        }
    }
    system_prompt.push_str(BUDGET_EXHAUSTED_SYNTHESIS_DIRECTIVE);
    system_prompt
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

pub(super) fn build_tool_continuation_system_prompt_with_notify_guidance(
    memory_context: Option<&str>,
    scratchpad_context: Option<&str>,
    notify_tool_guidance_enabled: bool,
    skill_prompt_summaries: Option<&[SkillPromptSummary]>,
) -> String {
    // Keep this visible to sibling test modules so both prompt paths can
    // exercise the same renderer without duplicating setup.
    build_system_prompt(
        memory_context,
        scratchpad_context,
        Some(TOOL_CONTINUATION_DIRECTIVE),
        notify_tool_guidance_enabled,
        skill_prompt_summaries,
    )
}

fn build_system_prompt(
    memory_context: Option<&str>,
    scratchpad_context: Option<&str>,
    extra_directive: Option<&str>,
    notify_tool_guidance_enabled: bool,
    skill_prompt_summaries: Option<&[SkillPromptSummary]>,
) -> String {
    let mut sections = vec![REASONING_SYSTEM_PROMPT.to_string()];

    if let Some(capabilities) = render_skill_capabilities(skill_prompt_summaries) {
        sections.push(capabilities);
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

fn trim_section_newlines(section: &str) -> &str {
    section.trim_matches('\n')
}

fn trim_leading_newlines(section: &str) -> &str {
    section.trim_start_matches('\n')
}

fn strip_system_messages(messages: &[Message]) -> Vec<Message> {
    messages
        .iter()
        .filter(|message| message.role != MessageRole::System)
        .cloned()
        .collect()
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
            "properties": {
                "sub_goals": {
                    "type": "array",
                    "items": {
                        "type": "object",
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
                    "description": "List of sub-goals to execute"
                },
                "strategy": {"type": "string", "enum": ["Sequential", "Parallel"], "description": "Execution strategy"}
            },
            "required": ["sub_goals"]
        }),
    }
}
