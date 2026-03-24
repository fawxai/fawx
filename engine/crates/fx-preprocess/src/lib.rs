//! Deterministic prompt preprocessing for token reduction.
//!
//! Provides zero-cost, algorithmic transforms that reduce prompt token usage
//! by 15-30% on tool-heavy conversations. No LLM required.

pub mod dedup;
mod json_minify;
mod noise;
mod whitespace;

/// Configuration for preprocessing transforms.
#[derive(Clone, Debug, PartialEq)]
pub struct PreprocessConfig {
    /// Minify JSON blocks found in text (default: true).
    pub minify_json: bool,
    /// Collapse excessive whitespace (default: true).
    pub collapse_whitespace: bool,
    /// Strip ANSI codes, timestamps, and build noise (default: true).
    pub strip_noise: bool,
}

impl Default for PreprocessConfig {
    fn default() -> Self {
        Self {
            minify_json: true,
            collapse_whitespace: true,
            strip_noise: true,
        }
    }
}

/// Apply all enabled transforms to the input text.
///
/// Transforms are applied in order: noise stripping → JSON minification →
/// whitespace collapsing. This ordering ensures noise is removed before
/// JSON detection, and whitespace is collapsed last for maximum reduction.
///
/// Never fails — returns input unchanged on error.
///
/// # Note on JSON minification
///
/// When `minify_json` is enabled, JSON blocks are parsed and re-serialized
/// via `serde_json::Value`. Because `serde_json` uses `BTreeMap` for JSON
/// objects (unless the `preserve_order` feature is enabled), keys in the
/// output will be sorted alphabetically. This is semantically equivalent
/// and fine for LLM consumption, but the key order may differ from the input.
#[must_use]
pub fn preprocess(text: &str, config: &PreprocessConfig) -> String {
    if text.is_empty() {
        return String::new();
    }

    let mut result = text.to_owned();

    if config.strip_noise {
        result = noise::strip_noise(&result);
    }
    if config.minify_json {
        result = json_minify::minify_json_blocks(&result);
    }
    if config.collapse_whitespace {
        result = whitespace::collapse_whitespace(&result);
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_transforms_together() {
        let input = concat!(
            "\x1b[32m2026-03-07T01:23:45Z [INFO] Result:\x1b[0m\n",
            "{\n  \"status\": \"ok\",\n  \"count\": 42\n}\n",
            "\n\n\n",
            "Done.   "
        );
        let result = preprocess(input, &PreprocessConfig::default());
        assert_eq!(result, "Result:\n{\"count\":42,\"status\":\"ok\"}\n\nDone.");
    }

    #[test]
    fn config_disables_json() {
        let config = PreprocessConfig {
            minify_json: false,
            ..Default::default()
        };
        let input = "{\n  \"a\": 1\n}";
        let result = preprocess(input, &config);
        // JSON should NOT be minified
        assert!(result.contains("\"a\": 1"));
    }

    #[test]
    fn config_disables_whitespace() {
        let config = PreprocessConfig {
            collapse_whitespace: false,
            ..Default::default()
        };
        let input = "hello   \n\n\n\nworld";
        let result = preprocess(input, &config);
        assert!(result.contains("\n\n\n\n"));
    }

    #[test]
    fn config_disables_noise() {
        let config = PreprocessConfig {
            strip_noise: false,
            ..Default::default()
        };
        let input = "\x1b[31mred\x1b[0m";
        let result = preprocess(input, &config);
        assert!(result.contains("\x1b[31m"));
    }

    #[test]
    fn empty_input() {
        assert_eq!(preprocess("", &PreprocessConfig::default()), "");
    }

    #[test]
    fn idempotent() {
        let input = concat!(
            "2026-01-01T00:00:00Z [DEBUG] test\n",
            "{\n  \"key\": \"value\"\n}\n",
            "\n\n\n",
            "trailing   \t  "
        );
        let config = PreprocessConfig::default();
        let first = preprocess(input, &config);
        let second = preprocess(&first, &config);
        assert_eq!(first, second, "preprocess must be idempotent");
    }

    #[test]
    fn large_input_not_pathological() {
        let chunk = "{\n  \"key\": \"value\",\n  \"number\": 42\n}\n\n";
        let input = chunk.repeat(1000);
        let start = std::time::Instant::now();
        let _ = preprocess(&input, &PreprocessConfig::default());
        let elapsed = start.elapsed();
        assert!(
            elapsed.as_secs() < 5,
            "processing 1000 JSON blocks took {elapsed:?}, expected < 5s"
        );
    }
}
