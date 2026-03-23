//! Perceive-step context assembly utilities.

use crate::budget::BudgetSnapshot;
use crate::conversation_compactor::estimate_text_tokens;
use crate::types::*;
use fx_core::types::{Notification, UiElement, UserInput};
use fx_llm::{DocumentAttachment, ImageAttachment, Message};
use serde::{Deserialize, Serialize};

/// Processed perception payload passed from Perceive to Reason.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessedPerception {
    /// Latest user-facing text input.
    pub user_message: String,
    /// Base64-encoded images attached to the latest user turn.
    #[serde(default)]
    pub images: Vec<ImageAttachment>,
    /// Base64-encoded documents attached to the latest user turn.
    #[serde(default)]
    pub documents: Vec<DocumentAttachment>,
    /// Conversation context assembled for this reasoning turn.
    pub context_window: Vec<Message>,
    /// Goals currently active in this loop.
    pub active_goals: Vec<String>,
    /// Remaining budget snapshot captured at perception time.
    pub budget_remaining: BudgetSnapshot,
    /// Latest steer text provided by the user, if any.
    pub steer_context: Option<String>,
}

/// Assembles a [`ReasoningContext`] from perception and memory retrieval outputs.
///
/// This is the Perceive step of the kernel loop.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerceptionAssembler {
    /// Maximum number of working memory entries to include.
    max_working_memory: usize,
    /// Maximum number of episodic memories to retrieve.
    max_episodic: usize,
    /// Maximum number of semantic facts to include.
    max_semantic: usize,
    /// Maximum total context size in tokens (approximate).
    max_context_tokens: usize,
}

/// Inputs for assembling a [`ReasoningContext`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssembleInput {
    pub perception: PerceptionSnapshot,
    pub working_memory: Vec<WorkingMemoryEntry>,
    pub episodic: Vec<EpisodicMemoryRef>,
    pub semantic: Vec<SemanticMemoryRef>,
    pub procedures: Vec<ProcedureRef>,
    pub identity: IdentityContext,
    pub goal: Goal,
    pub depth: u32,
    pub parent: Option<Box<ReasoningContext>>,
}

const BASE_CONTEXT_OVERHEAD_TOKENS: usize = 24;
const SCREEN_ELEMENT_OVERHEAD_TOKENS: usize = 4;
const NOTIFICATION_OVERHEAD_TOKENS: usize = 6;
const SENSOR_BASE_OVERHEAD_TOKENS: usize = 6;
const SENSOR_LOCATION_TOKENS: usize = 8;
const SENSOR_BATTERY_TOKENS: usize = 2;
const USER_INPUT_OVERHEAD_TOKENS: usize = 4;
const STEER_CONTEXT_OVERHEAD_TOKENS: usize = 3;
const CONVERSATION_MESSAGE_OVERHEAD_TOKENS: usize = 4;
const MEMORY_ENTRY_OVERHEAD_TOKENS: usize = 3;
const GOAL_OVERHEAD_TOKENS: usize = 4;
const PREFERENCE_ENTRY_OVERHEAD_TOKENS: usize = 2;
const IMAGE_TOKEN_ESTIMATE: usize = 1600;
const DOCUMENT_TOKEN_ESTIMATE: usize = 3200;

impl PerceptionAssembler {
    /// Create a new [`PerceptionAssembler`] with fixed limits.
    pub fn new(
        max_working_memory: usize,
        max_episodic: usize,
        max_semantic: usize,
        max_context_tokens: usize,
    ) -> Self {
        Self {
            max_working_memory,
            max_episodic,
            max_semantic,
            max_context_tokens,
        }
    }

