use super::{truncate_prompt_text, LlmProvider};
use crate::conversation_compactor::{
    assemble_summarized_messages, generate_summary, CompactionConfig, CompactionError,
    CompactionResult, ConversationBudget, SlideSummarizationPlan,
};
use crate::types::{LoopError, ReasoningContext};
use fx_llm::{ContentBlock, Message, MessageRole};
use fx_session::SessionMemoryUpdate;

const COMPACTED_CONTEXT_SUMMARY_KEY: &str = "compacted_context_summary";
#[cfg(test)]
const COMPACTED_CONTEXT_SUMMARY_PREFIX: &str = "Compacted context summary:";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CompactionScope {
    Perceive,
    ToolContinuation,
    DecomposeChild,
}

impl CompactionScope {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Perceive => "perceive",
            Self::ToolContinuation => "tool_continuation",
            Self::DecomposeChild => "decompose_child",
        }
    }
}

impl std::fmt::Display for CompactionScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) enum CompactionTier {
    Prune,
    Slide,
    Emergency,
}

impl CompactionTier {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Prune => "prune",
            Self::Slide => "slide",
            Self::Emergency => "emergency",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct FinishTierContext {
    pub(super) scope: CompactionScope,
    pub(super) iteration: Option<u32>,
    pub(super) target_tokens: usize,
}

pub(super) fn highest_compaction_tier(
    messages: &[Message],
    conversation_budget: &ConversationBudget,
    compaction_config: &CompactionConfig,
) -> Option<CompactionTier> {
    if conversation_budget.at_tier(messages, compaction_config.emergency_threshold) {
        return Some(CompactionTier::Emergency);
    }
    if conversation_budget.at_tier(messages, compaction_config.slide_threshold) {
        return Some(CompactionTier::Slide);
    }
    None
}

pub(super) fn compaction_cooldown_active(
    last_iteration: Option<u32>,
    iteration: u32,
    cooldown_turns: u32,
) -> bool {
    last_iteration
        .map(|last| iteration.saturating_sub(last) < cooldown_turns)
        .unwrap_or(false)
}

pub(super) fn can_summarize_eviction(
    compaction_config: &CompactionConfig,
    has_compaction_llm: bool,
) -> bool {
    compaction_config.use_summarization && has_compaction_llm
}

pub(super) async fn generate_eviction_summary(
    llm: &dyn LlmProvider,
    messages: &[Message],
    max_summary_tokens: usize,
) -> Result<String, CompactionError> {
    generate_summary(llm, messages, max_summary_tokens).await
}

pub(super) fn summarized_compaction_result(
    messages: &[Message],
    plan: &SlideSummarizationPlan,
    summary: String,
) -> CompactionResult {
    let compacted_messages = assemble_summarized_messages(messages, plan, &summary);
    CompactionResult {
        estimated_tokens: ConversationBudget::estimate_tokens(&compacted_messages),
        messages: compacted_messages,
        compacted_count: plan.evicted_messages.len(),
        used_summarization: true,
        summary: Some(summary),
        evicted_indices: plan.evicted_indices.clone(),
    }
}

pub(super) fn merge_summarized_follow_up(
    base: CompactionResult,
    follow_up: CompactionResult,
) -> CompactionResult {
    CompactionResult {
        messages: follow_up.messages,
        compacted_count: base.compacted_count + follow_up.compacted_count,
        estimated_tokens: follow_up.estimated_tokens,
        used_summarization: true,
        summary: base.summary,
        evicted_indices: base.evicted_indices,
    }
}

pub(super) fn build_extraction_prompt(messages: &[Message]) -> String {
    format!(
        concat!(
            "Extract key facts from this conversation excerpt that is being removed from context.\n",
            "Return a JSON object with these optional fields:\n",
            "- \"project\": what the session is about (string, only if clearly identifiable)\n",
            "- \"current_state\": current state of work (string, only if clear)\n",
            "- \"key_decisions\": important decisions made (array of short strings)\n",
            "- \"active_files\": files being worked on (array of paths)\n",
            "- \"custom_context\": other important facts to remember (array of short strings)\n\n",
            "Only include fields where the conversation clearly contains relevant information.\n",
            "Keep each string under 100 characters. Return ONLY valid JSON, no markdown.\n\n",
            "Conversation:\n{}"
        ),
        format_extraction_messages(messages)
    )
}

fn format_extraction_messages(messages: &[Message]) -> String {
    messages
        .iter()
        .filter_map(format_extraction_message)
        .collect::<Vec<_>>()
        .join("\n")
}

fn format_extraction_message(message: &Message) -> Option<String> {
    let role = extraction_role(&message.role)?;
    let content = message
        .content
        .iter()
        .map(format_extraction_block)
        .collect::<Vec<_>>()
        .join(" ");
    Some(format!("{role}: {content}"))
}

fn extraction_role(role: &MessageRole) -> Option<&'static str> {
    match role {
        MessageRole::User => Some("user"),
        MessageRole::Assistant => Some("assistant"),
        MessageRole::System => None,
        MessageRole::Tool => Some("tool"),
    }
}

