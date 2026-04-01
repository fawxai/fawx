use super::{loop_error, truncate_prompt_text, LlmProvider, LoopEngine, EMERGENCY_SUMMARY_TIMEOUT};
use crate::conversation_compactor::{
    assemble_summarized_messages, debug_assert_tool_pair_integrity, emergency_compact,
    generate_summary, has_prunable_blocks, prune_tool_blocks, slide_summarization_plan,
    summary_message, CompactionConfig, CompactionError, CompactionMemoryFlush, CompactionResult,
    ConversationBudget, SlideSummarizationPlan, SlidingWindowCompactor,
};
use crate::streaming::{ErrorCategory, StreamCallback, StreamEvent};
use crate::types::{LoopError, ReasoningContext};
use fx_llm::{ContentBlock, Message, MessageRole};
use fx_session::{SessionMemory, SessionMemoryUpdate};
use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::Mutex;

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

pub(super) struct CompactionSubsystem<'a> {
    compaction_config: &'a CompactionConfig,
    conversation_budget: &'a ConversationBudget,
    compaction_llm: Option<&'a dyn LlmProvider>,
    memory_flush: Option<&'a dyn CompactionMemoryFlush>,
    session_memory: &'a Mutex<SessionMemory>,
    compaction_last_iteration: &'a Mutex<HashMap<CompactionScope, u32>>,
    error_callback: Option<&'a StreamCallback>,
}

impl LoopEngine {
    pub(super) fn compaction(&self) -> CompactionSubsystem<'_> {
        CompactionSubsystem::from_engine(self)
    }

    #[cfg(test)]
    pub(super) async fn compact_if_needed<'messages>(
        &self,
        messages: &'messages [Message],
        scope: CompactionScope,
        iteration: u32,
    ) -> Result<Cow<'messages, [Message]>, LoopError> {
        self.compaction()
            .compact_if_needed(messages, scope, iteration)
            .await
    }

    #[cfg(test)]
    pub(super) async fn extract_memory_from_evicted(
        &self,
        evicted: &[Message],
        summary: Option<&str>,
    ) {
        self.compaction()
            .extract_memory_from_evicted(evicted, summary)
            .await;
    }

    #[cfg(test)]
    pub(super) fn should_skip_compaction(
        &self,
        scope: CompactionScope,
        iteration: u32,
        tier: CompactionTier,
    ) -> bool {
        self.compaction()
            .should_skip_compaction(scope, iteration, tier)
    }

    #[cfg(test)]
    pub(super) async fn summarize_before_slide(
        &self,
        messages: &[Message],
        target_tokens: usize,
        scope: CompactionScope,
    ) -> Result<CompactionResult, LoopError> {
        self.compaction()
            .summarize_before_slide(messages, target_tokens, scope)
            .await
    }
}

impl<'a> CompactionSubsystem<'a> {
    pub(super) fn from_engine(engine: &'a LoopEngine) -> Self {
        Self {
            compaction_config: &engine.compaction_config,
            conversation_budget: &engine.conversation_budget,
            compaction_llm: engine.compaction_llm.as_deref(),
            memory_flush: engine.memory_flush.as_deref(),
            session_memory: engine.session_memory.as_ref(),
            compaction_last_iteration: &engine.compaction_last_iteration,
            error_callback: engine.error_callback.as_ref(),
        }
    }

    pub(super) fn should_skip_compaction(
        &self,
        scope: CompactionScope,
        iteration: u32,
        tier: CompactionTier,
    ) -> bool {
        let last_iteration = self
            .compaction_last_iteration
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(&scope)
            .copied();
        let cooldown_active = compaction_cooldown_active(
            last_iteration,
            iteration,
            self.compaction_config.recompact_cooldown_turns,
        );
        if cooldown_active {
            tracing::debug!(
                scope = scope.as_str(),
                tier = tier.as_str(),
                iteration,
                cooldown_turns = self.compaction_config.recompact_cooldown_turns,
                "compaction tier skipped due to cooldown guard"
            );
        }
        cooldown_active
    }

