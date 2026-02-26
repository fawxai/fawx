//! Perceive-step context assembly utilities.

use crate::types::*;
use serde::{Deserialize, Serialize};

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
    #[allow(clippy::too_many_arguments)]
    pub fn assemble(
        &self,
        perception: PerceptionSnapshot,
        mut working_memory: Vec<WorkingMemoryEntry>,
        mut episodic: Vec<EpisodicMemoryRef>,
        mut semantic: Vec<SemanticMemoryRef>,
        procedures: Vec<ProcedureRef>,
        identity: IdentityContext,
        goal: Goal,
        depth: u32,
        parent: Option<Box<ReasoningContext>>,
    ) -> ReasoningContext {
        working_memory.sort_by(|left, right| right.relevance.total_cmp(&left.relevance));
        working_memory.truncate(self.max_working_memory);

        episodic.sort_by(|left, right| right.relevance.total_cmp(&left.relevance));
        episodic.truncate(self.max_episodic);

        semantic.sort_by(|left, right| right.confidence.total_cmp(&left.confidence));
        semantic.truncate(self.max_semantic);

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
        let mut total = 24;

        total += estimate_text_tokens(&context.perception.active_app);
        total += estimate_text_tokens(&context.perception.screen.current_app);
        total += estimate_text_tokens(&context.perception.screen.text_content);

        total += context
            .perception
            .screen
            .elements
            .iter()
            .map(|element| {
                4 + estimate_text_tokens(&element.id)
                    + estimate_text_tokens(&element.element_type)
                    + estimate_text_tokens(&element.text)
            })
            .sum::<usize>();

        total += context
            .perception
            .notifications
            .iter()
            .map(|notification| {
                6 + estimate_text_tokens(&notification.id)
                    + estimate_text_tokens(&notification.app)
                    + estimate_text_tokens(&notification.title)
                    + estimate_text_tokens(&notification.content)
            })
            .sum::<usize>();

        if let Some(sensor) = &context.perception.sensor_data {
            total += 6;
            if sensor.location.is_some() {
                total += 8;
            }
            if sensor.battery_percent.is_some() {
                total += 2;
            }
        }

        if let Some(user_input) = &context.perception.user_input {
            total += 4 + estimate_text_tokens(&user_input.text);
            if let Some(context_id) = &user_input.context_id {
                total += estimate_text_tokens(context_id);
            }
        }

        total += context
            .working_memory
            .iter()
            .map(|entry| 3 + estimate_text_tokens(&entry.key) + estimate_text_tokens(&entry.value))
            .sum::<usize>();

        total += context
            .relevant_episodic
            .iter()
            .map(|entry| 3 + estimate_text_tokens(&entry.summary))
            .sum::<usize>();

        total += context
            .relevant_semantic
            .iter()
            .map(|entry| 3 + estimate_text_tokens(&entry.fact))
            .sum::<usize>();

        total += context
            .active_procedures
            .iter()
            .map(|procedure| {
                3 + estimate_text_tokens(&procedure.id) + estimate_text_tokens(&procedure.name)
            })
            .sum::<usize>();

        if let Some(user_name) = &context.identity_context.user_name {
            total += estimate_text_tokens(user_name);
        }

        total += context
            .identity_context
            .preferences
            .iter()
            .map(|(key, value)| 2 + estimate_text_tokens(key) + estimate_text_tokens(value))
            .sum::<usize>();

        total += context
            .identity_context
            .personality_traits
            .iter()
            .map(|trait_text| estimate_text_tokens(trait_text))
            .sum::<usize>();

        total += 4 + estimate_text_tokens(&context.goal.description);
        total += context
            .goal
            .success_criteria
            .iter()
            .map(|criterion| estimate_text_tokens(criterion))
            .sum::<usize>();

        if context.goal.max_steps.is_some() {
            total += 1;
        }

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

fn estimate_text_tokens(text: &str) -> usize {
    if text.trim().is_empty() {
        return 0;
    }

    let char_estimate = (text.chars().count() + 3) / 4;
    let word_estimate = text.split_whitespace().count();
    char_estimate.max(word_estimate).max(1)
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

    let working = context
        .working_memory
        .iter()
        .enumerate()
        .min_by(|(_, left), (_, right)| left.relevance.total_cmp(&right.relevance))
        .map(|(idx, entry)| (EntryKind::Working, idx, entry.relevance));

    let episodic = context
        .relevant_episodic
        .iter()
        .enumerate()
        .min_by(|(_, left), (_, right)| left.relevance.total_cmp(&right.relevance))
        .map(|(idx, entry)| (EntryKind::Episodic, idx, entry.relevance));

    let semantic = context
        .relevant_semantic
        .iter()
        .enumerate()
        .min_by(|(_, left), (_, right)| left.confidence.total_cmp(&right.confidence))
        .map(|(idx, entry)| (EntryKind::Semantic, idx, entry.confidence));

    let candidate = [working, episodic, semantic]
        .into_iter()
        .flatten()
        .min_by(|left, right| left.2.total_cmp(&right.2));

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
    if let Some((oldest_idx, _)) = context
        .relevant_episodic
        .iter()
        .enumerate()
        .min_by_key(|(_, entry)| entry.timestamp_ms)
    {
        context.relevant_episodic.remove(oldest_idx);
        return true;
    }

    if !context.working_memory.is_empty() {
        context.working_memory.remove(0);
        return true;
    }

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
    use ct_core::types::ScreenState;
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
    fn assemble_respects_limits_and_relevance_order() {
        let assembler = PerceptionAssembler::new(2, 2, 2, 2_000);

        let context = assembler.assemble(
            sample_perception("Inbox"),
            vec![
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
            vec![
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
            vec![
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
            vec![],
            sample_identity(),
            sample_goal(),
            0,
            None,
        );

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
        let mut base_context = PerceptionAssembler::new(2, 1, 1, 10_000).assemble(
            sample_perception("Messages"),
            vec![WorkingMemoryEntry {
                key: "thread_id".to_owned(),
                value: "42".to_owned(),
                relevance: 0.8,
            }],
            vec![],
            vec![],
            vec![],
            sample_identity(),
            sample_goal(),
            0,
            None,
        );

        let base_tokens = PerceptionAssembler::estimate_tokens(&base_context);

        base_context.working_memory.push(WorkingMemoryEntry {
            key: "draft_reply".to_owned(),
            value: "Thanks for the update — I will review and get back to you shortly."
                .to_owned(),
            relevance: 0.7,
        });

        let expanded_tokens = PerceptionAssembler::estimate_tokens(&base_context);
        assert!(expanded_tokens > base_tokens);
    }

    #[test]
    fn assemble_trims_context_when_over_budget() {
        let assembler = PerceptionAssembler::new(10, 10, 10, 90);

        let long_text = "very long memory entry repeated repeated repeated repeated";

        let context = assembler.assemble(
            sample_perception("Chat"),
            (0..6)
                .map(|index| WorkingMemoryEntry {
                    key: format!("wm-{index}"),
                    value: long_text.to_owned(),
                    relevance: 0.9 - index as f32 * 0.1,
                })
                .collect(),
            (0..5)
                .map(|index| EpisodicMemoryRef {
                    id: index,
                    summary: long_text.to_owned(),
                    relevance: 0.8 - index as f32 * 0.1,
                    timestamp_ms: index,
                })
                .collect(),
            (0..5)
                .map(|index| SemanticMemoryRef {
                    id: index,
                    fact: long_text.to_owned(),
                    confidence: 0.8 - index as f32 * 0.1,
                })
                .collect(),
            vec![],
            sample_identity(),
            sample_goal(),
            0,
            None,
        );

        let estimated = PerceptionAssembler::estimate_tokens(&context);
        assert!(estimated <= 90, "estimated tokens: {estimated}");

        let retained_entries = context.working_memory.len()
            + context.relevant_episodic.len()
            + context.relevant_semantic.len();
        assert!(retained_entries < 16);
    }
}
