use crate::loop_engine::LlmProvider;
use async_trait::async_trait;
use fx_llm::{ContentBlock, Message, MessageRole};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::error::Error;
use std::sync::Arc;

const COMPACTION_MARKER_PREFIX: &str = "[context compacted:";
const SUMMARY_MARKER_PREFIX: &str = "[context summary]";
const IMAGE_TOKEN_ESTIMATE: usize = 1600;

/// Shared token estimation heuristic used by loop and perception accounting.
///
/// NOTE: This currently estimates text-only content. Multimodal token accounting
/// is out-of-scope for this phase and should be added in a follow-up.
pub fn estimate_text_tokens(text: &str) -> usize {
    if text.trim().is_empty() {
        return 0;
    }

    let char_estimate = text.chars().count().div_ceil(4);
    let word_estimate = text.split_whitespace().count();
    char_estimate.max(word_estimate).max(1)
}

fn estimate_content_tokens(content: &ContentBlock) -> usize {
    match content {
        ContentBlock::Text { text } => estimate_text_tokens(text),
        ContentBlock::ToolUse { id, name, input } => {
            estimate_text_tokens(id)
                + estimate_text_tokens(name)
                + estimate_text_tokens(&input.to_string())
        }
        ContentBlock::ToolResult {
            tool_use_id,
            content,
        } => estimate_text_tokens(tool_use_id) + estimate_text_tokens(&content.to_string()),
        ContentBlock::Image { .. } => IMAGE_TOKEN_ESTIMATE,
    }
}

fn estimate_message_tokens(message: &Message) -> usize {
    message.content.iter().map(estimate_content_tokens).sum()
}

fn text_blocks(message: &Message) -> impl Iterator<Item = &str> {
    message.content.iter().filter_map(|block| match block {
        ContentBlock::Text { text } => Some(text.as_str()),
        _ => None,
    })
}

fn message_contains_marker(message: &Message) -> bool {
    text_blocks(message).any(|text| text.starts_with(COMPACTION_MARKER_PREFIX))
}

fn message_is_system_like(message: &Message) -> bool {
    matches!(message.role, MessageRole::System) || message_contains_marker(message)
}

/// Summary messages intentionally use `assistant` role so they remain visible
/// to subsequent model turns as authored conversation state.
///
/// This can create adjacent assistant-role messages in the compacted window,
/// which is acceptable in the current integration because message ordering is
/// preserved and no role alternation invariant is enforced by providers.
fn summary_message(summary: &str) -> Message {
    Message::assistant(format!("{SUMMARY_MARKER_PREFIX}\n{summary}"))
}

/// Marker messages intentionally use `assistant` role to keep compaction
/// metadata in-band with conversational history and avoid special-casing a
/// synthetic role in downstream adapters.
///
/// Adjacent assistant messages are safe for the same reason documented on
/// [`summary_message`]: ordering is preserved and role alternation is not
/// required by current request builders.
fn compaction_marker_message(compacted_count: usize) -> Message {
    Message::assistant(format!(
        "{COMPACTION_MARKER_PREFIX} {compacted_count} older messages removed]"
    ))
}

fn tool_ids_in_message(message: &Message) -> Vec<&str> {
    message
        .content
        .iter()
        .filter_map(|block| match block {
            ContentBlock::ToolUse { id, .. } => Some(id.as_str()),
            ContentBlock::ToolResult { tool_use_id, .. } => Some(tool_use_id.as_str()),
            ContentBlock::Text { .. } | ContentBlock::Image { .. } => None,
        })
        .collect()
}

fn unresolved_tool_use_ids(messages: &[Message]) -> HashSet<&str> {
    let mut tool_use_ids = HashSet::new();
    let mut tool_result_ids = HashSet::new();

    for message in messages {
        for block in &message.content {
            match block {
                ContentBlock::ToolUse { id, .. } => {
                    tool_use_ids.insert(id.as_str());
                }
                ContentBlock::ToolResult { tool_use_id, .. } => {
                    tool_result_ids.insert(tool_use_id.as_str());
                }
                ContentBlock::Text { .. } | ContentBlock::Image { .. } => {}
            }
        }
    }

    tool_use_ids
        .into_iter()
        .filter(|id| !tool_result_ids.contains(id))
        .collect()
}

fn ids_referenced_in_tail(messages: &[Message], tail_start: usize) -> HashSet<&str> {
    messages[tail_start..]
        .iter()
        .flat_map(tool_ids_in_message)
        .collect()
}

#[derive(Debug, Clone)]
struct ZoneBounds {
    prefix_end: usize,
    tail_start: usize,
}

fn zone_bounds(messages: &[Message], preserve_recent_turns: usize) -> ZoneBounds {
    let prefix_end = messages
        .iter()
        .take_while(|message| message_is_system_like(message))
        .count();
    let tail_start = messages
        .len()
        .saturating_sub(preserve_recent_turns)
        .max(prefix_end);

    ZoneBounds {
        prefix_end,
        tail_start,
    }
}

fn protected_middle_indices(messages: &[Message], bounds: &ZoneBounds) -> HashSet<usize> {
    let unresolved_ids = unresolved_tool_use_ids(messages);
    let tail_ids = ids_referenced_in_tail(messages, bounds.tail_start);

    (bounds.prefix_end..bounds.tail_start)
        .filter(|index| {
            let message = &messages[*index];
            message_is_system_like(message)
                || tool_ids_in_message(message)
                    .iter()
                    .any(|id| unresolved_ids.contains(id) || tail_ids.contains(id))
        })
        .collect()
}

