//! Session-scoped structured working memory for Fawx reasoning loops.

pub mod skill;

use serde::{Deserialize, Serialize};
use std::fmt;
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ScratchpadError {
    #[error("entry not found: id={0}")]
    EntryNotFound(u32),
    #[error("parent not found: id={0}")]
    ParentNotFound(u32),
    #[error("label must not be empty")]
    EmptyLabel,
    #[error("content must not be empty")]
    EmptyContent,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EntryKind {
    Hypothesis,
    Observation,
    Conclusion,
    Note,
}

impl fmt::Display for EntryKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Hypothesis => write!(f, "hypothesis"),
            Self::Observation => write!(f, "observation"),
            Self::Conclusion => write!(f, "conclusion"),
            Self::Note => write!(f, "note"),
        }
    }
}

impl EntryKind {
    pub fn from_str_loose(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "hypothesis" => Some(Self::Hypothesis),
            "observation" => Some(Self::Observation),
            "conclusion" => Some(Self::Conclusion),
            "note" => Some(Self::Note),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EntryStatus {
    Active,
    Superseded,
    Invalidated,
}

impl fmt::Display for EntryStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Active => write!(f, "active"),
            Self::Superseded => write!(f, "superseded"),
            Self::Invalidated => write!(f, "invalidated"),
        }
    }
}

impl EntryStatus {
    pub fn from_str_loose(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "active" => Some(Self::Active),
            "superseded" => Some(Self::Superseded),
            "invalidated" => Some(Self::Invalidated),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    High,
    Medium,
    Low,
}

impl fmt::Display for Confidence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::High => write!(f, "high"),
            Self::Medium => write!(f, "medium"),
            Self::Low => write!(f, "low"),
        }
    }
}