    /// Assemble a [`ReasoningContext`] from raw inputs.
    ///
    /// Retrieved memories are prioritized by relevance/confidence and trimmed to fit
    /// the configured context token budget.
    pub fn assemble(&self, input: AssembleInput) -> ReasoningContext {
        let AssembleInput {
            perception,
            mut working_memory,
            mut episodic,
            mut semantic,
            procedures,
            identity,
            goal,
            depth,
            parent,
        } = input;
        working_memory = retain_top_by_score_preserving_order(
            working_memory,
            self.max_working_memory,
            |entry| entry.relevance,
        );

        episodic.sort_by(|left, right| right.relevance.total_cmp(&left.relevance));
        episodic.truncate(self.max_episodic);

        semantic = retain_top_by_score_preserving_order(semantic, self.max_semantic, |entry| {
            entry.confidence
        });

        let mut context = ReasoningContext {
            perception,
            working_memory,
            relevant_episodic: episodic,
            relevant_semantic: semantic,
            active_procedures: procedures,
            identity_context: identity,
            goal,
            depth,
            parent_context: parent,
        };

        self.trim_to_budget(&mut context, TrimmingPolicy::ByRelevance);
        context
    }

    /// Estimate token count for a [`ReasoningContext`] with a rough heuristic.
    ///
    /// The heuristic combines a character-based estimate with per-structure overhead.
    pub fn estimate_tokens(context: &ReasoningContext) -> usize {
        let mut total = BASE_CONTEXT_OVERHEAD_TOKENS;
        total += estimate_perception_tokens(&context.perception);
        total += estimate_memory_tokens(
            &context.working_memory,
            &context.relevant_episodic,
            &context.relevant_semantic,
            &context.active_procedures,
        );
        total += estimate_identity_tokens(&context.identity_context);
        total += estimate_goal_tokens(&context.goal);
        if let Some(parent) = &context.parent_context {
            total += Self::estimate_tokens(parent);
        }
        total
    }

    fn trim_to_budget(&self, context: &mut ReasoningContext, policy: TrimmingPolicy) {
        while Self::estimate_tokens(context) > self.max_context_tokens {
            if remove_next_memory_entry(context, policy) {
                continue;
            }

            if !context.active_procedures.is_empty() {
                context.active_procedures.pop();
                continue;
            }

            if !context.goal.success_criteria.is_empty() {
                context.goal.success_criteria.pop();
                continue;
            }

            if !context.identity_context.personality_traits.is_empty() {
                context.identity_context.personality_traits.pop();
                continue;
            }

            if let Some(first_key) = context.identity_context.preferences.keys().next().cloned() {
                context.identity_context.preferences.remove(&first_key);
                continue;
            }

            if context.parent_context.is_some() {
                context.parent_context = None;
                continue;
            }

            break;
        }
    }
}

fn estimate_perception_tokens(perception: &PerceptionSnapshot) -> usize {
    let mut total = estimate_text_tokens(&perception.active_app)
        + estimate_text_tokens(&perception.screen.current_app)
        + estimate_text_tokens(&perception.screen.text_content);

    total += estimate_screen_element_tokens(&perception.screen.elements);
    total += estimate_notification_tokens(&perception.notifications);
    total += estimate_sensor_tokens(&perception.sensor_data);
    total += estimate_user_input_tokens(&perception.user_input);
    total += estimate_steer_context_tokens(&perception.steer_context);
    total += estimate_conversation_history_tokens(&perception.conversation_history);
    total
}

fn estimate_screen_element_tokens(elements: &[UiElement]) -> usize {
    elements
        .iter()
        .map(|el| {
            SCREEN_ELEMENT_OVERHEAD_TOKENS
                + estimate_text_tokens(&el.id)
                + estimate_text_tokens(&el.element_type)
                + estimate_text_tokens(&el.text)
        })
        .sum()
}

fn estimate_notification_tokens(notifications: &[Notification]) -> usize {
    notifications
        .iter()
        .map(|n| {
            NOTIFICATION_OVERHEAD_TOKENS
                + estimate_text_tokens(&n.id)
                + estimate_text_tokens(&n.app)
                + estimate_text_tokens(&n.title)
                + estimate_text_tokens(&n.content)
        })
        .sum()
}

fn estimate_sensor_tokens(sensor_data: &Option<SensorData>) -> usize {
    let Some(sensor) = sensor_data else { return 0 };
    let mut total = SENSOR_BASE_OVERHEAD_TOKENS;
    if sensor.location.is_some() {
        total += SENSOR_LOCATION_TOKENS;
    }
    if sensor.battery_percent.is_some() {
        total += SENSOR_BATTERY_TOKENS;
    }
    total
}

