//! Utility functions for policy evaluation.

/// Check if an action matches a pattern with glob-style wildcards.
///
/// Supports:
/// - `*` - Matches zero or more characters
/// - `?` - Matches exactly one character
/// - Exact matches (no wildcards)
///
/// # Examples
///
/// ```
/// use fx_security::policy::util::matches_action;
///
/// // Exact match
/// assert!(matches_action("launch_app", "launch_app"));
/// assert!(!matches_action("launch_app", "launch_app2"));
///
/// // Wildcard *
/// assert!(matches_action("delete_*", "delete_file"));
/// assert!(matches_action("delete_*", "delete_contact"));
/// assert!(matches_action("*", "anything"));
/// assert!(matches_action("*.txt", "notes.txt"));
/// assert!(matches_action("send_*_message", "send_sms_message"));
///
/// // Wildcard ?
/// assert!(matches_action("app_?", "app_a"));
/// assert!(!matches_action("app_?", "app_ab"));
/// assert!(matches_action("test_??", "test_ab"));
/// ```
pub fn matches_action(pattern: &str, action: &str) -> bool {
    matches_glob(pattern, action)
}

/// Internal glob matcher supporting * and ?.
///
/// # Performance Note
/// This implementation uses recursion and creates new strings on each recursive call.
/// For patterns with multiple `*` wildcards, this could be less efficient than
/// specialized glob libraries. However, for typical policy patterns (e.g., "delete_*",
/// "*.txt"), the performance is acceptable.
fn matches_glob(pattern: &str, text: &str) -> bool {
    let mut pattern_chars = pattern.chars().peekable();
    let mut text_chars = text.chars().peekable();

    loop {
        match pattern_chars.peek() {
            Some('*') => {
                pattern_chars.next();

                // * at end matches everything remaining
                if pattern_chars.peek().is_none() {
                    return true;
                }

                // Try to match the rest of the pattern with different positions in text
                while text_chars.peek().is_some() {
                    if matches_glob(
                        pattern_chars.clone().collect::<String>().as_str(),
                        text_chars.clone().collect::<String>().as_str(),
                    ) {
                        return true;
                    }
                    text_chars.next();
                }

                // Check if empty text matches remaining pattern
                return matches_glob(pattern_chars.clone().collect::<String>().as_str(), "");
            }
            Some('?') => {
                pattern_chars.next();
                if text_chars.next().is_none() {
                    return false;
                }
            }
            Some(&p) => {
                pattern_chars.next();
                match text_chars.next() {
                    Some(t) if t == p => continue,
                    _ => return false,
                }
            }
            None => {
                return text_chars.peek().is_none();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exact_match() {
        assert!(matches_action("launch_app", "launch_app"));
        assert!(!matches_action("launch_app", "launch_app2"));
        assert!(!matches_action("launch_app", "launch"));
    }

    #[test]
    fn test_wildcard_star_end() {
        assert!(matches_action("delete_*", "delete_file"));
        assert!(matches_action("delete_*", "delete_contact"));
        assert!(matches_action("delete_*", "delete_"));
        assert!(!matches_action("delete_*", "send_message"));
    }

    #[test]
    fn test_wildcard_star_start() {
        assert!(matches_action("*.txt", "notes.txt"));
        assert!(matches_action("*.txt", "readme.txt"));
        assert!(matches_action("*.txt", ".txt"));
        assert!(!matches_action("*.txt", "notes.md"));
    }

    #[test]
    fn test_wildcard_star_middle() {
        assert!(matches_action("send_*_message", "send_sms_message"));
        assert!(matches_action("send_*_message", "send_email_message"));
        assert!(matches_action("send_*_message", "send__message"));
        assert!(!matches_action("send_*_message", "send_sms"));
    }

    #[test]
    fn test_wildcard_star_all() {
        assert!(matches_action("*", "anything"));
        assert!(matches_action("*", ""));
        assert!(matches_action("*", "launch_app"));
    }

    #[test]
    fn test_wildcard_question() {
        assert!(matches_action("app_?", "app_a"));
        assert!(matches_action("app_?", "app_1"));
        assert!(!matches_action("app_?", "app_ab"));
        assert!(!matches_action("app_?", "app_"));
    }

    #[test]
    fn test_wildcard_multiple_question() {
        assert!(matches_action("test_??", "test_ab"));
        assert!(matches_action("test_??", "test_12"));
        assert!(!matches_action("test_??", "test_a"));
        assert!(!matches_action("test_??", "test_abc"));
    }

    #[test]
    fn test_mixed_wildcards() {
        assert!(matches_action("app_?_*", "app_a_launch"));
        assert!(matches_action("app_?_*", "app_1_delete_file"));
        assert!(!matches_action("app_?_*", "app__launch"));
    }

    #[test]
    fn test_empty_pattern() {
        assert!(matches_action("", ""));
        assert!(!matches_action("", "something"));
    }

    #[test]
    fn test_empty_action() {
        assert!(matches_action("", ""));
        assert!(matches_action("*", ""));
        assert!(!matches_action("?", ""));
        assert!(!matches_action("app", ""));
    }

    #[test]
    fn test_special_chars() {
        assert!(matches_action("test-action", "test-action"));
        assert!(matches_action("test_action", "test_action"));
        assert!(matches_action("test.action", "test.action"));
    }
}
