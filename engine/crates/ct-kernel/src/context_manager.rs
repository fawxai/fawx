//! Context-window compaction utilities for the kernel loop.

use crate::perceive::{PerceptionAssembler, TrimmingPolicy};
use crate::types::*;
use serde::{Deserialize, Serialize};

/// Manages context-window limits by compacting older context.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextCompactor {
    /// Maximum tokens before compaction triggers.
    compaction_threshold: usize,
    /// Target size after compaction.
    target_size: usize,
}

impl ContextCompactor {
    /// Create a new [`ContextCompactor`].
    pub fn new(compaction_threshold: usize, target_size: usize) -> Self {
        Self {
            compaction_threshold,
            target_size,
        }
    }

    /// Check if context needs compaction.
    pub fn needs_compaction(&self, context: &ReasoningContext) -> bool {
        PerceptionAssembler::estimate_tokens(context) > self.compaction_threshold
    }

    /// Compact a context by summarizing older entries.
    ///
    /// Returns a new context trimmed to the configured target token budget.
    pub fn compact(&self, context: ReasoningContext, policy: TrimmingPolicy) -> ReasoningContext {
        let mut compacted = context;
        if PerceptionAssembler::estimate_tokens(&compacted) <= self.target_size {
            return compacted;
        }

        let mut removed_fragments = Vec::new();

        while PerceptionAssembler::estimate_tokens(&compacted) > self.target_size {
            if let Some(fragment) = remove_for_compaction(&mut compacted, policy) {
                removed_fragments.push(fragment);
                continue;
            }

            if !compacted.active_procedures.is_empty() {
                let removed = compacted.active_procedures.remove(0);
                removed_fragments.push(format!("Dropped procedure '{}'.", removed.name));
                continue;
            }

            if !compacted.goal.success_criteria.is_empty() {
                let removed = compacted.goal.success_criteria.remove(0);
                removed_fragments.push(format!("Dropped success criterion '{}'.", removed));
                continue;
            }

            if !compacted.identity_context.personality_traits.is_empty() {
                let removed = compacted.identity_context.personality_traits.remove(0);
                removed_fragments.push(format!("Dropped personality trait '{}'.", removed));
                continue;
            }

            if let Some((key, value)) = compacted
                .identity_context
                .preferences
                .iter()
                .next()
                .map(|(key, value)| (key.clone(), value.clone()))
            {
                compacted.identity_context.preferences.remove(&key);
                removed_fragments.push(format!("Dropped preference {key}={value}."));
                continue;
            }

            if compacted.parent_context.is_some() {
                compacted.parent_context = None;
                removed_fragments.push("Dropped parent context.".to_owned());
                continue;
            }

            break;
        }

        if !removed_fragments.is_empty() {
            upsert_summary_entry(&mut compacted, &removed_fragments);

            while PerceptionAssembler::estimate_tokens(&compacted) > self.target_size {
                if !shrink_summary_entry(&mut compacted) {
                    break;
                }
            }
        }

        compacted
    }
}

#[derive(Debug, Clone, Copy)]
enum EntryKind {
    Working,
    Episodic,
    Semantic,
}

fn remove_for_compaction(context: &mut ReasoningContext, policy: TrimmingPolicy) -> Option<String> {
    match policy {
        TrimmingPolicy::ByRelevance => remove_by_relevance(context),
        TrimmingPolicy::ByRecency => remove_by_recency(context),
        TrimmingPolicy::ByGoalDistance => remove_by_goal_distance(context),
    }
}

