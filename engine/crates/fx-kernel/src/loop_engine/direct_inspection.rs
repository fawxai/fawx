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
const NONLOCAL_SCOPE_WORDS: &[&str] =
    &["browse", "internet", "latest", "online", "research", "web"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DirectInspectionProfile {
    ReadLocalPath,
}

pub(super) fn detect_direct_inspection_profile(
    user_message: &str,
) -> Option<DirectInspectionProfile> {
    let words = message_words(user_message);
    if !contains_any_word(&words, INSPECTION_ACTION_WORDS) {
        return None;
    }
    if explicit_local_path_count(user_message) != 1 {
        return None;
    }
    if contains_any_word(&words, MUTATION_ACTION_WORDS)
        || contains_any_word(&words, NONLOCAL_SCOPE_WORDS)
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

fn contains_any_word(words: &HashSet<String>, candidates: &[&str]) -> bool {
    candidates
        .iter()
        .any(|candidate| words.contains(*candidate))
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