    pub(super) async fn compact_if_needed<'messages>(
        &self,
        messages: &'messages [Message],
        scope: CompactionScope,
        iteration: u32,
    ) -> Result<Cow<'messages, [Message]>, LoopError> {
        let current = Cow::Borrowed(messages);
        let current = self.apply_prune_tier(current, scope);
        let current = match highest_compaction_tier(
            current.as_ref(),
            self.conversation_budget,
            self.compaction_config,
        ) {
            Some(CompactionTier::Emergency) => self.apply_emergency_tier(current, scope).await?,
            Some(tier @ CompactionTier::Slide)
                if self.should_skip_compaction(scope, iteration, tier) =>
            {
                current
            }
            Some(CompactionTier::Slide) => self.apply_slide_tier(current, scope, iteration).await?,
            Some(CompactionTier::Prune) | None => current,
        };
        debug_assert_tool_pair_integrity(current.as_ref());
        self.ensure_within_hard_limit(scope, current.as_ref())?;
        Ok(current)
    }

    pub(super) fn ensure_within_hard_limit(
        &self,
        scope: CompactionScope,
        messages: &[Message],
    ) -> Result<(), LoopError> {
        let estimated_tokens = ConversationBudget::estimate_tokens(messages);
        let hard_limit_tokens = self.conversation_budget.conversation_budget();
        if estimated_tokens > hard_limit_tokens {
            return Err(context_exceeded_after_compaction_error(
                scope,
                estimated_tokens,
                hard_limit_tokens,
            ));
        }
        Ok(())
    }

    pub(super) async fn extract_memory_from_evicted(
        &self,
        evicted: &[Message],
        summary: Option<&str>,
    ) {
        if let Some(update) = summary.and_then(parse_summary_memory_update) {
            self.apply_session_memory_update(update);
            return;
        }
        self.extract_memory_with_llm(evicted).await;
    }

    fn record_compaction_iteration(&self, scope: CompactionScope, iteration: u32) {
        let mut map = self
            .compaction_last_iteration
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        map.insert(scope, iteration);
    }

    fn log_tier_result(
        &self,
        tier: CompactionTier,
        scope: CompactionScope,
        before_messages: &[Message],
        target_tokens: usize,
        result: &CompactionResult,
    ) {
        let before_tokens = ConversationBudget::estimate_tokens(before_messages);
        tracing::info!(
            scope = scope.as_str(),
            tier = tier.as_str(),
            strategy = if matches!(tier, CompactionTier::Emergency) {
                "emergency"
            } else if result.used_summarization {
                "summarizing"
            } else {
                "sliding_window"
            },
            before_tokens,
            after_tokens = result.estimated_tokens,
            target_tokens,
            usage_ratio_before = self.conversation_budget.usage_ratio(before_messages),
            usage_ratio_after = self.conversation_budget.usage_ratio(&result.messages),
            messages_removed = result.compacted_count,
            tokens_saved = before_tokens.saturating_sub(result.estimated_tokens),
            "conversation compaction tier completed"
        );
    }

    fn collect_evicted_messages(messages: &[Message], evicted_indices: &[usize]) -> Vec<Message> {
        evicted_indices
            .iter()
            .filter_map(|&index| messages.get(index).cloned())
            .collect()
    }

    fn apply_session_memory_update(&self, update: SessionMemoryUpdate) {
        let mut memory = self
            .session_memory
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Err(err) = memory.apply_update(update) {
            tracing::warn!(
                error = %err,
                "auto-extracted memory update rejected (token cap)"
            );
        }
    }

    async fn flush_evicted(
        &self,
        messages: &[Message],
        result: &CompactionResult,
        scope: CompactionScope,
    ) {
        if result.compacted_count == 0 {
            return;
        }
        let evicted = Self::collect_evicted_messages(messages, &result.evicted_indices);
        if let Some(flush) = self.memory_flush {
            let flush_result = if let Some(summary) = result.summary.as_deref() {
                let summary = summary_message(summary);
                flush
                    .flush(std::slice::from_ref(&summary), scope.as_str())
                    .await
            } else if evicted.is_empty() {
                Ok(())
            } else {
                flush.flush(&evicted, scope.as_str()).await
            };
            if let Err(err) = flush_result {
                tracing::warn!(
                    scope = scope.as_str(),
                    error = %err,
                    evicted_count = evicted.len(),
                    "pre-compaction memory flush failed; proceeding without flush"
                );
                self.emit_background_error(
                    ErrorCategory::Memory,
                    format!("Memory flush failed during compaction: {err}"),
                    true,
                );
            }
        }
        self.extract_memory_from_evicted(&evicted, result.summary.as_deref())
            .await;
    }

    async fn extract_memory_with_llm(&self, evicted: &[Message]) {
        let Some(llm) = self.compaction_llm else {
            return;
        };
        if evicted.is_empty() {
            return;
        }
        let prompt = build_extraction_prompt(evicted);
        match llm.generate(&prompt, 512).await {
            Ok(response) => {
                if let Some(update) = parse_extraction_response(&response) {
                    self.apply_session_memory_update(update);
                }
            }
            Err(err) => {
                tracing::warn!(error = %err, "memory extraction from evicted messages failed")
            }
        }
    }

    async fn apply_follow_up_slide(
        &self,
        result: CompactionResult,
        target_tokens: usize,
        scope: CompactionScope,
    ) -> CompactionResult {
        if result.estimated_tokens <= target_tokens {
            return result;
        }
        match self
            .run_sliding_compaction(&result.messages, scope, target_tokens)
            .await
        {
            Ok(follow_up) => merge_summarized_follow_up(result, follow_up),
            Err(error) => {
                tracing::warn!(
                    scope = scope.as_str(),
                    tier = CompactionTier::Slide.as_str(),
                    error = ?error,
                    "follow-up slide after summarization failed; keeping summary result"
                );
                result
            }
        }
    }

    async fn finish_tier<'messages>(
        &self,
        tier: CompactionTier,
        current: Cow<'messages, [Message]>,
        result: CompactionResult,
        context: FinishTierContext,
    ) -> Cow<'messages, [Message]> {
        let before_tokens = ConversationBudget::estimate_tokens(current.as_ref());
        let after_tokens = result.estimated_tokens;
        self.flush_evicted(current.as_ref(), &result, context.scope)
            .await;
        if let Some(iteration) = context.iteration {
            self.record_compaction_iteration(context.scope, iteration);
        }
        self.log_tier_result(
            tier,
            context.scope,
            current.as_ref(),
            context.target_tokens,
            &result,
        );
        if result.compacted_count > 0 {
            self.emit_stream_event(StreamEvent::ContextCompacted {
                tier: tier.as_str().to_string(),
                messages_removed: result.compacted_count,
                tokens_before: before_tokens,
                tokens_after: after_tokens,
                usage_ratio: f64::from(self.conversation_budget.usage_ratio(&result.messages)),
            });
        }
        Cow::Owned(result.messages)
    }

    fn apply_prune_tier<'messages>(
        &self,
        current: Cow<'messages, [Message]>,
        scope: CompactionScope,
    ) -> Cow<'messages, [Message]> {
        if !self
            .conversation_budget
            .at_tier(current.as_ref(), self.compaction_config.prune_threshold)
        {
            return current;
        }
        if let Some(pruned) = self.maybe_prune_tool_blocks(current.as_ref(), scope) {
            return Cow::Owned(pruned);
        }
        current
    }

    async fn summarize_before_slide(
        &self,
        messages: &[Message],
        target_tokens: usize,
        scope: CompactionScope,
    ) -> Result<CompactionResult, LoopError> {
        let plan = slide_summarization_plan(messages, self.compaction_config.preserve_recent_turns)
            .map_err(|error| compaction_failed_error(scope, error))?;
        let summary = match self.summary_llm() {
            Ok(llm) => {
                generate_summary(
                    llm,
                    &plan.evicted_messages,
                    self.compaction_config.max_summary_tokens,
                )
                .await
            }
            Err(error) => Err(error),
        };
        match summary {
            Ok(summary) => {
                let result = summarized_compaction_result(messages, &plan, summary);
                Ok(self
                    .apply_follow_up_slide(result, target_tokens, scope)
                    .await)
            }
            Err(error) => {
                tracing::warn!(
                    scope = scope.as_str(),
                    tier = CompactionTier::Slide.as_str(),
                    error = %error,
                    "pre-slide summarization failed; falling back to lossy slide"
                );
                self.run_sliding_compaction(messages, scope, target_tokens)
                    .await
            }
        }
    }

    async fn best_effort_emergency_summary(
        &self,
        messages: &[Message],
        scope: CompactionScope,
    ) -> Option<CompactionResult> {
        let plan = slide_summarization_plan(messages, self.compaction_config.preserve_recent_turns)
            .ok()?;
        let Ok(llm) = self.summary_llm() else {
            return None;
        };
        let summary_future = generate_summary(
            llm,
            &plan.evicted_messages,
            self.compaction_config.max_summary_tokens,
        );
        match tokio::time::timeout(EMERGENCY_SUMMARY_TIMEOUT, summary_future).await {
            Ok(Ok(summary)) => Some(summarized_compaction_result(messages, &plan, summary)),
            Ok(Err(error)) => {
                tracing::warn!(
                    scope = scope.as_str(),
                    tier = CompactionTier::Emergency.as_str(),
                    error = %error,
                    "emergency summarization failed; falling back to mechanical emergency compaction"
                );
                None
            }
            Err(_) => {
                tracing::warn!(
                    scope = scope.as_str(),
                    tier = CompactionTier::Emergency.as_str(),
                    "emergency summarization timed out; falling back to mechanical emergency compaction"
                );
                None
            }
        }
    }

    async fn apply_slide_tier<'messages>(
        &self,
        current: Cow<'messages, [Message]>,
        scope: CompactionScope,
        iteration: u32,
    ) -> Result<Cow<'messages, [Message]>, LoopError> {
        let target_tokens = self.conversation_budget.compaction_target();
        let result =
            if can_summarize_eviction(self.compaction_config, self.compaction_llm.is_some()) {
                self.summarize_before_slide(current.as_ref(), target_tokens, scope)
                    .await
            } else {
                self.run_sliding_compaction(current.as_ref(), scope, target_tokens)
                    .await
            };
        match result {
            Ok(result) => {
                let context = FinishTierContext {
                    scope,
                    iteration: Some(iteration),
                    target_tokens,
                };
                Ok(self
                    .finish_tier(CompactionTier::Slide, current, result, context)
                    .await)
            }
            Err(error) => {
                tracing::warn!(
                    scope = scope.as_str(),
                    tier = CompactionTier::Slide.as_str(),
                    error = ?error,
                    "conversation compaction tier failed; continuing"
                );
                Ok(current)
            }
        }
    }

    async fn apply_emergency_tier<'messages>(
        &self,
        current: Cow<'messages, [Message]>,
        scope: CompactionScope,
    ) -> Result<Cow<'messages, [Message]>, LoopError> {
        let result =
            if can_summarize_eviction(self.compaction_config, self.compaction_llm.is_some()) {
                self.best_effort_emergency_summary(current.as_ref(), scope)
                    .await
                    .unwrap_or_else(|| {
                        emergency_compact(
                            current.as_ref(),
                            self.compaction_config.preserve_recent_turns,
                        )
                    })
            } else {
                emergency_compact(
                    current.as_ref(),
                    self.compaction_config.preserve_recent_turns,
                )
            };
        let context = FinishTierContext {
            scope,
            iteration: None,
            target_tokens: 0,
        };
        Ok(self
            .finish_tier(CompactionTier::Emergency, current, result, context)
            .await)
    }

    fn maybe_prune_tool_blocks(
        &self,
        messages: &[Message],
        scope: CompactionScope,
    ) -> Option<Vec<Message>> {
        if !self.compaction_config.prune_tool_blocks {
            return None;
        }
        if !has_prunable_blocks(messages, self.compaction_config.preserve_recent_turns) {
            return None;
        }
        let before_tokens = ConversationBudget::estimate_tokens(messages);
        let mut owned = messages.to_vec();
        let result = prune_tool_blocks(
            &mut owned,
            self.compaction_config.preserve_recent_turns,
            self.compaction_config.tool_block_summary_max_chars,
        );
        match result {
            Some(prune_result) => {
                let after_tokens = ConversationBudget::estimate_tokens(&owned);
                tracing::info!(
                    scope = scope.as_str(),
                    tier = CompactionTier::Prune.as_str(),
                    strategy = "prune",
                    before_tokens,
                    after_tokens,
                    target_tokens = 0,
                    usage_ratio_before = self.conversation_budget.usage_ratio(messages),
                    usage_ratio_after = self.conversation_budget.usage_ratio(&owned),
                    pruned_blocks = prune_result.pruned_count,
                    messages_removed = 0,
                    tokens_saved = prune_result.tokens_saved,
                    "conversation compaction tier completed"
                );
                Some(owned)
            }
            None => None,
        }
    }

    async fn run_sliding_compaction(
        &self,
        messages: &[Message],
        scope: CompactionScope,
        target_tokens: usize,
    ) -> Result<CompactionResult, LoopError> {
        SlidingWindowCompactor::new(self.compaction_config.preserve_recent_turns)
            .compact(messages, target_tokens)
            .await
            .map_err(|error| compaction_failed_error(scope, error))
    }

    fn summary_llm(&self) -> Result<&dyn LlmProvider, CompactionError> {
        self.compaction_llm
            .ok_or_else(|| CompactionError::SummarizationFailed {
                source: Box::new(std::io::Error::other("no compaction LLM")),
            })
    }

    fn emit_background_error(
        &self,
        category: ErrorCategory,
        message: impl Into<String>,
        recoverable: bool,
    ) {
        self.emit_stream_event(StreamEvent::Error {
            category,
            message: message.into(),
            recoverable,
        });
    }

    fn emit_stream_event(&self, event: StreamEvent) {
        if let Some(callback) = self.error_callback {
            callback(event);
        }
    }
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
    if end <= start {
        return None;
    }
    Some(&text[start..=end])
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
    loop_error(
        "compaction",
        &format!("compaction_failed: scope={scope} error={error}"),
        true,
    )
}

pub(super) fn context_exceeded_after_compaction_error(
    scope: CompactionScope,
    estimated_tokens: usize,
    hard_limit_tokens: usize,
) -> LoopError {
    loop_error(
        "compaction",
        &format!(
            "context_exceeded_after_compaction: scope={scope} estimated_tokens={estimated_tokens} hard_limit_tokens={hard_limit_tokens}",
        ),
        true,
    )
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
    fn parse_extraction_response_returns_none_for_reversed_braces() {
        assert!(parse_extraction_response("}garbage{").is_none());
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