fn remove_by_relevance(context: &mut ReasoningContext) -> Option<String> {
    let working = context
        .working_memory
        .iter()
        .enumerate()
        .min_by(|(_, left), (_, right)| left.relevance.total_cmp(&right.relevance))
        .map(|(index, entry)| (EntryKind::Working, index, entry.relevance));

    let episodic = context
        .relevant_episodic
        .iter()
        .enumerate()
        .min_by(|(_, left), (_, right)| left.relevance.total_cmp(&right.relevance))
        .map(|(index, entry)| (EntryKind::Episodic, index, entry.relevance));

    let semantic = context
        .relevant_semantic
        .iter()
        .enumerate()
        .min_by(|(_, left), (_, right)| left.confidence.total_cmp(&right.confidence))
        .map(|(index, entry)| (EntryKind::Semantic, index, entry.confidence));

    let candidate = [working, episodic, semantic]
        .into_iter()
        .flatten()
        .min_by(|left, right| left.2.total_cmp(&right.2));

    match candidate {
        Some((EntryKind::Working, index, _)) => {
            let removed = context.working_memory.remove(index);
            Some(format!("WM: {}={}", removed.key, removed.value))
        }
        Some((EntryKind::Episodic, index, _)) => {
            let removed = context.relevant_episodic.remove(index);
            Some(format!("Episodic: {}", removed.summary))
        }
        Some((EntryKind::Semantic, index, _)) => {
            let removed = context.relevant_semantic.remove(index);
            Some(format!("Semantic: {}", removed.fact))
        }
        None => None,
    }
}

fn remove_by_recency(context: &mut ReasoningContext) -> Option<String> {
    if let Some((oldest_index, _)) = context
        .relevant_episodic
        .iter()
        .enumerate()
        .min_by_key(|(_, entry)| entry.timestamp_ms)
    {
        let removed = context.relevant_episodic.remove(oldest_index);
        return Some(format!("Episodic(old): {}", removed.summary));
    }

    if !context.working_memory.is_empty() {
        let removed = context.working_memory.remove(0);
        return Some(format!("WM(old): {}={}", removed.key, removed.value));
    }

    if !context.relevant_semantic.is_empty() {
        let removed = context.relevant_semantic.remove(0);
        return Some(format!("Semantic(old): {}", removed.fact));
    }

    None
}

fn remove_by_goal_distance(context: &mut ReasoningContext) -> Option<String> {
    let goal_text = context.goal.description.to_lowercase();

    let mut candidate: Option<(EntryKind, usize, usize, f32)> = None;

    for (index, entry) in context.working_memory.iter().enumerate() {
        let overlap = keyword_overlap(&goal_text, &format!("{} {}", entry.key, entry.value));
        update_candidate(
            &mut candidate,
            EntryKind::Working,
            index,
            overlap,
            entry.relevance,
        );
    }

    for (index, entry) in context.relevant_episodic.iter().enumerate() {
        let overlap = keyword_overlap(&goal_text, &entry.summary);
        update_candidate(
            &mut candidate,
            EntryKind::Episodic,
            index,
            overlap,
            entry.relevance,
        );
    }

    for (index, entry) in context.relevant_semantic.iter().enumerate() {
        let overlap = keyword_overlap(&goal_text, &entry.fact);
        update_candidate(
            &mut candidate,
            EntryKind::Semantic,
            index,
            overlap,
            entry.confidence,
        );
    }

    match candidate {
        Some((EntryKind::Working, index, _, _)) => {
            let removed = context.working_memory.remove(index);
            Some(format!("WM(goal-distance): {}={}", removed.key, removed.value))
        }
        Some((EntryKind::Episodic, index, _, _)) => {
            let removed = context.relevant_episodic.remove(index);
            Some(format!("Episodic(goal-distance): {}", removed.summary))
        }
        Some((EntryKind::Semantic, index, _, _)) => {
            let removed = context.relevant_semantic.remove(index);
            Some(format!("Semantic(goal-distance): {}", removed.fact))
        }
        None => None,
    }
}

