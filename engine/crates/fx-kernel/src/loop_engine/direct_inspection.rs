use super::{DIRECT_INSPECTION_READ_LOCAL_PATH_PHASE_DIRECTIVE, DIRECT_INSPECTION_TASK_DIRECTIVE};
use std::collections::HashSet;

const INSPECTION_ACTION_WORDS: &[&str] = &["inspect", "quote", "read", "summarize", "summarise"];
const MUTATION_ACTION_WORDS: &[&str] = &[
    "add",
    "change",
    "create",
    "debug",
    "delete",
    "diagnose",
    "edit",
    "execute",
    "fix",
    "implement",
    "modify",
    "mutate",
    "remove",
    "rewrite",
    "run",
    "test",
    "update",
    "write",
];
const EXTERNAL_CONTEXT_WORDS: &[&str] = &[
    "against",
    "browse",
    "compare",
    "comparison",
    "guidance",
    "internet",
    "latest",
    "online",
    "research",
    "web",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DirectInspectionProfile {
    ReadLocalPath,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum DirectInspectionOwnership {
    #[default]
    DetectFromTurn,
    PreserveParent(Option<DirectInspectionProfile>),
}

#[derive(Debug)]
struct InspectionRequestAnalysis {
    explicit_local_path_count: usize,
    words: HashSet<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InspectionSatisfiability {
    LocalObservationOnly,
    RequiresExternalContext,
    RequiresMutation,
}

pub(super) fn detect_direct_inspection_profile(
    user_message: &str,
) -> Option<DirectInspectionProfile> {
    let analysis = InspectionRequestAnalysis::from_user_message(user_message);
    if !analysis.requests_read_local_path()
        || analysis.satisfiability() != InspectionSatisfiability::LocalObservationOnly
    {
        return None;
    }
    Some(DirectInspectionProfile::ReadLocalPath)
}

pub(super) fn direct_inspection_profile_label(profile: DirectInspectionProfile) -> &'static str {
    match profile {
        DirectInspectionProfile::ReadLocalPath => "read_local_path",
    }
}

pub(super) fn direct_inspection_tool_names(
    profile: &DirectInspectionProfile,
) -> &'static [&'static str] {
    match profile {
        DirectInspectionProfile::ReadLocalPath => &["read_file"],
    }
}

pub(super) fn direct_inspection_directive(profile: &DirectInspectionProfile) -> String {
    match profile {
        DirectInspectionProfile::ReadLocalPath => format!(
            "{DIRECT_INSPECTION_TASK_DIRECTIVE}{DIRECT_INSPECTION_READ_LOCAL_PATH_PHASE_DIRECTIVE}"
        ),
    }
}

pub(super) fn direct_inspection_block_reason(profile: &DirectInspectionProfile) -> &'static str {
    match profile {
        DirectInspectionProfile::ReadLocalPath => {
            "direct inspection only allows observation tools for the requested local path"
        }
    }
}

impl DirectInspectionOwnership {
    pub(super) fn profile_for_turn(self, user_message: &str) -> Option<DirectInspectionProfile> {
        match self {
            Self::DetectFromTurn => detect_direct_inspection_profile(user_message),
            Self::PreserveParent(profile) => profile,
        }
    }
}

fn contains_any_word(words: &HashSet<String>, candidates: &[&str]) -> bool {
    candidates
        .iter()
        .any(|candidate| words.contains(*candidate))
}

impl InspectionRequestAnalysis {
    fn from_user_message(user_message: &str) -> Self {
        Self {
            explicit_local_path_count: explicit_local_path_count(user_message),
            words: message_words(user_message),
        }
    }

    fn requests_read_local_path(&self) -> bool {
        self.explicit_local_path_count == 1
            && contains_any_word(&self.words, INSPECTION_ACTION_WORDS)
    }

    fn satisfiability(&self) -> InspectionSatisfiability {
        if contains_any_word(&self.words, MUTATION_ACTION_WORDS) {
            return InspectionSatisfiability::RequiresMutation;
        }
        if contains_any_word(&self.words, EXTERNAL_CONTEXT_WORDS) {
            return InspectionSatisfiability::RequiresExternalContext;
        }
        InspectionSatisfiability::LocalObservationOnly
    }
}

fn explicit_local_path_count(user_message: &str) -> usize {
    user_message
        .split_whitespace()
        .filter_map(normalized_explicit_local_path_token)
        .collect::<HashSet<_>>()
        .len()
}

fn normalized_explicit_local_path_token(token: &str) -> Option<&str> {
    let normalized = trim_wrapping_punctuation(token);
    is_explicit_local_path(normalized).then_some(normalized)
}

fn is_explicit_local_path(token: &str) -> bool {
    token.starts_with('/') || token.starts_with("~/")
}

fn message_words(user_message: &str) -> HashSet<String> {
    user_message
        .split_whitespace()
        .map(trim_wrapping_punctuation)
        .filter(|token| !is_explicit_local_path(token))
        .flat_map(|token| token.split(|c: char| !c.is_ascii_alphanumeric()))
        .filter(|word| !word.is_empty())
        .map(str::to_ascii_lowercase)
        .collect()
}

fn trim_wrapping_punctuation(token: &str) -> &str {
    token.trim_matches(|c: char| {
        matches!(
            c,
            '"' | '\''
                | '`'
                | ','
                | '.'
                | ':'
                | ';'
                | '?'
                | '!'
                | '('
                | ')'
                | '['
                | ']'
                | '{'
                | '}'
        )
    })
}