fn removable_middle_offsets(bounds: &ZoneBounds, protected_middle: &HashSet<usize>) -> Vec<usize> {
    (bounds.prefix_end..bounds.tail_start)
        .filter(|index| !protected_middle.contains(index))
        .map(|index| index - bounds.prefix_end)
        .collect()
}

fn evicted_indices_from_keep_middle(bounds: &ZoneBounds, keep_middle: &[bool]) -> Vec<usize> {
    keep_middle
        .iter()
        .enumerate()
        .filter_map(|(offset, keep)| (!keep).then_some(bounds.prefix_end + offset))
        .collect()
}

fn summarizable_middle_indices(
    bounds: &ZoneBounds,
    protected_middle: &HashSet<usize>,
) -> Vec<usize> {
    (bounds.prefix_end..bounds.tail_start)
        .filter(|index| !protected_middle.contains(index))
        .collect()
}

fn cloned_messages_at_indices(messages: &[Message], indices: &[usize]) -> Vec<Message> {
    indices
        .iter()
        .map(|index| messages[*index].clone())
        .collect()
}

fn append_protected_middle_messages(
    compacted_messages: &mut Vec<Message>,
    messages: &[Message],
    bounds: &ZoneBounds,
    protected_middle: &HashSet<usize>,
) {
    for (index, message) in messages
        .iter()
        .enumerate()
        .skip(bounds.prefix_end)
        .take(bounds.tail_start.saturating_sub(bounds.prefix_end))
    {
        if protected_middle.contains(&index) {
            compacted_messages.push(message.clone());
        }
    }
}

fn assemble_sliding_result(
    messages: &[Message],
    bounds: &ZoneBounds,
    keep_middle: &[bool],
    compacted_count: usize,
) -> Vec<Message> {
    let mut compacted = Vec::new();
    compacted.extend_from_slice(&messages[..bounds.prefix_end]);

    if compacted_count > 0 {
        compacted.push(compaction_marker_message(compacted_count));
    }

    for (offset, keep) in keep_middle.iter().enumerate() {
        if *keep {
            compacted.push(messages[bounds.prefix_end + offset].clone());
        }
    }

    compacted.extend_from_slice(&messages[bounds.tail_start..]);
    compacted
}

fn compaction_marker_tokens(compacted_count: usize) -> usize {
    estimate_message_tokens(&compaction_marker_message(compacted_count))
}

fn remove_oldest_middle_until_target(
    messages: &[Message],
    target_tokens: usize,
    bounds: &ZoneBounds,
    removable_offsets: &[usize],
) -> Result<(Vec<bool>, usize), CompactionError> {
    let middle_len = bounds.tail_start.saturating_sub(bounds.prefix_end);
    let mut keep_middle = vec![true; middle_len];
    let mut compacted_count = 0;
    let mut estimated_tokens = ConversationBudget::estimate_tokens(messages);
    let mut marker_tokens = 0;
    let removable_token_costs = removable_offsets
        .iter()
        .map(|offset| estimate_message_tokens(&messages[bounds.prefix_end + *offset]))
        .collect::<Vec<_>>();

    for (offset, token_cost) in removable_offsets.iter().zip(removable_token_costs.iter()) {
        if estimated_tokens <= target_tokens {
            break;
        }

        keep_middle[*offset] = false;
        compacted_count += 1;
        estimated_tokens = estimated_tokens.saturating_sub(*token_cost);

        let next_marker_tokens = compaction_marker_tokens(compacted_count);
        if next_marker_tokens >= marker_tokens {
            estimated_tokens = estimated_tokens.saturating_add(next_marker_tokens - marker_tokens);
        } else {
            estimated_tokens = estimated_tokens.saturating_sub(marker_tokens - next_marker_tokens);
        }
        marker_tokens = next_marker_tokens;
    }

    if estimated_tokens > target_tokens {
        return Err(CompactionError::AllMessagesProtected);
    }

    Ok((keep_middle, compacted_count))
}

fn sliding_compaction_result(
    messages: &[Message],
    target_tokens: usize,
    preserve_recent_turns: usize,
) -> Result<CompactionResult, CompactionError> {
    let before_tokens = ConversationBudget::estimate_tokens(messages);
    if before_tokens <= target_tokens {
        return Ok(CompactionResult {
            messages: messages.to_vec(),
            compacted_count: 0,
            estimated_tokens: before_tokens,
            used_summarization: false,
            evicted_indices: Vec::new(),
        });
    }

    let bounds = zone_bounds(messages, preserve_recent_turns);
    let protected_middle = protected_middle_indices(messages, &bounds);
    let removable_offsets = removable_middle_offsets(&bounds, &protected_middle);

    if removable_offsets.is_empty() {
        return Err(CompactionError::AllMessagesProtected);
    }

    let (keep_middle, compacted_count) =
        remove_oldest_middle_until_target(messages, target_tokens, &bounds, &removable_offsets)?;
    let evicted_indices = evicted_indices_from_keep_middle(&bounds, &keep_middle);
    let compacted_messages =
        assemble_sliding_result(messages, &bounds, &keep_middle, compacted_count);

    Ok(CompactionResult {
        estimated_tokens: ConversationBudget::estimate_tokens(&compacted_messages),
        messages: compacted_messages,
        compacted_count,
        used_summarization: false,
        evicted_indices,
    })
}

/// Budget tracker for conversation-level context usage.
#[derive(Debug, Clone)]
pub struct ConversationBudget {
    model_context_limit: usize,
    compaction_threshold: f32,
    reserved_tokens: usize,
    output_reserve_tokens: usize,
}