fn format_extraction_block(block: &ContentBlock) -> String {
    match block {
        ContentBlock::Text { text } => text.clone(),
        ContentBlock::ToolUse { name, .. } => format!("[tool: {name}]"),
        ContentBlock::ToolResult { content, .. } => {
            truncate_prompt_text(&render_tool_result(content), 200)
        }
        ContentBlock::Image { .. } => "[image]".to_string(),
        ContentBlock::Document { filename, .. } => filename
            .as_ref()
            .map(|filename| format!("[document:{filename}]"))
            .unwrap_or_else(|| "[document]".to_string()),
    }
}

fn render_tool_result(content: &serde_json::Value) -> String {
    match content.as_str() {
        Some(text) => text.to_string(),
        None => content.to_string(),
    }
}

pub(super) fn parse_extraction_response(response: &str) -> Option<SessionMemoryUpdate> {
    let trimmed = response.trim();
    if let Ok(update) = serde_json::from_str::<SessionMemoryUpdate>(trimmed) {
        return Some(update);
    }
    if let Some(json) = extract_json_object(trimmed) {
        if let Ok(update) = serde_json::from_str::<SessionMemoryUpdate>(json) {
            return Some(update);
        }
    }
    tracing::warn!(
        response_len = response.len(),
        "failed to parse memory extraction response as JSON"
    );
    None
}

fn extract_json_object(text: &str) -> Option<&str> {
    let start = text.find('{')?;
    let end = text.rfind('}')?;
    (end > start).then_some(&text[start..=end])
}

#[derive(Clone, Copy)]
enum SummarySection {
    Decisions,
    FilesModified,
    TaskState,
    KeyContext,
}

#[derive(Default)]
struct ParsedSummarySections {
    decisions: Vec<String>,
    files_modified: Vec<String>,
    task_state: Vec<String>,
    key_context: Vec<String>,
}

pub(super) fn parse_summary_memory_update(summary: &str) -> Option<SessionMemoryUpdate> {
    let sections = parse_summary_sections(summary);
    let update = SessionMemoryUpdate {
        project: None,
        current_state: joined_summary_section(&sections.task_state),
        key_decisions: optional_summary_items(sections.decisions),
        active_files: optional_summary_items(sections.files_modified),
        custom_context: optional_summary_items(sections.key_context),
    };
    has_memory_update_fields(&update).then_some(update)
}

fn parse_summary_sections(summary: &str) -> ParsedSummarySections {
    let mut sections = ParsedSummarySections::default();
    let mut current = None;
    for line in summary
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        if let Some((section, inline)) = summary_section_header(line) {
            current = Some(section);
            if let Some(text) = inline {
                push_summary_section_line(&mut sections, section, text);
            }
            continue;
        }
        if let Some(section) = current {
            push_summary_section_line(&mut sections, section, line);
        }
    }
    sections
}

fn summary_section_header(line: &str) -> Option<(SummarySection, Option<&str>)> {
    let (heading, remainder) = line.split_once(':')?;
    let section = match strip_summary_section_numbering(heading) {
        text if text.eq_ignore_ascii_case("Decisions") => SummarySection::Decisions,
        text if text.eq_ignore_ascii_case("Files modified") => SummarySection::FilesModified,
        text if text.eq_ignore_ascii_case("Task state") => SummarySection::TaskState,
        text if text.eq_ignore_ascii_case("Key context") => SummarySection::KeyContext,
        _ => return None,
    };
    let inline = (!remainder.trim().is_empty()).then_some(remainder.trim());
    Some((section, inline))
}