fn update_candidate(
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

fn upsert_summary_entry(context: &mut ReasoningContext, removed_fragments: &[String]) {
    let max_fragments = 6;
    let mut summary_items: Vec<String> = removed_fragments
        .iter()
        .take(max_fragments)
        .cloned()
        .collect();

    if removed_fragments.len() > max_fragments {
        summary_items.push(format!(
            "...and {} more entries",
            removed_fragments.len() - max_fragments
        ));
    }

    let summary_text = format!("Compacted context summary: {}", summary_items.join("; "));

    if let Some(existing) = context
        .working_memory
        .iter_mut()
        .find(|entry| entry.key == "compacted_context_summary")
    {
        existing.value = summary_text;
        existing.relevance = existing.relevance.max(0.35);
    } else {
        context.working_memory.push(WorkingMemoryEntry {
            key: "compacted_context_summary".to_owned(),
            value: summary_text,
            relevance: 0.35,
        });
    }
}

fn shrink_summary_entry(context: &mut ReasoningContext) -> bool {
    if let Some(index) = context
        .working_memory
        .iter()
        .position(|entry| entry.key == "compacted_context_summary")
    {
        let value = &mut context.working_memory[index].value;
        if value.len() > 64 {
            let new_len = ((value.len() * 2) / 3).max(64);
            value.truncate(new_len);
            return true;
        }

        context.working_memory.remove(index);
        return true;
    }

    if !context.working_memory.is_empty() {
        context.working_memory.pop();
        return true;
    }

    if !context.relevant_episodic.is_empty() {
        context.relevant_episodic.pop();
        return true;
    }

    if !context.relevant_semantic.is_empty() {
        context.relevant_semantic.pop();
        return true;
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use ct_core::types::ScreenState;
    use std::collections::HashMap;

    fn sample_context() -> ReasoningContext {
        let mut preferences = HashMap::new();
        preferences.insert("tone".to_owned(), "concise".to_owned());

        ReasoningContext {
            perception: PerceptionSnapshot {
                screen: ScreenState {
                    current_app: "com.example.mail".to_owned(),
                    elements: vec![],
                    text_content: "Inbox with several unread messages".to_owned(),
                },
                notifications: vec![],
                active_app: "com.example.mail".to_owned(),
                timestamp_ms: 1_700_000_000_000,
                sensor_data: None,
                user_input: None,
            },
            working_memory: (0..8)
                .map(|index| WorkingMemoryEntry {
                    key: format!("wm-{index}"),
                    value: "Long working memory value repeated repeated repeated".to_owned(),
                    relevance: 0.95 - index as f32 * 0.1,
                })
                .collect(),
            relevant_episodic: (0..6)
                .map(|index| EpisodicMemoryRef {
                    id: index,
                    summary: "Long episodic memory summary repeated repeated repeated".to_owned(),
                    relevance: 0.85 - index as f32 * 0.1,
                    timestamp_ms: 1_700_000_000_000 + index,
                })
                .collect(),
            relevant_semantic: (0..6)
                .map(|index| SemanticMemoryRef {
                    id: index,
                    fact: "Long semantic fact repeated repeated repeated".to_owned(),
                    confidence: 0.8 - index as f32 * 0.1,
                })
                .collect(),
            active_procedures: vec![ProcedureRef {
                id: "mail-reply".to_owned(),
                name: "Mail Reply".to_owned(),
                version: 1,
            }],
            identity_context: IdentityContext {
                user_name: Some("Joe".to_owned()),
                preferences,
                personality_traits: vec!["focused".to_owned(), "concise".to_owned()],
            },
            goal: Goal::new(
                "Reply to unread email threads",
                vec!["At least one reply is drafted".to_owned()],
                Some(4),
            ),
            depth: 0,
            parent_context: None,
        }
    }

    #[test]
    fn needs_compaction_detects_threshold_crossing() {
        let context = sample_context();
        let token_estimate = PerceptionAssembler::estimate_tokens(&context);

        let no_compaction = ContextCompactor::new(token_estimate + 10, token_estimate / 2);
        assert!(!no_compaction.needs_compaction(&context));

        let must_compact = ContextCompactor::new(token_estimate.saturating_sub(1), token_estimate / 2);
        assert!(must_compact.needs_compaction(&context));
    }

    #[test]
    fn compact_reduces_context_size_and_meets_target() {
        let context = sample_context();
        let before = PerceptionAssembler::estimate_tokens(&context);

        let target = before / 2;
        let compactor = ContextCompactor::new(before.saturating_sub(1), target);
        let compacted = compactor.compact(context, TrimmingPolicy::ByRelevance);

        let after = PerceptionAssembler::estimate_tokens(&compacted);
        assert!(after < before, "before={before}, after={after}");
        assert!(after <= target, "target={target}, after={after}");

        assert!(
            compacted
                .working_memory
                .iter()
                .any(|entry| entry.key == "compacted_context_summary")
                || compacted.working_memory.len() < 8,
            "expected either summary entry or reduced working memory"
        );
    }
}