impl ConversationBudget {
    pub const DEFAULT_OUTPUT_RESERVE_TOKENS: usize = 4_096;

    pub fn new(
        model_context_limit: usize,
        compaction_threshold: f32,
        reserved_tokens: usize,
    ) -> Self {
        Self {
            model_context_limit,
            compaction_threshold,
            reserved_tokens,
            output_reserve_tokens: Self::DEFAULT_OUTPUT_RESERVE_TOKENS,
        }
    }

    pub fn conversation_budget(&self) -> usize {
        self.model_context_limit
            .saturating_sub(self.reserved_tokens)
            .saturating_sub(self.output_reserve_tokens)
    }

    pub fn needs_compaction(&self, messages: &[Message]) -> bool {
        let trigger =
            (self.conversation_budget() as f32 * self.compaction_threshold).ceil() as usize;
        Self::estimate_tokens(messages) >= trigger
    }

    pub fn exceeds_hard_limit(&self, messages: &[Message]) -> bool {
        Self::estimate_tokens(messages) > self.conversation_budget()
    }

    pub fn estimate_tokens(messages: &[Message]) -> usize {
        messages.iter().map(estimate_message_tokens).sum()
    }

    pub fn compaction_target(&self) -> usize {
        self.conversation_budget().saturating_mul(3) / 5
    }
}

/// Strategy for compacting an oversized conversation history.
#[async_trait]
pub trait CompactionStrategy: Send + Sync + std::fmt::Debug {
    async fn compact(
        &self,
        messages: &[Message],
        target_tokens: usize,
    ) -> Result<CompactionResult, CompactionError>;
}

/// Persists evicted message content before compaction drops them.
#[async_trait]
pub trait CompactionMemoryFlush: Send + Sync + std::fmt::Debug {
    /// Flush content from messages about to be evicted.
    async fn flush(
        &self,
        evicted: &[Message],
        scope_label: &str,
    ) -> Result<(), CompactionFlushError>;
}

#[derive(Debug, thiserror::Error)]
pub enum CompactionFlushError {
    #[error("memory flush failed: {reason}")]
    FlushFailed { reason: String },
}

#[derive(Debug, thiserror::Error)]
pub enum CompactionError {
    #[error("summarization failed")]
    SummarizationFailed {
        source: Box<dyn Error + Send + Sync>,
    },
    #[error("summary exceeded target token budget")]
    SummaryExceededTarget,
    #[error("all messages are protected; cannot compact further")]
    AllMessagesProtected,
}

#[derive(Debug, Clone)]
pub struct CompactionResult {
    pub(crate) messages: Vec<Message>,
    pub(crate) compacted_count: usize,
    pub(crate) estimated_tokens: usize,
    pub(crate) used_summarization: bool,
    /// Indices into the original message slice for messages evicted by compaction.
    pub(crate) evicted_indices: Vec<usize>,
}

/// Keeps the N most recent turns and drops older ones.
#[derive(Debug, Clone)]
pub struct SlidingWindowCompactor {
    preserve_recent_turns: usize,
}

impl SlidingWindowCompactor {
    pub fn new(preserve_recent_turns: usize) -> Self {
        Self {
            preserve_recent_turns,
        }
    }
}

#[async_trait]
impl CompactionStrategy for SlidingWindowCompactor {
    async fn compact(
        &self,
        messages: &[Message],
        target_tokens: usize,
    ) -> Result<CompactionResult, CompactionError> {
        sliding_compaction_result(messages, target_tokens, self.preserve_recent_turns)
    }
}

/// Summarizes older turns into structured context using an LLM call.
#[derive(Debug)]
pub struct SummarizingCompactor {
    llm: Arc<dyn LlmProvider>,
    preserve_recent_turns: usize,
    max_summary_tokens: usize,
}

impl SummarizingCompactor {
    pub const DEFAULT_MAX_SUMMARY_TOKENS: usize = 1_024;

    pub fn new(llm: Arc<dyn LlmProvider>, preserve_recent_turns: usize) -> Self {
        Self::with_max_summary_tokens(llm, preserve_recent_turns, Self::DEFAULT_MAX_SUMMARY_TOKENS)
    }

    pub fn with_max_summary_tokens(
        llm: Arc<dyn LlmProvider>,
        preserve_recent_turns: usize,
        max_summary_tokens: usize,
    ) -> Self {
        Self {
            llm,
            preserve_recent_turns,
            max_summary_tokens,
        }
    }

    fn summarizable_indices(
        &self,
        bounds: &ZoneBounds,
        protected_middle: &HashSet<usize>,
    ) -> Result<Vec<usize>, CompactionError> {
        let indices = summarizable_middle_indices(bounds, protected_middle);
        if indices.is_empty() {
            return Err(CompactionError::AllMessagesProtected);
        }
        Ok(indices)
    }

    async fn generate_summary(
        &self,
        summarizable_messages: &[Message],
    ) -> Result<String, CompactionError> {
        let prompt = Self::summary_prompt(summarizable_messages);
        self.llm
            .generate(&prompt, self.max_summary_tokens as u32)
            .await
            .map_err(|source| CompactionError::SummarizationFailed {
                source: Box::new(source),
            })
    }

    fn assemble_summarized_messages(
        &self,
        messages: &[Message],
        bounds: &ZoneBounds,
        protected_middle: &HashSet<usize>,
        summary: &str,
    ) -> Vec<Message> {
        let mut compacted_messages = Vec::new();
        compacted_messages.extend_from_slice(&messages[..bounds.prefix_end]);
        append_protected_middle_messages(
            &mut compacted_messages,
            messages,
            bounds,
            protected_middle,
        );
        compacted_messages.push(summary_message(summary));
        compacted_messages.extend_from_slice(&messages[bounds.tail_start..]);
        compacted_messages
    }