fn strip_summary_section_numbering(heading: &str) -> &str {
    let trimmed = heading.trim();
    let digits_len = trimmed
        .as_bytes()
        .iter()
        .take_while(|byte| byte.is_ascii_digit())
        .count();
    if digits_len == 0 {
        return trimmed;
    }
    trimmed[digits_len..]
        .strip_prefix('.')
        .map_or(trimmed, |remainder| remainder.trim_start())
}

fn push_summary_section_line(
    sections: &mut ParsedSummarySections,
    section: SummarySection,
    line: &str,
) {
    let trimmed = line.trim();
    let item = trimmed
        .strip_prefix("- ")
        .or_else(|| trimmed.strip_prefix("* "))
        .unwrap_or(trimmed)
        .trim();
    if item.is_empty() {
        return;
    }
    match section {
        SummarySection::Decisions => sections.decisions.push(item.to_string()),
        SummarySection::FilesModified => sections.files_modified.push(item.to_string()),
        SummarySection::TaskState => sections.task_state.push(item.to_string()),
        SummarySection::KeyContext => sections.key_context.push(item.to_string()),
    }
}

fn joined_summary_section(items: &[String]) -> Option<String> {
    (!items.is_empty()).then(|| items.join("; "))
}

fn optional_summary_items(items: Vec<String>) -> Option<Vec<String>> {
    (!items.is_empty()).then_some(items)
}

fn has_memory_update_fields(update: &SessionMemoryUpdate) -> bool {
    update.project.is_some()
        || update.current_state.is_some()
        || update.key_decisions.is_some()
        || update.active_files.is_some()
        || update.custom_context.is_some()
}

pub(super) fn compaction_failed_error(scope: CompactionScope, error: CompactionError) -> LoopError {
    LoopError {
        stage: "compaction".to_string(),
        reason: format!("compaction_failed: scope={scope} error={error}"),
        recoverable: true,
    }
}

pub(super) fn context_exceeded_after_compaction_error(
    scope: CompactionScope,
    estimated_tokens: usize,
    hard_limit_tokens: usize,
) -> LoopError {
    LoopError {
        stage: "compaction".to_string(),
        reason: format!(
            "context_exceeded_after_compaction: scope={scope} estimated_tokens={estimated_tokens} hard_limit_tokens={hard_limit_tokens}",
        ),
        recoverable: true,
    }
}

pub(super) fn compacted_context_summary(context: &ReasoningContext) -> Option<&str> {
    context
        .working_memory
        .iter()
        .find(|entry| entry.key == COMPACTED_CONTEXT_SUMMARY_KEY)
        .map(|entry| entry.value.as_str())
}

#[cfg(test)]
pub(super) fn has_compaction_marker(messages: &[Message]) -> bool {
    messages.iter().any(|message| {
        message.content.iter().any(|block| {
            matches!(
                block,
                ContentBlock::Text { text } if text.starts_with("[context compacted:")
            )
        })
    })
}

#[cfg(test)]
pub(super) fn has_emergency_compaction_marker(messages: &[Message]) -> bool {
    messages.iter().any(|message| {
        message.content.iter().any(|block| {
            matches!(
                block,
                ContentBlock::Text { text }
                    if text.starts_with("[context compacted:") && text.contains("emergency")
            )
        })
    })
}

#[cfg(test)]
pub(super) fn has_conversation_summary_marker(messages: &[Message]) -> bool {
    messages.iter().any(|message| {
        message.content.iter().any(|block| {
            matches!(
                block,
                ContentBlock::Text { text } if text.starts_with("[context summary]")
            )
        })
    })
}

#[cfg(test)]
pub(super) fn summary_message_index(messages: &[Message]) -> Option<usize> {
    messages.iter().position(|message| {
        message.content.iter().any(|block| {
            matches!(
                block,
                ContentBlock::Text { text }
                    if text.starts_with(COMPACTED_CONTEXT_SUMMARY_PREFIX)
            )
        })
    })
}

#[cfg(test)]
pub(super) fn marker_message_index(messages: &[Message]) -> Option<usize> {
    messages.iter().position(|message| {
        message.content.iter().any(|block| {
            matches!(
                block,
                ContentBlock::Text { text } if text.starts_with("[context compacted:")
            )
        })
    })
}