fn estimate_user_input_tokens(user_input: &Option<UserInput>) -> usize {
    let Some(input) = user_input else { return 0 };
    let mut total = USER_INPUT_OVERHEAD_TOKENS + estimate_text_tokens(&input.text);
    if let Some(ctx_id) = &input.context_id {
        total += estimate_text_tokens(ctx_id);
    }
    total
}

fn estimate_steer_context_tokens(steer_context: &Option<String>) -> usize {
    steer_context
        .as_ref()
        .map(|text| STEER_CONTEXT_OVERHEAD_TOKENS + estimate_text_tokens(text))
        .unwrap_or(0)
}

fn estimate_conversation_history_tokens(history: &[Message]) -> usize {
    history
        .iter()
        .map(|message| {
            CONVERSATION_MESSAGE_OVERHEAD_TOKENS
                + message
                    .content
                    .iter()
                    .map(|content| match content {
                        fx_llm::ContentBlock::Text { text } => estimate_text_tokens(text),
                        fx_llm::ContentBlock::ToolUse { name, .. } => estimate_text_tokens(name),
                        fx_llm::ContentBlock::ToolResult { tool_use_id, .. } => {
                            estimate_text_tokens(tool_use_id)
                        }
                        fx_llm::ContentBlock::Image { .. } => IMAGE_TOKEN_ESTIMATE,
                        fx_llm::ContentBlock::Document { .. } => DOCUMENT_TOKEN_ESTIMATE,
                    })
                    .sum::<usize>()
        })
        .sum()
}

fn estimate_memory_tokens(
    working: &[WorkingMemoryEntry],
    episodic: &[EpisodicMemoryRef],
    semantic: &[SemanticMemoryRef],
    procedures: &[ProcedureRef],
) -> usize {
    let working_tokens: usize = working
        .iter()
        .map(|e| {
            MEMORY_ENTRY_OVERHEAD_TOKENS
                + estimate_text_tokens(&e.key)
                + estimate_text_tokens(&e.value)
        })
        .sum();

    let episodic_tokens: usize = episodic
        .iter()
        .map(|e| MEMORY_ENTRY_OVERHEAD_TOKENS + estimate_text_tokens(&e.summary))
        .sum();

    let semantic_tokens: usize = semantic
        .iter()
        .map(|e| MEMORY_ENTRY_OVERHEAD_TOKENS + estimate_text_tokens(&e.fact))
        .sum();

    let procedure_tokens: usize = procedures
        .iter()
        .map(|p| {
            MEMORY_ENTRY_OVERHEAD_TOKENS
                + estimate_text_tokens(&p.id)
                + estimate_text_tokens(&p.name)
        })
        .sum();

    working_tokens + episodic_tokens + semantic_tokens + procedure_tokens
}

fn estimate_identity_tokens(identity: &IdentityContext) -> usize {
    let mut total = 0;
    if let Some(name) = &identity.user_name {
        total += estimate_text_tokens(name);
    }
    total += identity
        .preferences
        .iter()
        .map(|(k, v)| {
            PREFERENCE_ENTRY_OVERHEAD_TOKENS + estimate_text_tokens(k) + estimate_text_tokens(v)
        })
        .sum::<usize>();
    total += identity
        .personality_traits
        .iter()
        .map(|t| estimate_text_tokens(t))
        .sum::<usize>();
    total
}

fn estimate_goal_tokens(goal: &Goal) -> usize {
    let mut total = GOAL_OVERHEAD_TOKENS + estimate_text_tokens(&goal.description);
    total += goal
        .success_criteria
        .iter()
        .map(|c| estimate_text_tokens(c))
        .sum::<usize>();
    if goal.max_steps.is_some() {
        total += 1;
    }
    total
}

/// Trimming policy for contexts that exceed budget.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum TrimmingPolicy {
    /// Remove lowest-relevance entries first.
    ByRelevance,
    /// Remove oldest entries first.
    ByRecency,
    /// Remove entries furthest from current goal.
    ByGoalDistance,
}