    fn summary_prompt(messages: &[Message]) -> String {
        let conversation = messages
            .iter()
            .map(message_to_summary_line)
            .collect::<Vec<_>>()
            .join("\n");

        format!(
            "Summarize the following conversation history.\n\
Keep the summary factual and grounded in provided content only.\n\
\nSections (required):\n\
1. Decisions\n\
2. Files modified\n\
3. Task state\n\
4. Key context\n\
\nConversation:\n{conversation}"
        )
    }
}

fn message_to_summary_line(message: &Message) -> String {
    let role = match message.role {
        MessageRole::System => "system",
        MessageRole::User => "user",
        MessageRole::Assistant => "assistant",
        MessageRole::Tool => "tool",
    };

    let text = message
        .content
        .iter()
        .map(|block| match block {
            ContentBlock::Text { text } => text.clone(),
            ContentBlock::ToolUse { name, input, .. } => {
                format!("[tool_use:{name}] {}", input)
            }
            ContentBlock::ToolResult {
                tool_use_id,
                content,
            } => {
                format!("[tool_result:{tool_use_id}] {}", content)
            }
            ContentBlock::Image { media_type, .. } => format!("[image:{media_type}]"),
        })
        .collect::<Vec<_>>()
        .join(" ");

    format!("- {role}: {text}")
}

#[async_trait]
impl CompactionStrategy for SummarizingCompactor {
    async fn compact(
        &self,
        messages: &[Message],
        target_tokens: usize,
    ) -> Result<CompactionResult, CompactionError> {
        let before_tokens = ConversationBudget::estimate_tokens(messages);
        if before_tokens <= target_tokens {
            return Ok(CompactionResult {
                messages: messages.to_vec(),
                compacted_count: 0,
                estimated_tokens: before_tokens,
                used_summarization: false,
                evicted_indices: Vec::new(),
            });
        }

        let bounds = zone_bounds(messages, self.preserve_recent_turns);
        let protected_middle = protected_middle_indices(messages, &bounds);
        let summarizable_indices = self.summarizable_indices(&bounds, &protected_middle)?;
        let summarizable_messages = cloned_messages_at_indices(messages, &summarizable_indices);
        let summary = self.generate_summary(&summarizable_messages).await?;
        let compacted_messages =
            self.assemble_summarized_messages(messages, &bounds, &protected_middle, &summary);

        let estimated_tokens = ConversationBudget::estimate_tokens(&compacted_messages);
        if estimated_tokens > target_tokens {
            return Err(CompactionError::SummaryExceededTarget);
        }

        Ok(CompactionResult {
            messages: compacted_messages,
            compacted_count: summarizable_messages.len(),
            estimated_tokens,
            used_summarization: true,
            evicted_indices: summarizable_indices,
        })
    }
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum CompactionConfigError {
    #[error("compaction_threshold must be in (0.0, 1.0], got {0}")]
    InvalidThreshold(f32),
    #[error("model_context_limit must be > 0")]
    ZeroContextLimit,
    #[error("reserved_system_tokens ({reserved}) must be < model_context_limit ({limit})")]
    ReservedExceedsLimit { reserved: usize, limit: usize },
    #[error("preserve_recent_turns must be > 0")]
    ZeroPreserveRecent,
    #[error("recompact_cooldown_turns must be > 0")]
    ZeroRecompactCooldown,
    #[error("max_summary_tokens must be > 0")]
    ZeroMaxSummaryTokens,
    #[error(
        "conversation budget too small ({available_tokens}) for preserve_recent_turns={preserve_recent_turns}; minimum required {min_required_tokens} to avoid compaction thrash"
    )]
    ConversationBudgetTooSmall {
        available_tokens: usize,
        preserve_recent_turns: usize,
        min_required_tokens: usize,
    },
}

/// Configuration for conversation-level compaction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactionConfig {
    pub(crate) compaction_threshold: f32,
    pub(crate) preserve_recent_turns: usize,
    pub(crate) model_context_limit: usize,
    pub(crate) reserved_system_tokens: usize,
    pub(crate) recompact_cooldown_turns: u32,
    pub(crate) use_summarization: bool,
    pub(crate) max_summary_tokens: usize,
}

impl CompactionConfig {
    pub fn validate(&self) -> Result<(), CompactionConfigError> {
        self.validate_ranges()?;
        self.validate_budget_floor()
    }

    fn validate_ranges(&self) -> Result<(), CompactionConfigError> {
        if !(0.0 < self.compaction_threshold && self.compaction_threshold <= 1.0) {
            return Err(CompactionConfigError::InvalidThreshold(
                self.compaction_threshold,
            ));
        }
        if self.model_context_limit == 0 {
            return Err(CompactionConfigError::ZeroContextLimit);
        }
        if self.reserved_system_tokens >= self.model_context_limit {
            return Err(CompactionConfigError::ReservedExceedsLimit {
                reserved: self.reserved_system_tokens,
                limit: self.model_context_limit,
            });
        }
        if self.preserve_recent_turns == 0 {
            return Err(CompactionConfigError::ZeroPreserveRecent);
        }
        if self.recompact_cooldown_turns == 0 {
            return Err(CompactionConfigError::ZeroRecompactCooldown);
        }
        if self.max_summary_tokens == 0 {
            return Err(CompactionConfigError::ZeroMaxSummaryTokens);
        }
        Ok(())
    }