impl Confidence {
    pub fn from_str_loose(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "high" => Some(Self::High),
            "medium" | "med" => Some(Self::Medium),
            "low" => Some(Self::Low),
            _ => None,
        }
    }

    fn compaction_priority(self) -> u8 {
        match self {
            Self::High => 2,
            Self::Medium => 1,
            Self::Low => 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScratchpadEntry {
    pub id: u32,
    pub kind: EntryKind,
    pub label: String,
    pub content: String,
    pub confidence: Confidence,
    pub status: EntryStatus,
    pub parent_id: Option<u32>,
    pub created_at_iteration: u32,
    pub updated_at_iteration: u32,
}

/// Character threshold for `render_for_context()` output above which
/// automatic compaction is triggered at iteration boundaries.
pub const SCRATCHPAD_COMPACT_THRESHOLD_CHARS: usize = 4000;

/// Default token budget passed to `compact()` when auto-compacting.
/// Approximation: 4 chars ≈ 1 token, target ≈ threshold / 8.
pub const SCRATCHPAD_COMPACT_TARGET_TOKENS: usize = SCRATCHPAD_COMPACT_THRESHOLD_CHARS / 8;

/// Age threshold (in iterations) for superseded entry eviction during compaction.
pub const SCRATCHPAD_AGE_THRESHOLD: u32 = 5;

#[derive(Debug, Clone, Default)]
pub struct Scratchpad {
    entries: Vec<ScratchpadEntry>,
    next_id: u32,
}

pub struct AddParams {
    pub kind: EntryKind,
    pub label: String,
    pub content: String,
    pub confidence: Confidence,
    pub parent_id: Option<u32>,
    pub iteration: u32,
}

/// Result of adding an entry to the scratchpad.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddResult {
    /// The ID assigned to the new entry.
    pub id: u32,
    /// If the caller specified a parent_id that didn't exist, this holds the
    /// original value. The entry was created as top-level instead.
    pub parent_dropped: Option<u32>,
}

impl Scratchpad {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(&mut self, params: AddParams) -> Result<AddResult, ScratchpadError> {
        if params.label.trim().is_empty() {
            return Err(ScratchpadError::EmptyLabel);
        }
        if params.content.trim().is_empty() {
            return Err(ScratchpadError::EmptyContent);
        }
        let (resolved_parent, parent_dropped) = match params.parent_id {
            Some(pid) if self.entries.iter().any(|e| e.id == pid) => (Some(pid), None),
            Some(pid) => (None, Some(pid)),
            None => (None, None),
        };
        let id = self.next_id;
        self.next_id += 1;
        self.entries.push(ScratchpadEntry {
            id,
            kind: params.kind,
            label: params.label,
            content: params.content,
            confidence: params.confidence,
            status: EntryStatus::Active,
            parent_id: resolved_parent,
            created_at_iteration: params.iteration,
            updated_at_iteration: params.iteration,
        });
        Ok(AddResult { id, parent_dropped })
    }

    pub fn update(
        &mut self,
        id: u32,
        content: Option<String>,
        confidence: Option<Confidence>,
        status: Option<EntryStatus>,
        iteration: u32,
    ) -> Result<(), ScratchpadError> {
        let entry = self
            .entries
            .iter_mut()
            .find(|e| e.id == id)
            .ok_or(ScratchpadError::EntryNotFound(id))?;
        if let Some(c) = content {
            entry.content = c;
        }
        if let Some(conf) = confidence {
            entry.confidence = conf;
        }
        if let Some(s) = status {
            entry.status = s;
        }
        entry.updated_at_iteration = iteration;
        Ok(())
    }

    pub fn remove(&mut self, id: u32) -> Result<ScratchpadEntry, ScratchpadError> {
        let idx = self
            .entries
            .iter()
            .position(|e| e.id == id)
            .ok_or(ScratchpadError::EntryNotFound(id))?;
        let removed = self.entries.remove(idx);
        // Re-parent orphaned children to None (root level).
        for entry in &mut self.entries {
            if entry.parent_id == Some(id) {
                entry.parent_id = None;
            }
        }
        Ok(removed)
    }

    pub fn get(&self, id: u32) -> Option<&ScratchpadEntry> {
        self.entries.iter().find(|e| e.id == id)
    }

    pub fn active_entries(&self) -> Vec<&ScratchpadEntry> {
        self.entries
            .iter()
            .filter(|e| e.status != EntryStatus::Invalidated)
            .collect()
    }

    pub fn entries_by_kind(&self, kind: EntryKind) -> Vec<&ScratchpadEntry> {
        self.entries.iter().filter(|e| e.kind == kind).collect()
    }

    pub fn all_entries(&self) -> &[ScratchpadEntry] {
        &self.entries
    }

    pub fn render_for_context(&self) -> String {
        let active: Vec<&ScratchpadEntry> = self.active_entries();
        if active.is_empty() {
            return "## Scratchpad\n(empty -- use scratchpad_add to track hypotheses, observations, conclusions)".to_string();
        }
        let mut lines =
            vec!["## Scratchpad (your working notes -- update as you learn)".to_string()];
        for kind in &[
            EntryKind::Hypothesis,
            EntryKind::Observation,
            EntryKind::Conclusion,
            EntryKind::Note,
        ] {
            let group: Vec<&&ScratchpadEntry> = active.iter().filter(|e| e.kind == *kind).collect();
            if group.is_empty() {
                continue;
            }
            lines.push(format!("\n### {}s", kind_label(*kind)));
            for entry in group {
                let status_tag = if entry.status == EntryStatus::Superseded {
                    " [SUPERSEDED]"
                } else {
                    ""
                };
                let parent_tag = entry
                    .parent_id
                    .map(|pid| format!(" (parent: #{pid})"))
                    .unwrap_or_default();
                lines.push(format!(
                    "- **#{}** [{}] {}: {}{status_tag}{parent_tag}",
                    entry.id, entry.confidence, entry.label, entry.content
                ));
            }
        }
        lines.join("\n")
    }

    pub fn estimated_tokens(&self) -> usize {
        let rendered = self.render_for_context();
        let word_count = rendered.split_whitespace().count();
        ((word_count as f64) / 0.75).ceil() as usize
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn compact(&mut self, max_tokens: usize, current_iteration: u32, age_threshold: u32) {
        if self.estimated_tokens() > max_tokens {
            self.entries
                .retain(|e| e.status != EntryStatus::Invalidated);
        }
        if self.estimated_tokens() > max_tokens {
            self.entries.retain(|e| {
                if e.status != EntryStatus::Superseded {
                    return true;
                }
                current_iteration.saturating_sub(e.updated_at_iteration) < age_threshold
            });
        }
        if self.estimated_tokens() > max_tokens {
            let mut note_ids: Vec<(u32, Confidence)> = self
                .entries
                .iter()
                .filter(|e| e.kind == EntryKind::Note && e.status == EntryStatus::Active)
                .map(|e| (e.id, e.confidence))
                .collect();
            note_ids.sort_by_key(|(_, c)| c.compaction_priority());
            for (id, _) in note_ids {
                if self.estimated_tokens() <= max_tokens {
                    break;
                }
                self.entries.retain(|e| e.id != id);
            }
        }
    }
}

fn kind_label(kind: EntryKind) -> &'static str {
    match kind {
        EntryKind::Hypothesis => "Hypothesis",
        EntryKind::Observation => "Observation",
        EntryKind::Conclusion => "Conclusion",
        EntryKind::Note => "Note",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn add_entry(sp: &mut Scratchpad, kind: EntryKind, label: &str, content: &str) -> u32 {
        sp.add(AddParams {
            kind,
            label: label.to_string(),
            content: content.to_string(),
            confidence: Confidence::Medium,
            parent_id: None,
            iteration: 0,
        })
        .expect("add should succeed")
        .id
    }

    #[test]
    fn add_returns_monotonic_ids() {
        let mut sp = Scratchpad::new();
        assert_eq!(add_entry(&mut sp, EntryKind::Note, "a", "first"), 0);
        assert_eq!(add_entry(&mut sp, EntryKind::Note, "b", "second"), 1);
        assert_eq!(add_entry(&mut sp, EntryKind::Note, "c", "third"), 2);
    }

    #[test]
    fn add_with_invalid_parent_falls_back_to_top_level() {
        let mut sp = Scratchpad::new();
        let result = sp
            .add(AddParams {
                kind: EntryKind::Note,
                label: "c".into(),
                content: "o".into(),
                confidence: Confidence::Medium,
                parent_id: Some(99),
                iteration: 0,
            })
            .expect("should succeed with fallback");
        assert_eq!(result.parent_dropped, Some(99));
        let entry = sp.get(result.id).expect("entry should exist");
        assert_eq!(entry.parent_id, None, "should be top-level after fallback");
    }

    #[test]
    fn add_with_valid_parent() {
        let mut sp = Scratchpad::new();
        let p = add_entry(&mut sp, EntryKind::Hypothesis, "p", "content");
        let result = sp
            .add(AddParams {
                kind: EntryKind::Observation,
                label: "c".into(),
                content: "s".into(),
                confidence: Confidence::High,
                parent_id: Some(p),
                iteration: 1,
            })
            .expect("ok");
        assert_eq!(result.parent_dropped, None);
        assert_eq!(sp.get(result.id).expect("exists").parent_id, Some(p));
    }

    #[test]
    fn add_rejects_empty_label() {
        let mut sp = Scratchpad::new();
        assert_eq!(
            sp.add(AddParams {
                kind: EntryKind::Note,
                label: "  ".into(),
                content: "c".into(),
                confidence: Confidence::Low,
                parent_id: None,
                iteration: 0,
            }),
            Err(ScratchpadError::EmptyLabel)
        );
    }

    #[test]
    fn add_rejects_empty_content() {
        let mut sp = Scratchpad::new();
        assert_eq!(
            sp.add(AddParams {
                kind: EntryKind::Note,
                label: "l".into(),
                content: "".into(),
                confidence: Confidence::Low,
                parent_id: None,
                iteration: 0,
            }),
            Err(ScratchpadError::EmptyContent)
        );
    }

    #[test]
    fn update_modifies_content_and_iteration() {
        let mut sp = Scratchpad::new();
        let id = add_entry(&mut sp, EntryKind::Hypothesis, "h1", "initial");
        sp.update(id, Some("updated".into()), None, None, 5)
            .expect("ok");
        let e = sp.get(id).expect("exists");
        assert_eq!(e.content, "updated");
        assert_eq!(e.updated_at_iteration, 5);
        assert_eq!(e.created_at_iteration, 0);
    }

    #[test]
    fn update_nonexistent() {
        let mut sp = Scratchpad::new();
        assert_eq!(
            sp.update(42, Some("x".into()), None, None, 0),
            Err(ScratchpadError::EntryNotFound(42))
        );
    }

    #[test]
    fn update_changes_status_and_confidence() {
        let mut sp = Scratchpad::new();
        let id = add_entry(&mut sp, EntryKind::Hypothesis, "h", "test");
        sp.update(
            id,
            None,
            Some(Confidence::High),
            Some(EntryStatus::Superseded),
            3,
        )
        .expect("ok");
        let e = sp.get(id).expect("exists");
        assert_eq!(e.status, EntryStatus::Superseded);
        assert_eq!(e.confidence, Confidence::High);
    }

    #[test]
    fn remove_returns_entry() {
        let mut sp = Scratchpad::new();
        let id = add_entry(&mut sp, EntryKind::Note, "n", "rm");
        let r = sp.remove(id).expect("ok");
        assert_eq!(r.label, "n");
        assert!(sp.get(id).is_none());
        assert_eq!(sp.len(), 0);
    }

    #[test]
    fn remove_nonexistent() {
        let mut sp = Scratchpad::new();
        assert_eq!(sp.remove(99), Err(ScratchpadError::EntryNotFound(99)));
    }

    #[test]
    fn active_entries_excludes_invalidated() {
        let mut sp = Scratchpad::new();
        let id0 = add_entry(&mut sp, EntryKind::Note, "a", "active");
        let id1 = add_entry(&mut sp, EntryKind::Note, "b", "inv");
        sp.update(id1, None, None, Some(EntryStatus::Invalidated), 1)
            .expect("ok");
        let active = sp.active_entries();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].id, id0);
    }

    #[test]
    fn active_entries_includes_superseded() {
        let mut sp = Scratchpad::new();
        let id = add_entry(&mut sp, EntryKind::Note, "s", "sup");
        sp.update(id, None, None, Some(EntryStatus::Superseded), 1)
            .expect("ok");
        assert_eq!(sp.active_entries().len(), 1);
    }

    #[test]
    fn entries_by_kind() {
        let mut sp = Scratchpad::new();
        add_entry(&mut sp, EntryKind::Hypothesis, "h1", "hypo");
        add_entry(&mut sp, EntryKind::Observation, "o1", "obs");
        add_entry(&mut sp, EntryKind::Hypothesis, "h2", "hypo2");
        assert_eq!(sp.entries_by_kind(EntryKind::Hypothesis).len(), 2);
    }

    #[test]
    fn render_empty() {
        let sp = Scratchpad::new();
        let r = sp.render_for_context();
        assert!(r.contains("empty"));
        assert!(r.contains("scratchpad_add"));
    }

    #[test]
    fn render_formats() {
        let mut sp = Scratchpad::new();
        add_entry(
            &mut sp,
            EntryKind::Hypothesis,
            "test hypo",
            "the API is broken",
        );
        add_entry(&mut sp, EntryKind::Observation, "obs1", "error 500");
        let r = sp.render_for_context();
        assert!(r.contains("Hypothesis"));
        assert!(r.contains("test hypo"));
        assert!(r.contains("the API is broken"));
        assert!(r.contains("obs1"));
    }

    #[test]
    fn render_shows_parent() {
        let mut sp = Scratchpad::new();
        let p = add_entry(&mut sp, EntryKind::Hypothesis, "parent", "root");
        sp.add(AddParams {
            kind: EntryKind::Observation,
            label: "child".into(),
            content: "sup".into(),
            confidence: Confidence::High,
            parent_id: Some(p),
            iteration: 1,
        })
        .expect("ok");
        assert!(sp.render_for_context().contains("parent: #0"));
    }

    #[test]
    fn estimated_tokens_scales() {
        let mut sp = Scratchpad::new();
        let t0 = sp.estimated_tokens();
        add_entry(&mut sp, EntryKind::Note, "n1", "some content here");
        let t1 = sp.estimated_tokens();
        add_entry(
            &mut sp,
            EntryKind::Note,
            "n2",
            "much longer content with many words",
        );
        assert!(t1 > t0);
        assert!(sp.estimated_tokens() > t1);
    }

    #[test]
    fn compact_drops_invalidated() {
        let mut sp = Scratchpad::new();
        add_entry(&mut sp, EntryKind::Note, "keep", "active note");
        let inv = add_entry(&mut sp, EntryKind::Note, "drop", "invalidated note");
        sp.update(inv, None, None, Some(EntryStatus::Invalidated), 1)
            .expect("ok");
        assert_eq!(sp.len(), 2);
        // Budget large enough for 1 entry but not 2 with the invalidated one.
        let tokens_before = sp.estimated_tokens();
        sp.compact(tokens_before - 1, 10, 5);
        // Invalidated should be removed first.
        assert!(sp.get(inv).is_none());
    }

    #[test]
    fn compact_drops_old_superseded() {
        let mut sp = Scratchpad::new();
        add_entry(&mut sp, EntryKind::Note, "keep", "active");
        let old = add_entry(&mut sp, EntryKind::Note, "old", "old superseded");
        sp.update(old, None, None, Some(EntryStatus::Superseded), 2)
            .expect("ok");
        let tokens_before = sp.estimated_tokens();
        sp.compact(tokens_before - 1, 10, 5);
        assert!(sp.get(old).is_none());
    }

    #[test]
    fn compact_drops_low_confidence_notes() {
        let mut sp = Scratchpad::new();
        add_entry(&mut sp, EntryKind::Hypothesis, "hypo", "important");
        sp.add(AddParams {
            kind: EntryKind::Note,
            label: "low".into(),
            content: "low confidence note with lots of words to be big".into(),
            confidence: Confidence::Low,
            parent_id: None,
            iteration: 0,
        })
        .expect("ok");
        sp.add(AddParams {
            kind: EntryKind::Note,
            label: "high".into(),
            content: "high confidence".into(),
            confidence: Confidence::High,
            parent_id: None,
            iteration: 0,
        })
        .expect("ok");
        let initial = sp.len();
        let tokens = sp.estimated_tokens();
        sp.compact(tokens - 1, 10, 5);
        assert!(sp.len() < initial);
        assert_eq!(sp.entries_by_kind(EntryKind::Hypothesis).len(), 1);
    }

    #[test]
    fn len_and_is_empty() {
        let mut sp = Scratchpad::new();
        assert!(sp.is_empty());
        assert_eq!(sp.len(), 0);
        add_entry(&mut sp, EntryKind::Note, "n", "c");
        assert!(!sp.is_empty());
        assert_eq!(sp.len(), 1);
    }

    #[test]
    fn entry_kind_roundtrip() {
        for kind in [
            EntryKind::Hypothesis,
            EntryKind::Observation,
            EntryKind::Conclusion,
            EntryKind::Note,
        ] {
            assert_eq!(EntryKind::from_str_loose(&kind.to_string()), Some(kind));
        }
        assert_eq!(
            EntryKind::from_str_loose("HYPOTHESIS"),
            Some(EntryKind::Hypothesis)
        );
        assert_eq!(EntryKind::from_str_loose("invalid"), None);
    }

    #[test]
    fn entry_status_roundtrip() {
        for s in [
            EntryStatus::Active,
            EntryStatus::Superseded,
            EntryStatus::Invalidated,
        ] {
            assert_eq!(EntryStatus::from_str_loose(&s.to_string()), Some(s));
        }
        assert_eq!(EntryStatus::from_str_loose("bogus"), None);
    }

    #[test]
    fn confidence_roundtrip() {
        for c in [Confidence::High, Confidence::Medium, Confidence::Low] {
            assert_eq!(Confidence::from_str_loose(&c.to_string()), Some(c));
        }
        assert_eq!(Confidence::from_str_loose("med"), Some(Confidence::Medium));
        assert_eq!(Confidence::from_str_loose("nope"), None);
    }

    #[test]
    fn render_excludes_invalidated() {
        let mut sp = Scratchpad::new();
        let id = add_entry(&mut sp, EntryKind::Note, "gone", "invalidated");
        sp.update(id, None, None, Some(EntryStatus::Invalidated), 1)
            .expect("ok");
        add_entry(&mut sp, EntryKind::Note, "visible", "active");
        let r = sp.render_for_context();
        assert!(!r.contains("gone"));
        assert!(r.contains("visible"));
    }

    // ── Regression tests for review findings ──────────────────────────

    #[test]
    fn kind_label_hypothesis_spelled_correctly() {
        let mut sp = Scratchpad::new();
        add_entry(&mut sp, EntryKind::Hypothesis, "h1", "body");
        let rendered = sp.render_for_context();
        assert!(
            rendered.contains("Hypothesis"),
            "expected 'Hypothesis', got: {rendered}"
        );
        assert!(
            !rendered.contains("Hypothese"),
            "old typo 'Hypothese' must not appear"
        );
    }

    #[test]
    fn remove_reparents_orphaned_children() {
        let mut sp = Scratchpad::new();
        let parent = add_entry(&mut sp, EntryKind::Hypothesis, "parent", "p");
        let child = sp
            .add(AddParams {
                kind: EntryKind::Observation,
                label: "child".to_string(),
                content: "c".to_string(),
                confidence: Confidence::Medium,
                parent_id: Some(parent),
                iteration: 0,
            })
            .expect("add child")
            .id;
        sp.remove(parent).expect("remove parent");
        let child_entry = sp.get(child).expect("child should still exist");
        assert_eq!(
            child_entry.parent_id, None,
            "orphaned child should be re-parented to None"
        );
    }

    #[test]
    fn compact_threshold_constants_are_sane() {
        const {
            assert!(SCRATCHPAD_COMPACT_THRESHOLD_CHARS > 0);
            assert!(SCRATCHPAD_COMPACT_TARGET_TOKENS > 0);
            assert!(SCRATCHPAD_COMPACT_TARGET_TOKENS < SCRATCHPAD_COMPACT_THRESHOLD_CHARS);
        }
    }

    #[test]
    fn compact_reduces_large_scratchpad() {
        let mut sp = Scratchpad::new();
        // Fill the scratchpad beyond the compact threshold.
        for i in 0..100 {
            sp.add(AddParams {
                kind: EntryKind::Note,
                label: format!("note-{i}"),
                content: "x".repeat(60),
                confidence: Confidence::Low,
                parent_id: None,
                iteration: 0,
            })
            .expect("add");
        }
        let before = sp.render_for_context().len();
        assert!(
            before > SCRATCHPAD_COMPACT_THRESHOLD_CHARS,
            "precondition: scratchpad should exceed threshold"
        );
        sp.compact(
            SCRATCHPAD_COMPACT_TARGET_TOKENS,
            10,
            SCRATCHPAD_AGE_THRESHOLD,
        );
        let after = sp.render_for_context().len();
        assert!(
            after < before,
            "compaction should reduce scratchpad size (before={before}, after={after})"
        );
    }
}
