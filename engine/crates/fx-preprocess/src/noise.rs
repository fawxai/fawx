use regex::Regex;
use std::sync::OnceLock;

/// Strip noise from text:
/// - ANSI escape codes
/// - Common log timestamp prefixes
/// - Log level prefixes ([INFO], [DEBUG], etc.)
/// - Cargo/rustc build noise lines
pub(crate) fn strip_noise(text: &str) -> String {
    let mut result = String::with_capacity(text.len());

    for (i, line) in text.lines().enumerate() {
        // Remove cargo/rustc noise lines entirely
        if is_cargo_noise(line) {
            continue;
        }

        if i > 0 && !result.is_empty() {
            result.push('\n');
        }

        let cleaned = strip_ansi(line);
        let cleaned = strip_timestamp(&cleaned);
        let cleaned = strip_log_prefix(&cleaned);
        result.push_str(&cleaned);
    }

    // Preserve trailing newline if input had one
    if text.ends_with('\n') && !result.is_empty() {
        result.push('\n');
    }

    result
}

/// Remove ANSI escape sequences.
fn strip_ansi(text: &str) -> String {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"\x1b\[[0-9;]*[a-zA-Z]").expect("valid ANSI regex"));
    re.replace_all(text, "").into_owned()
}

/// Remove common log timestamp prefixes (ISO 8601 variants).
fn strip_timestamp(text: &str) -> String {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(r"^\d{4}-\d{2}-\d{2}[T ]\d{2}:\d{2}:\d{2}(\.\d+)?(Z|[+-]\d{2}:?\d{2})?\s*")
            .expect("valid timestamp regex")
    });
    re.replace(text, "").into_owned()
}

/// Remove common log level prefixes.
fn strip_log_prefix(text: &str) -> String {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(r"^\[(INFO|DEBUG|WARN|WARNING|ERROR|TRACE)\]\s*")
            .expect("valid log prefix regex")
    });
    re.replace(text, "").into_owned()
}

/// Check if a line is cargo/rustc build noise that should be removed entirely.
fn is_cargo_noise(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with("Compiling ")
        || trimmed.starts_with("Downloading ")
        || trimmed.starts_with("Downloaded ")
        || trimmed.starts_with("Finished ")
        || trimmed.starts_with("Fresh ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ansi_codes_removed() {
        let input = "\x1b[31mred text\x1b[0m normal";
        assert_eq!(strip_noise(input), "red text normal");
    }

    #[test]
    fn log_timestamps_removed() {
        let input = "2026-03-07T01:23:45Z something happened";
        assert_eq!(strip_noise(input), "something happened");
    }

    #[test]
    fn cargo_lines_removed() {
        let input = "   Compiling serde v1.0\n   Downloading regex\nactual output";
        assert_eq!(strip_noise(input), "actual output");
    }

    #[test]
    fn mixed_noise_preserves_content() {
        let input = "\x1b[32m2026-01-15T10:30:00Z [INFO] Server started\x1b[0m\nUser logged in";
        let result = strip_noise(input);
        assert_eq!(result, "Server started\nUser logged in");
    }

    #[test]
    fn log_prefix_stripped() {
        assert_eq!(strip_noise("[DEBUG] checking value"), "checking value");
        assert_eq!(strip_noise("[ERROR] bad thing"), "bad thing");
    }
}