fn retain_top_by_score_preserving_order<T, F>(entries: Vec<T>, limit: usize, score: F) -> Vec<T>
where
    F: Fn(&T) -> f32,
{
    if entries.len() <= limit {
        return entries;
    }

    let mut indexed = entries
        .into_iter()
        .enumerate()
        .map(|(index, entry)| {
            let entry_score = score(&entry);
            (index, entry, entry_score)
        })
        .collect::<Vec<_>>();

    indexed.sort_by(|left, right| {
        right
            .2
            .total_cmp(&left.2)
            .then_with(|| left.0.cmp(&right.0))
    });
    indexed.truncate(limit);
    indexed.sort_by_key(|(index, _, _)| *index);

    indexed.into_iter().map(|(_, entry, _)| entry).collect()
}

fn remove_next_memory_entry(context: &mut ReasoningContext, policy: TrimmingPolicy) -> bool {
    match policy {
        TrimmingPolicy::ByRelevance => remove_by_relevance(context),
        TrimmingPolicy::ByRecency => remove_by_recency(context),
        TrimmingPolicy::ByGoalDistance => remove_by_goal_distance(context),
    }
}

#[derive(Debug, Clone, Copy)]
enum EntryKind {
    Working,
    Episodic,
    Semantic,
}

fn remove_by_relevance(context: &mut ReasoningContext) -> bool {
    let candidate = find_lowest_relevance_entry(context);
    remove_entry_by_kind(context, candidate)
}

fn find_lowest_relevance_entry(context: &ReasoningContext) -> Option<(EntryKind, usize, f32)> {
    let working = context
        .working_memory
        .iter()
        .enumerate()
        .min_by(|(_, l), (_, r)| l.relevance.total_cmp(&r.relevance))
        .map(|(i, e)| (EntryKind::Working, i, e.relevance));

    let episodic = context
        .relevant_episodic
        .iter()
        .enumerate()
        .min_by(|(_, l), (_, r)| l.relevance.total_cmp(&r.relevance))
        .map(|(i, e)| (EntryKind::Episodic, i, e.relevance));

    let semantic = context
        .relevant_semantic
        .iter()
        .enumerate()
        .min_by(|(_, l), (_, r)| l.confidence.total_cmp(&r.confidence))
        .map(|(i, e)| (EntryKind::Semantic, i, e.confidence));

    [working, episodic, semantic]
        .into_iter()
        .flatten()
        .min_by(|l, r| l.2.total_cmp(&r.2))
}

fn remove_entry_by_kind(
    context: &mut ReasoningContext,
    candidate: Option<(EntryKind, usize, f32)>,
) -> bool {
    match candidate {
        Some((EntryKind::Working, idx, _)) => {
            context.working_memory.remove(idx);
            true
        }
        Some((EntryKind::Episodic, idx, _)) => {
            context.relevant_episodic.remove(idx);
            true
        }
        Some((EntryKind::Semantic, idx, _)) => {
            context.relevant_semantic.remove(idx);
            true
        }
        None => false,
    }
}

fn remove_by_recency(context: &mut ReasoningContext) -> bool {
    if !context.relevant_episodic.is_empty() {
        context
            .relevant_episodic
            .sort_by_key(|entry| entry.timestamp_ms);
        context.relevant_episodic.remove(0);
        return true;
    }

    // Working-memory entries do not carry timestamps; preserve insertion order
    // during assembly so index 0 remains the oldest retained entry.
    if !context.working_memory.is_empty() {
        context.working_memory.remove(0);
        return true;
    }

    // Semantic entries also lack explicit timestamps; insertion order is used as
    // the recency proxy.
    if !context.relevant_semantic.is_empty() {
        context.relevant_semantic.remove(0);
        return true;
    }

    false
}

fn remove_by_goal_distance(context: &mut ReasoningContext) -> bool {
    let goal_text = context.goal.description.to_lowercase();

    let mut candidate: Option<(EntryKind, usize, usize, f32)> = None;

    for (idx, entry) in context.working_memory.iter().enumerate() {
        let overlap = keyword_overlap(&goal_text, &format!("{} {}", entry.key, entry.value));
        let score = entry.relevance;
        update_goal_distance_candidate(&mut candidate, EntryKind::Working, idx, overlap, score);
    }

    for (idx, entry) in context.relevant_episodic.iter().enumerate() {
        let overlap = keyword_overlap(&goal_text, &entry.summary);
        let score = entry.relevance;
        update_goal_distance_candidate(&mut candidate, EntryKind::Episodic, idx, overlap, score);
    }

    for (idx, entry) in context.relevant_semantic.iter().enumerate() {
        let overlap = keyword_overlap(&goal_text, &entry.fact);
        let score = entry.confidence;
        update_goal_distance_candidate(&mut candidate, EntryKind::Semantic, idx, overlap, score);
    }

    match candidate {
        Some((EntryKind::Working, idx, _, _)) => {
            context.working_memory.remove(idx);
            true
        }
        Some((EntryKind::Episodic, idx, _, _)) => {
            context.relevant_episodic.remove(idx);
            true
        }
        Some((EntryKind::Semantic, idx, _, _)) => {
            context.relevant_semantic.remove(idx);
            true
        }
        None => false,
    }
}