#[cfg(test)]
pub(super) fn session_memory_message_index(messages: &[Message]) -> Option<usize> {
    messages.iter().position(|message| {
        message.content.iter().any(|block| {
            matches!(
                block,
                ContentBlock::Text { text } if text.starts_with("[Session Memory]")
            )
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use fx_llm::{ContentBlock, Message, MessageRole};

    fn words(count: usize) -> String {
        std::iter::repeat_n("a", count)
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn tool_use(id: &str) -> Message {
        Message {
            role: MessageRole::Assistant,
            content: vec![ContentBlock::ToolUse {
                id: id.to_string(),
                provider_id: None,
                name: "read".to_string(),
                input: serde_json::json!({"path": "/tmp/a"}),
            }],
        }
    }

    fn tool_result(id: &str, word_count: usize) -> Message {
        Message {
            role: MessageRole::Tool,
            content: vec![ContentBlock::ToolResult {
                tool_use_id: id.to_string(),
                content: serde_json::json!(words(word_count)),
            }],
        }
    }

    #[test]
    fn compaction_scope_display_uses_scope_label() {
        assert_eq!(CompactionScope::Perceive.to_string(), "perceive");
        assert_eq!(
            CompactionScope::ToolContinuation.to_string(),
            "tool_continuation"
        );
        assert_eq!(
            CompactionScope::DecomposeChild.to_string(),
            "decompose_child"
        );
    }

    #[test]
    fn build_extraction_prompt_formats_messages() {
        let prompt = build_extraction_prompt(&[
            Message::system("system policy"),
            Message::user("User fact"),
            tool_use("call-1"),
            tool_result("call-1", 250),
            Message {
                role: MessageRole::Assistant,
                content: vec![ContentBlock::Image {
                    media_type: "image/png".to_string(),
                    data: "abc".to_string(),
                }],
            },
        ]);

        assert!(prompt.contains("Return ONLY valid JSON"));
        assert!(prompt.contains("user: User fact"));
        assert!(prompt.contains("assistant: [tool: read]"));
        assert!(prompt.contains("tool: "));
        assert!(prompt.contains("[image]"));
        assert!(prompt.contains("..."));
        assert!(!prompt.contains("system: system policy"));
    }

    #[test]
    fn parse_extraction_response_handles_code_block() {
        let response = "```json\n{\"project\":\"Phase 5\"}\n```";

        let update = parse_extraction_response(response).expect("parse code block");

        assert_eq!(update.project.as_deref(), Some("Phase 5"));
    }

    #[test]
    fn parse_extraction_response_returns_none_for_garbage() {
        assert!(parse_extraction_response("definitely not json").is_none());
    }

    #[test]
    fn parse_summary_memory_update_extracts_sections() {
        let summary = concat!(
            "Decisions:\n",
            "- Use summarize-before-slide\n",
            "Files modified:\n",
            "- engine/crates/fx-kernel/src/loop_engine.rs\n",
            "Task state:\n",
            "- Implementing Phase 2\n",
            "Key context:\n",
            "- Preserve summary markers during follow-up slide"
        );

        let update = parse_summary_memory_update(summary).expect("summary parse");

        assert_eq!(update.project, None);
        assert_eq!(
            update.current_state.as_deref(),
            Some("Implementing Phase 2")
        );
        assert_eq!(
            update.key_decisions,
            Some(vec!["Use summarize-before-slide".to_string()])
        );
        assert_eq!(
            update.active_files,
            Some(vec![
                "engine/crates/fx-kernel/src/loop_engine.rs".to_string()
            ])
        );
        assert_eq!(
            update.custom_context,
            Some(vec![
                "Preserve summary markers during follow-up slide".to_string()
            ])
        );
    }

    #[test]
    fn parse_summary_memory_update_extracts_numbered_sections() {
        let summary = concat!(
            "1. Decisions:\n",
            "- Use summarize-before-slide\n",
            "2. Files modified:\n",
            "- engine/crates/fx-kernel/src/loop_engine.rs\n",
            "3. Task state:\n",
            "- Implementing Phase 2\n",
            "4. Key context:\n",
            "- Preserve summary markers during follow-up slide"
        );

        let update = parse_summary_memory_update(summary).expect("summary parse");

        assert_eq!(update.project, None);
        assert_eq!(
            update.current_state.as_deref(),
            Some("Implementing Phase 2")
        );
        assert_eq!(
            update.key_decisions,
            Some(vec!["Use summarize-before-slide".to_string()])
        );
        assert_eq!(
            update.active_files,
            Some(vec![
                "engine/crates/fx-kernel/src/loop_engine.rs".to_string()
            ])
        );
        assert_eq!(
            update.custom_context,
            Some(vec![
                "Preserve summary markers during follow-up slide".to_string()
            ])
        );
    }
}