    fn validate_budget_floor(&self) -> Result<(), CompactionConfigError> {
        let available_tokens = self.model_context_limit.saturating_sub(
            self.reserved_system_tokens + ConversationBudget::DEFAULT_OUTPUT_RESERVE_TOKENS,
        );
        let min_required_tokens = (self.preserve_recent_turns + 2) * 120;
        if available_tokens < min_required_tokens {
            return Err(CompactionConfigError::ConversationBudgetTooSmall {
                available_tokens,
                preserve_recent_turns: self.preserve_recent_turns,
                min_required_tokens,
            });
        }
        Ok(())
    }

    pub fn build_strategy(&self, llm: Option<Arc<dyn LlmProvider>>) -> Box<dyn CompactionStrategy> {
        if self.use_summarization {
            if let Some(provider) = llm {
                return Box::new(SummarizingCompactor::with_max_summary_tokens(
                    provider,
                    self.preserve_recent_turns,
                    self.max_summary_tokens,
                ));
            }

            tracing::warn!(
                "use_summarization=true but no llm provider available; falling back to SlidingWindowCompactor"
            );
        }

        Box::new(SlidingWindowCompactor::new(self.preserve_recent_turns))
    }
}

impl Default for CompactionConfig {
    fn default() -> Self {
        Self {
            compaction_threshold: 0.80,
            preserve_recent_turns: 6,
            model_context_limit: 128_000,
            reserved_system_tokens: 2_000,
            recompact_cooldown_turns: 2,
            use_summarization: false,
            max_summary_tokens: SummarizingCompactor::DEFAULT_MAX_SUMMARY_TOKENS,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use fx_core::error::LlmError as CoreLlmError;
    use fx_llm::{CompletionRequest, CompletionResponse, ProviderError, ToolCall};
    use std::collections::VecDeque;
    use std::sync::Mutex;

    fn words(count: usize) -> String {
        std::iter::repeat_n("a", count)
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn user(words_count: usize) -> Message {
        Message::user(words(words_count))
    }

    fn assistant(words_count: usize) -> Message {
        Message::assistant(words(words_count))
    }

    fn system(words_count: usize) -> Message {
        Message::system(words(words_count))
    }

    fn tool_use(id: &str) -> Message {
        Message {
            role: MessageRole::Assistant,
            content: vec![ContentBlock::ToolUse {
                id: id.to_string(),
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

    fn has_compaction_marker(messages: &[Message]) -> bool {
        messages.iter().any(message_contains_marker)
    }

    #[derive(Debug)]
    struct MockSummaryLlm {
        responses: Mutex<VecDeque<Result<String, CoreLlmError>>>,
        prompts: Mutex<Vec<String>>,
    }

    impl MockSummaryLlm {
        fn new(responses: Vec<Result<String, CoreLlmError>>) -> Self {
            Self {
                responses: Mutex::new(VecDeque::from(responses)),
                prompts: Mutex::new(Vec::new()),
            }
        }

        fn prompts(&self) -> Vec<String> {
            self.prompts.lock().expect("prompt lock").clone()
        }
    }

    #[async_trait]
    impl LlmProvider for MockSummaryLlm {
        async fn generate(&self, prompt: &str, _: u32) -> Result<String, CoreLlmError> {
            self.prompts
                .lock()
                .expect("prompt lock")
                .push(prompt.to_string());
            self.responses
                .lock()
                .expect("response lock")
                .pop_front()
                .unwrap_or_else(|| Ok("Decisions:\n- none\nFiles modified:\n- none\nTask state:\n- done\nKey context:\n- none".to_string()))
        }

        async fn generate_streaming(
            &self,
            _: &str,
            _: u32,
            _: Box<dyn Fn(String) + Send + 'static>,
        ) -> Result<String, CoreLlmError> {
            Ok(String::new())
        }

        fn model_name(&self) -> &str {
            "mock"
        }

        async fn complete(
            &self,
            _: CompletionRequest,
        ) -> Result<CompletionResponse, ProviderError> {
            Ok(CompletionResponse {
                content: vec![ContentBlock::Text {
                    text: "ok".to_string(),
                }],
                tool_calls: Vec::<ToolCall>::new(),
                usage: None,
                stop_reason: None,
            })
        }
    }

    // 5.1 ConversationBudget tests

    #[test]
    fn budget_with_default_config_has_expected_values() {
        let config = CompactionConfig::default();
        assert_eq!(config.compaction_threshold, 0.80);
        assert_eq!(config.preserve_recent_turns, 6);
        assert_eq!(config.model_context_limit, 128_000);
        assert_eq!(config.reserved_system_tokens, 2_000);
        assert_eq!(config.recompact_cooldown_turns, 2);
        assert!(!config.use_summarization);
        assert_eq!(config.max_summary_tokens, 1_024);
    }

    #[test]
    fn conversation_budget_subtracts_reserved_and_output_reserve() {
        let budget = ConversationBudget::new(16_384, 0.8, 2_000);
        assert_eq!(budget.conversation_budget(), 16_384 - 2_000 - 4_096);
    }

    #[test]
    fn needs_compaction_returns_false_below_threshold() {
        let budget = ConversationBudget::new(5_000, 0.50, 0);
        let messages = vec![user(451)];
        assert!(!budget.needs_compaction(&messages));
    }

    #[test]
    fn needs_compaction_returns_true_at_threshold() {
        let budget = ConversationBudget::new(5_000, 0.50, 0);
        let messages = vec![user(452)];
        assert!(budget.needs_compaction(&messages));
    }

    #[test]
    fn needs_compaction_returns_true_above_threshold() {
        let budget = ConversationBudget::new(5_000, 0.50, 0);
        let messages = vec![user(453)];
        assert!(budget.needs_compaction(&messages));
    }

    #[test]
    fn exceeds_hard_limit_returns_false_within_budget() {
        let budget = ConversationBudget::new(5_000, 0.8, 0);
        let messages = vec![user(900)];
        assert!(!budget.exceeds_hard_limit(&messages));
    }

    #[test]
    fn exceeds_hard_limit_returns_true_above_budget() {
        let budget = ConversationBudget::new(5_000, 0.8, 0);
        let messages = vec![user(905)];
        assert!(budget.exceeds_hard_limit(&messages));
    }

    #[test]
    fn estimate_tokens_empty_messages_returns_zero() {
        assert_eq!(ConversationBudget::estimate_tokens(&[]), 0);
    }

    #[test]
    fn estimate_tokens_matches_existing_heuristic() {
        let text = "abcd ef";
        let chars_div4 = text.chars().count().div_ceil(4);
        let words = text.split_whitespace().count();
        assert_eq!(estimate_text_tokens(text), chars_div4.max(words));
    }

    #[test]
    fn estimate_text_tokens_empty_and_whitespace_are_zero() {
        assert_eq!(estimate_text_tokens(""), 0);
        assert_eq!(estimate_text_tokens("   \t\n"), 0);
    }

    #[test]
    fn estimate_text_tokens_single_character_is_one() {
        assert_eq!(estimate_text_tokens("a"), 1);
    }

    #[test]
    fn estimate_text_tokens_unicode_uses_char_count_heuristic() {
        assert_eq!(estimate_text_tokens("🦝"), 1);
    }

    #[test]
    fn estimate_content_tokens_uses_fixed_cost_for_images() {
        let image = ContentBlock::Image {
            media_type: "image/jpeg".to_string(),
            data: "tiny".to_string(),
        };

        assert_eq!(estimate_content_tokens(&image), IMAGE_TOKEN_ESTIMATE);
    }

    // 5.2 SlidingWindowCompactor tests

    #[tokio::test]
    async fn compact_below_target_is_noop() {
        let compactor = SlidingWindowCompactor::new(3);
        let messages = vec![user(5), assistant(5), user(5)];
        let target = ConversationBudget::estimate_tokens(&messages) + 1;
        let result = compactor.compact(&messages, target).await.expect("compact");
        assert_eq!(result.messages, messages);
        assert_eq!(result.compacted_count, 0);
    }

    #[tokio::test]
    async fn evicted_indices_empty_when_no_compaction_needed() {
        let compactor = SlidingWindowCompactor::new(3);
        let messages = vec![user(5), assistant(5), user(5)];
        let target = ConversationBudget::estimate_tokens(&messages) + 1;

        let result = compactor.compact(&messages, target).await.expect("compact");

        assert!(result.evicted_indices.is_empty());
    }

    #[tokio::test]
    async fn compact_preserves_recent_turns() {
        let compactor = SlidingWindowCompactor::new(4);
        let messages = vec![
            user(25),
            assistant(25),
            user(25),
            assistant(25),
            user(25),
            assistant(25),
            user(25),
            assistant(25),
        ];

        let result = compactor.compact(&messages, 120).await.expect("compact");
        assert_eq!(
            &result.messages[result.messages.len() - 4..],
            &messages[4..]
        );
    }

    #[tokio::test]
    async fn compact_preserves_system_messages() {
        let compactor = SlidingWindowCompactor::new(2);
        let messages = vec![system(10), user(30), assistant(30), user(30), assistant(30)];
        let result = compactor.compact(&messages, 90).await.expect("compact");
        assert_eq!(result.messages.first(), Some(&messages[0]));
    }

    #[tokio::test]
    async fn compact_preserves_prior_compaction_markers() {
        let compactor = SlidingWindowCompactor::new(2);
        let messages = vec![
            Message::assistant("[context compacted: 2 older messages removed]"),
            user(30),
            assistant(30),
            user(30),
            assistant(30),
        ];
        let result = compactor.compact(&messages, 90).await.expect("compact");
        assert!(message_contains_marker(&result.messages[0]));
    }

    #[tokio::test]
    async fn compact_drops_oldest_middle_turns_first() {
        let compactor = SlidingWindowCompactor::new(2);
        let oldest = Message::user(format!("old {}", words(29)));
        let second = Message::assistant(format!("second {}", words(29)));
        let newer = Message::user(format!("newer {}", words(29)));
        let newer2 = Message::assistant(format!("newer2 {}", words(29)));
        let messages = vec![
            oldest.clone(),
            second.clone(),
            newer.clone(),
            newer2.clone(),
        ];

        let result = compactor.compact(&messages, 95).await.expect("compact");
        assert!(!result.messages.contains(&oldest));
        assert!(result.messages.contains(&newer));
    }

    #[tokio::test]
    async fn evicted_indices_populated_for_sliding_window() {
        let compactor = SlidingWindowCompactor::new(2);
        let messages = vec![
            Message::system("system"),
            user(30),
            assistant(30),
            user(30),
            assistant(30),
        ];

        let result = compactor.compact(&messages, 95).await.expect("compact");

        assert_eq!(result.evicted_indices, vec![1, 2]);
    }

    #[tokio::test]
    async fn compact_inserts_truncation_marker() {
        let compactor = SlidingWindowCompactor::new(1);
        let messages = vec![user(20), assistant(20), user(20), assistant(20)];
        let result = compactor.compact(&messages, 45).await.expect("compact");
        assert!(has_compaction_marker(&result.messages));
    }

    #[tokio::test]
    async fn compact_preserves_active_tool_chain() {
        let compactor = SlidingWindowCompactor::new(2);
        let active = tool_use("active-1");
        let messages = vec![
            user(50),
            active.clone(),
            user(20),
            assistant(20),
            user(20),
            assistant(20),
        ];

        let result = compactor.compact(&messages, 95).await.expect("compact");
        assert!(result.messages.contains(&active));
    }

    #[tokio::test]
    async fn compact_handles_empty_history() {
        let compactor = SlidingWindowCompactor::new(2);
        let result = compactor.compact(&[], 20).await.expect("compact");
        assert!(result.messages.is_empty());
        assert_eq!(result.compacted_count, 0);
    }

    #[tokio::test]
    async fn compact_handles_single_message() {
        let compactor = SlidingWindowCompactor::new(2);
        let messages = vec![user(10)];
        let result = compactor.compact(&messages, 20).await.expect("compact");
        assert_eq!(result.messages, messages);
    }

    #[tokio::test]
    async fn compact_all_messages_protected_returns_error() {
        let compactor = SlidingWindowCompactor::new(4);
        let messages = vec![user(40), assistant(40), user(40), assistant(40)];
        let error = compactor.compact(&messages, 20).await.expect_err("error");
        assert!(matches!(error, CompactionError::AllMessagesProtected));
    }

    #[tokio::test]
    async fn compact_large_tool_result_removed_when_not_active() {
        let compactor = SlidingWindowCompactor::new(2);
        let old_use = tool_use("old");
        let old_result = tool_result("old", 120);
        let messages = vec![
            old_use,
            old_result.clone(),
            user(10),
            assistant(10),
            user(10),
        ];

        let result = compactor.compact(&messages, 60).await.expect("compact");
        assert!(!result.messages.contains(&old_result));
    }

    #[tokio::test]
    async fn compact_result_reports_correct_counts() {
        let compactor = SlidingWindowCompactor::new(1);
        let messages = vec![user(30), assistant(30), user(30), assistant(30), user(30)];
        let result = compactor.compact(&messages, 65).await.expect("compact");
        assert!(result.compacted_count > 0);
        assert_eq!(
            messages.len() + 1 - result.messages.len(),
            result.compacted_count
        );
    }

    // 5.3 SummarizingCompactor tests

    #[tokio::test]
    async fn summarize_produces_structured_output() {
        let llm = Arc::new(MockSummaryLlm::new(vec![Ok(
            "Decisions:\n- keep\nFiles modified:\n- src/lib.rs\nTask state:\n- in progress\nKey context:\n- tests failing"
                .to_string(),
        )]));
        let compactor = SummarizingCompactor::new(llm, 2);
        let messages = vec![user(40), assistant(40), user(30), assistant(30), user(20)];

        let result = compactor.compact(&messages, 120).await.expect("compact");
        assert!(result.used_summarization);
        assert_eq!(
            result
                .messages
                .iter()
                .filter(|message| {
                    text_blocks(message).any(|text| text.starts_with(SUMMARY_MARKER_PREFIX))
                })
                .count(),
            1
        );
    }

    #[tokio::test]
    async fn evicted_indices_populated_for_summarizing() {
        let llm = Arc::new(MockSummaryLlm::new(vec![Ok(
            "Decisions:\n- keep\nFiles modified:\n- src/lib.rs\nTask state:\n- in progress\nKey context:\n- tests failing"
                .to_string(),
        )]));
        let compactor = SummarizingCompactor::new(llm, 2);
        let messages = vec![
            Message::system("system"),
            user(40),
            assistant(40),
            user(30),
            assistant(30),
            user(20),
        ];

        let result = compactor.compact(&messages, 120).await.expect("compact");

        assert_eq!(result.evicted_indices, vec![1, 2, 3]);
    }

    #[tokio::test]
    async fn summarize_returns_summarization_failed_on_llm_error() {
        let llm = Arc::new(MockSummaryLlm::new(vec![Err(CoreLlmError::Inference(
            "boom".to_string(),
        ))]));
        let compactor = SummarizingCompactor::new(llm, 2);
        let messages = vec![user(40), assistant(40), user(30), assistant(30), user(20)];

        let error = compactor.compact(&messages, 120).await.expect_err("error");
        assert!(matches!(error, CompactionError::SummarizationFailed { .. }));
    }

    #[tokio::test]
    async fn summarize_returns_summarization_failed_on_timeout() {
        let llm = Arc::new(MockSummaryLlm::new(vec![Err(CoreLlmError::ApiRequest(
            "timeout".to_string(),
        ))]));
        let compactor = SummarizingCompactor::new(llm, 2);
        let messages = vec![user(40), assistant(40), user(30), assistant(30), user(20)];

        let error = compactor.compact(&messages, 120).await.expect_err("error");
        assert!(matches!(error, CompactionError::SummarizationFailed { .. }));
    }

    #[tokio::test]
    async fn summarize_returns_summary_exceeded_target_when_summary_too_large() {
        let llm = Arc::new(MockSummaryLlm::new(vec![Ok(words(500))]));
        let compactor = SummarizingCompactor::new(llm, 2);
        let messages = vec![user(40), assistant(40), user(30), assistant(30), user(20)];

        let error = compactor.compact(&messages, 120).await.expect_err("error");
        assert!(matches!(error, CompactionError::SummaryExceededTarget));
    }

    #[tokio::test]
    async fn summarize_respects_target_budget() {
        let llm = Arc::new(MockSummaryLlm::new(vec![Ok(
            "Decisions:\n- x\nFiles modified:\n- y\nTask state:\n- z\nKey context:\n- q"
                .to_string(),
        )]));
        let compactor = SummarizingCompactor::new(llm, 2);
        let messages = vec![user(30), assistant(30), user(30), assistant(30), user(20)];

        let result = compactor.compact(&messages, 110).await.expect("compact");
        assert!(result.estimated_tokens <= 110);
    }

    #[tokio::test]
    async fn summary_preserves_key_context_categories() {
        let llm = Arc::new(MockSummaryLlm::new(vec![Ok(
            "Decisions:\n- keep\nFiles modified:\n- src/main.rs\nTask state:\n- done\nKey context:\n- regression fixed"
                .to_string(),
        )]));
        let provider: Arc<dyn LlmProvider> = llm.clone();
        let compactor = SummarizingCompactor::new(provider, 2);
        let messages = vec![user(35), assistant(35), user(30), assistant(30), user(20)];

        let result = compactor.compact(&messages, 120).await.expect("compact");
        let summary_text = text_blocks(
            result
                .messages
                .iter()
                .find(|message| {
                    text_blocks(message).any(|text| text.starts_with(SUMMARY_MARKER_PREFIX))
                })
                .expect("summary"),
        )
        .collect::<Vec<_>>()
        .join("\n");

        assert!(summary_text.contains("Decisions:"));
        assert!(summary_text.contains("Files modified:"));
        assert!(summary_text.contains("Task state:"));
        assert!(summary_text.contains("Key context:"));

        let prompts = llm.prompts();
        assert!(prompts[0].contains("Sections (required):"));
    }

    // 5.6 CompactionConfig validation tests

    #[test]
    fn config_rejects_threshold_above_one() {
        let mut config = CompactionConfig::default();
        config.compaction_threshold = 1.1;
        assert!(matches!(
            config.validate(),
            Err(CompactionConfigError::InvalidThreshold(_))
        ));
    }

    #[test]
    fn config_rejects_threshold_at_zero() {
        let mut config = CompactionConfig::default();
        config.compaction_threshold = 0.0;
        assert!(matches!(
            config.validate(),
            Err(CompactionConfigError::InvalidThreshold(_))
        ));
    }

    #[test]
    fn config_rejects_negative_threshold() {
        let mut config = CompactionConfig::default();
        config.compaction_threshold = -0.1;
        assert!(matches!(
            config.validate(),
            Err(CompactionConfigError::InvalidThreshold(_))
        ));
    }

    #[test]
    fn config_rejects_zero_context_limit() {
        let mut config = CompactionConfig::default();
        config.model_context_limit = 0;
        assert!(matches!(
            config.validate(),
            Err(CompactionConfigError::ZeroContextLimit)
        ));
    }

    #[test]
    fn config_rejects_reserved_exceeding_limit() {
        let mut config = CompactionConfig::default();
        config.model_context_limit = 2_000;
        config.reserved_system_tokens = 2_000;
        assert!(matches!(
            config.validate(),
            Err(CompactionConfigError::ReservedExceedsLimit { .. })
        ));
    }

    #[test]
    fn config_rejects_zero_preserve() {
        let mut config = CompactionConfig::default();
        config.preserve_recent_turns = 0;
        assert!(matches!(
            config.validate(),
            Err(CompactionConfigError::ZeroPreserveRecent)
        ));
    }

    #[test]
    fn config_rejects_zero_recompact_cooldown() {
        let mut config = CompactionConfig::default();
        config.recompact_cooldown_turns = 0;
        assert!(matches!(
            config.validate(),
            Err(CompactionConfigError::ZeroRecompactCooldown)
        ));
    }

    #[test]
    fn config_rejects_zero_max_summary_tokens() {
        let mut config = CompactionConfig::default();
        config.max_summary_tokens = 0;
        assert!(matches!(
            config.validate(),
            Err(CompactionConfigError::ZeroMaxSummaryTokens)
        ));
    }

    #[test]
    fn config_rejects_tight_budget_that_would_thrash() {
        let config = CompactionConfig {
            model_context_limit: 5_000,
            reserved_system_tokens: 200,
            preserve_recent_turns: 12,
            ..CompactionConfig::default()
        };
        assert!(matches!(
            config.validate(),
            Err(CompactionConfigError::ConversationBudgetTooSmall { .. })
        ));
    }

    // 5.5 edge tests that are local to compaction strategy

    #[tokio::test]
    async fn mid_tool_call_compaction_preserves_in_flight_calls() {
        let compactor = SlidingWindowCompactor::new(2);
        let inflight = tool_use("inflight");
        let messages = vec![
            user(50),
            inflight.clone(),
            user(20),
            assistant(20),
            user(20),
        ];

        let result = compactor.compact(&messages, 80).await.expect("compact");
        assert!(result.messages.contains(&inflight));
    }

    #[tokio::test]
    async fn compaction_with_only_tool_messages() {
        let compactor = SlidingWindowCompactor::new(1);
        let messages = vec![tool_use("a"), tool_result("a", 60), tool_use("b")];

        let result = compactor.compact(&messages, 60).await.expect("compact");
        assert!(result.messages.iter().any(|message| {
            message.content.iter().any(|block| {
                matches!(
                    block,
                    ContentBlock::ToolUse { id, .. } if id == "b"
                )
            })
        }));
    }
}