fn update_goal_distance_candidate(
    candidate: &mut Option<(EntryKind, usize, usize, f32)>,
    kind: EntryKind,
    index: usize,
    overlap: usize,
    relevance: f32,
) {
    match candidate {
        Some((_, _, current_overlap, current_relevance)) => {
            if overlap < *current_overlap
                || (overlap == *current_overlap && relevance < *current_relevance)
            {
                *candidate = Some((kind, index, overlap, relevance));
            }
        }
        None => {
            *candidate = Some((kind, index, overlap, relevance));
        }
    }
}

fn keyword_overlap(goal_text: &str, candidate_text: &str) -> usize {
    let goal_terms = normalized_terms(goal_text);
    if goal_terms.is_empty() {
        return 0;
    }

    normalized_terms(candidate_text)
        .iter()
        .filter(|term| goal_terms.iter().any(|goal_term| goal_term == *term))
        .count()
}

fn normalized_terms(text: &str) -> Vec<String> {
    text.split_whitespace()
        .map(|token| {
            token
                .chars()
                .filter(|character| character.is_ascii_alphanumeric())
                .collect::<String>()
                .to_lowercase()
        })
        .filter(|token| !token.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use fx_core::types::ScreenState;
    use std::collections::HashMap;

    fn sample_perception(screen_text: &str) -> PerceptionSnapshot {
        PerceptionSnapshot {
            screen: ScreenState {
                current_app: "com.example.chat".to_owned(),
                elements: vec![],
                text_content: screen_text.to_owned(),
            },
            notifications: vec![],
            active_app: "com.example.chat".to_owned(),
            timestamp_ms: 1_700_000_000_000,
            sensor_data: None,
            user_input: None,
            conversation_history: Vec::new(),
            steer_context: None,
        }
    }

    fn sample_identity() -> IdentityContext {
        let mut preferences = HashMap::new();
        preferences.insert("tone".to_owned(), "concise".to_owned());

        IdentityContext {
            user_name: Some("Joe".to_owned()),
            preferences,
            personality_traits: vec!["helpful".to_owned()],
        }
    }

    fn sample_goal() -> Goal {
        Goal::new(
            "Reply to the latest unread message",
            vec!["A reply was sent".to_owned()],
            Some(3),
        )
    }

    #[test]
    fn estimate_conversation_history_uses_fixed_cost_for_images() {
        let history = vec![Message::user_with_images(
            "describe",
            vec![ImageAttachment {
                media_type: "image/png".to_string(),
                data: "small".to_string(),
            }],
        )];

        let tokens = estimate_conversation_history_tokens(&history);

        assert_eq!(
            tokens,
            CONVERSATION_MESSAGE_OVERHEAD_TOKENS
                + IMAGE_TOKEN_ESTIMATE
                + estimate_text_tokens("describe")
        );
    }

    #[test]
    fn assemble_respects_limits_and_relevance_order() {
        let assembler = PerceptionAssembler::new(2, 2, 2, 2_000);

        let context = assembler.assemble(AssembleInput {
            perception: sample_perception("Inbox"),
            working_memory: vec![
                WorkingMemoryEntry {
                    key: "low".to_owned(),
                    value: "low relevance".to_owned(),
                    relevance: 0.2,
                },
                WorkingMemoryEntry {
                    key: "high".to_owned(),
                    value: "high relevance".to_owned(),
                    relevance: 0.95,
                },
                WorkingMemoryEntry {
                    key: "mid".to_owned(),
                    value: "mid relevance".to_owned(),
                    relevance: 0.5,
                },
            ],
            episodic: vec![
                EpisodicMemoryRef {
                    id: 1,
                    summary: "old low relevance episode".to_owned(),
                    relevance: 0.1,
                    timestamp_ms: 10,
                },
                EpisodicMemoryRef {
                    id: 2,
                    summary: "important recent episode".to_owned(),
                    relevance: 0.9,
                    timestamp_ms: 20,
                },
                EpisodicMemoryRef {
                    id: 3,
                    summary: "medium episode".to_owned(),
                    relevance: 0.6,
                    timestamp_ms: 30,
                },
            ],
            semantic: vec![
                SemanticMemoryRef {
                    id: 11,
                    fact: "Low confidence fact".to_owned(),
                    confidence: 0.2,
                },
                SemanticMemoryRef {
                    id: 12,
                    fact: "High confidence fact".to_owned(),
                    confidence: 0.85,
                },
                SemanticMemoryRef {
                    id: 13,
                    fact: "Medium confidence fact".to_owned(),
                    confidence: 0.55,
                },
            ],
            procedures: vec![],
            identity: sample_identity(),
            goal: sample_goal(),
            depth: 0,
            parent: None,
        });

        assert_eq!(context.working_memory.len(), 2);
        assert_eq!(context.working_memory[0].key, "high");
        assert_eq!(context.working_memory[1].key, "mid");

        assert_eq!(context.relevant_episodic.len(), 2);
        assert_eq!(context.relevant_episodic[0].id, 2);
        assert_eq!(context.relevant_episodic[1].id, 3);

        assert_eq!(context.relevant_semantic.len(), 2);
        assert_eq!(context.relevant_semantic[0].id, 12);
        assert_eq!(context.relevant_semantic[1].id, 13);
    }

    #[test]
    fn estimate_tokens_increases_with_added_context() {
        let assembler = PerceptionAssembler::new(2, 1, 1, 10_000);
        let mut base_context = assembler.assemble(AssembleInput {
            perception: sample_perception("Messages"),
            working_memory: vec![WorkingMemoryEntry {
                key: "thread_id".to_owned(),
                value: "42".to_owned(),
                relevance: 0.8,
            }],
            episodic: vec![],
            semantic: vec![],
            procedures: vec![],
            identity: sample_identity(),
            goal: sample_goal(),
            depth: 0,
            parent: None,
        });

        let base_tokens = PerceptionAssembler::estimate_tokens(&base_context);

        base_context.working_memory.push(WorkingMemoryEntry {
            key: "draft_reply".to_owned(),
            value: "Thanks for the update — I will review and get back to you shortly.".to_owned(),
            relevance: 0.7,
        });

        let expanded_tokens = PerceptionAssembler::estimate_tokens(&base_context);
        assert!(expanded_tokens > base_tokens);
    }

    #[test]
    fn assemble_trims_context_when_over_budget() {
        let assembler = PerceptionAssembler::new(10, 10, 10, 90);

        let long_text = "very long memory entry repeated repeated repeated repeated";

        let context = assembler.assemble(AssembleInput {
            perception: sample_perception("Chat"),
            working_memory: (0..6)
                .map(|index| WorkingMemoryEntry {
                    key: format!("wm-{index}"),
                    value: long_text.to_owned(),
                    relevance: 0.9 - index as f32 * 0.1,
                })
                .collect(),
            episodic: (0..5)
                .map(|index| EpisodicMemoryRef {
                    id: index,
                    summary: long_text.to_owned(),
                    relevance: 0.8 - index as f32 * 0.1,
                    timestamp_ms: index,
                })
                .collect(),
            semantic: (0..5)
                .map(|index| SemanticMemoryRef {
                    id: index,
                    fact: long_text.to_owned(),
                    confidence: 0.8 - index as f32 * 0.1,
                })
                .collect(),
            procedures: vec![],
            identity: sample_identity(),
            goal: sample_goal(),
            depth: 0,
            parent: None,
        });

        let estimated = PerceptionAssembler::estimate_tokens(&context);
        assert!(estimated <= 90, "estimated tokens: {estimated}");

        let retained_entries = context.working_memory.len()
            + context.relevant_episodic.len()
            + context.relevant_semantic.len();
        assert!(retained_entries < 16);
    }

    #[test]
    fn by_recency_removes_oldest_working_memory_entry() {
        let assembler = PerceptionAssembler::new(3, 0, 0, 10_000);

        let mut context = assembler.assemble(AssembleInput {
            perception: sample_perception("Chat"),
            working_memory: vec![
                WorkingMemoryEntry {
                    key: "oldest".to_owned(),
                    value: "first inserted".to_owned(),
                    relevance: 0.20,
                },
                WorkingMemoryEntry {
                    key: "newest_high".to_owned(),
                    value: "most relevant but newest".to_owned(),
                    relevance: 0.99,
                },
                WorkingMemoryEntry {
                    key: "middle".to_owned(),
                    value: "middle inserted".to_owned(),
                    relevance: 0.70,
                },
            ],
            episodic: vec![],
            semantic: vec![],
            procedures: vec![],
            identity: sample_identity(),
            goal: sample_goal(),
            depth: 0,
            parent: None,
        });

        assert!(remove_next_memory_entry(
            &mut context,
            TrimmingPolicy::ByRecency
        ));
        assert_eq!(context.working_memory.len(), 2);
        assert!(context
            .working_memory
            .iter()
            .all(|entry| entry.key != "oldest"));
        assert!(context
            .working_memory
            .iter()
            .any(|entry| entry.key == "newest_high"));
    }

    #[test]
    fn by_goal_distance_removes_least_goal_aligned_entry() {
        let mut context = ReasoningContext {
            perception: sample_perception("Messages"),
            working_memory: vec![
                WorkingMemoryEntry {
                    key: "goal_related".to_owned(),
                    value: "reply to alex about calendar".to_owned(),
                    relevance: 0.10,
                },
                WorkingMemoryEntry {
                    key: "unrelated_low".to_owned(),
                    value: "buy milk at grocery".to_owned(),
                    relevance: 0.20,
                },
                WorkingMemoryEntry {
                    key: "unrelated_high".to_owned(),
                    value: "sports scoreboard".to_owned(),
                    relevance: 0.90,
                },
            ],
            relevant_episodic: vec![],
            relevant_semantic: vec![],
            active_procedures: vec![],
            identity_context: sample_identity(),
            goal: Goal::new(
                "reply to alex about calendar",
                vec!["sent calendar reply".to_owned()],
                Some(2),
            ),
            depth: 0,
            parent_context: None,
        };

        assert!(remove_next_memory_entry(
            &mut context,
            TrimmingPolicy::ByGoalDistance
        ));

        assert_eq!(context.working_memory.len(), 2);
        assert!(context
            .working_memory
            .iter()
            .all(|entry| entry.key != "unrelated_low"));
        assert!(context
            .working_memory
            .iter()
            .any(|entry| entry.key == "goal_related"));
    }

    #[test]
    fn assemble_handles_zero_token_budget_without_panicking() {
        let assembler = PerceptionAssembler::new(4, 4, 4, 0);

        let context = assembler.assemble(AssembleInput {
            perception: sample_perception("Chat"),
            working_memory: vec![WorkingMemoryEntry {
                key: "wm".to_owned(),
                value: "value".to_owned(),
                relevance: 0.9,
            }],
            episodic: vec![EpisodicMemoryRef {
                id: 1,
                summary: "episode".to_owned(),
                relevance: 0.8,
                timestamp_ms: 123,
            }],
            semantic: vec![SemanticMemoryRef {
                id: 1,
                fact: "fact".to_owned(),
                confidence: 0.7,
            }],
            procedures: vec![ProcedureRef {
                id: "reply".to_owned(),
                name: "Reply".to_owned(),
                version: 1,
            }],
            identity: sample_identity(),
            goal: sample_goal(),
            depth: 0,
            parent: None,
        });

        assert!(context.working_memory.is_empty());
        assert!(context.relevant_episodic.is_empty());
        assert!(context.relevant_semantic.is_empty());
        assert!(context.active_procedures.is_empty());
        assert!(context.goal.success_criteria.is_empty());
        assert!(context.identity_context.preferences.is_empty());
        assert!(context.identity_context.personality_traits.is_empty());
    }
}
