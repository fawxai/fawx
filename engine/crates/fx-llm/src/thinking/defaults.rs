use crate::ThinkingConfig;

const OFF_LEVELS: &[&str] = &["off"];
const CLAUDE_OPUS_46_LEVELS: &[&str] = &["off", "adaptive", "low", "medium", "high", "max"];
const CLAUDE_SONNET_46_LEVELS: &[&str] = &["off", "adaptive", "low", "medium", "high"];
const CLAUDE_LEGACY_LEVELS: &[&str] = &["off", "low", "high"];
const GPT_54_LEVELS: &[&str] = &["none", "low", "medium", "high", "xhigh"];
const GPT_52_LEVELS: &[&str] = &["none", "low", "medium", "high"];
const GPT_5_LEVELS: &[&str] = &["minimal", "low", "medium", "high"];
const O1_O3_LEVELS: &[&str] = &["off", "low", "medium", "high"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ModelFamily {
    ClaudeOpus46,
    ClaudeSonnet46,
    /// Claude 4.5, Haiku, and all older Claude models (same behavior).
    ClaudeLegacy,
    Gpt54,
    Gpt5,
    Gpt52,
    O1O3,
    Unknown,
}

fn model_name(model_id: &str) -> &str {
    model_id.split('/').next_back().unwrap_or(model_id)
}

fn classify_model(model_id: &str) -> ModelFamily {
    let model = model_name(model_id);
    if model.contains("opus-4-6") {
        return ModelFamily::ClaudeOpus46;
    }
    if model.contains("sonnet-4-6") {
        return ModelFamily::ClaudeSonnet46;
    }
    if model.starts_with("claude-opus-4-5")
        || model.starts_with("claude-sonnet-4-5")
        || model.starts_with("claude-haiku-4-5")
    {
        return ModelFamily::ClaudeLegacy;
    }
    if model.starts_with("claude-") {
        return ModelFamily::ClaudeLegacy;
    }
    if model.starts_with("gpt-5.4") {
        return ModelFamily::Gpt54;
    }
    if model.starts_with("gpt-5.2") {
        return ModelFamily::Gpt52;
    }
    if model.starts_with("gpt-5") || model.starts_with("codex-") {
        return ModelFamily::Gpt5;
    }
    if model.starts_with("o1") || model.starts_with("o3") {
        return ModelFamily::O1O3;
    }
    ModelFamily::Unknown
}

fn anthropic_46_effort(level: &str) -> String {
    if level == "adaptive" {
        "high".to_string()
    } else {
        level.to_string()
    }
}

fn openai_reasoning_effort(level: &str) -> String {
    if level == "adaptive" {
        "medium".to_string()
    } else {
        level.to_string()
    }
}

fn legacy_claude_budget(model_id: &str, level: &str) -> u32 {
    match level {
        "low" => 1_024,
        "high" => 10_000,
        _ => {
            tracing::warn!(
                level,
                model = model_name(model_id),
                "unexpected thinking level for model, using default budget"
            );
            4_096
        }
    }
}

/// Return valid thinking levels for a model.
pub fn valid_thinking_levels(model_id: &str) -> &'static [&'static str] {
    match classify_model(model_id) {
        ModelFamily::ClaudeOpus46 => CLAUDE_OPUS_46_LEVELS,
        ModelFamily::ClaudeSonnet46 => CLAUDE_SONNET_46_LEVELS,
        ModelFamily::ClaudeLegacy => CLAUDE_LEGACY_LEVELS,
        ModelFamily::Gpt54 => GPT_54_LEVELS,
        ModelFamily::Gpt52 => GPT_52_LEVELS,
        ModelFamily::Gpt5 => GPT_5_LEVELS,
        ModelFamily::O1O3 => O1_O3_LEVELS,
        ModelFamily::Unknown => OFF_LEVELS,
    }
}

/// Return the default thinking level for a model.
pub fn default_thinking_level(model_id: &str) -> &'static str {
    match classify_model(model_id) {
        ModelFamily::ClaudeOpus46 | ModelFamily::ClaudeSonnet46 | ModelFamily::ClaudeLegacy => {
            "high"
        }
        ModelFamily::Gpt54 | ModelFamily::Gpt52 => "none",
        ModelFamily::Gpt5 => "medium",
        ModelFamily::O1O3 | ModelFamily::Unknown => "off",
    }
}

/// Build ThinkingConfig from a model ID and user-selected level.
pub fn thinking_config_for_model(model_id: &str, level: &str) -> Option<ThinkingConfig> {
    if level == "off" || level == "none" {
        return Some(ThinkingConfig::Off);
    }

    match classify_model(model_id) {
        ModelFamily::ClaudeOpus46 | ModelFamily::ClaudeSonnet46 => Some(ThinkingConfig::Adaptive {
            effort: anthropic_46_effort(level),
        }),
        ModelFamily::ClaudeLegacy => Some(ThinkingConfig::Enabled {
            budget_tokens: legacy_claude_budget(model_id, level),
        }),
        ModelFamily::Gpt54 | ModelFamily::Gpt52 | ModelFamily::Gpt5 | ModelFamily::O1O3 => {
            Some(ThinkingConfig::Reasoning {
                effort: openai_reasoning_effort(level),
            })
        }
        ModelFamily::Unknown => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn opus_4_6_includes_max() {
        let levels = valid_thinking_levels("claude-opus-4-6");
        assert!(levels.contains(&"max"));
        assert!(levels.contains(&"high"));
    }

    #[test]
    fn sonnet_4_6_excludes_max() {
        let levels = valid_thinking_levels("claude-sonnet-4-6");
        assert!(!levels.contains(&"max"));
        assert!(levels.contains(&"high"));
    }

    #[test]
    fn gpt_5_4_includes_xhigh() {
        let levels = valid_thinking_levels("gpt-5.4");
        assert!(levels.contains(&"xhigh"));
    }

    #[test]
    fn gpt_5_2_excludes_xhigh() {
        let levels = valid_thinking_levels("gpt-5.2");
        assert!(!levels.contains(&"xhigh"));
        assert!(levels.contains(&"high"));
    }

    #[test]
    fn gpt_5_includes_minimal() {
        let levels = valid_thinking_levels("gpt-5");
        assert!(levels.contains(&"minimal"));
    }

    #[test]
    fn o3_includes_off() {
        let levels = valid_thinking_levels("o3");
        assert!(levels.contains(&"off"));
        assert!(levels.contains(&"high"));
    }

    #[test]
    fn unknown_model_returns_off() {
        let levels = valid_thinking_levels("unknown-model");
        assert_eq!(levels, &["off"]);
    }

    #[test]
    fn provider_prefix_stripped() {
        let a = valid_thinking_levels("claude-opus-4-6");
        let b = valid_thinking_levels("anthropic/claude-opus-4-6");
        assert_eq!(a, b);
    }

    #[test]
    fn codex_model_classified_as_gpt5() {
        let levels = valid_thinking_levels("codex-mini-latest");
        assert_eq!(levels, GPT_5_LEVELS);
    }

    #[test]
    fn default_level_anthropic_4_6() {
        assert_eq!(default_thinking_level("claude-opus-4-6"), "high");
    }

    #[test]
    fn default_level_openai_5_4() {
        assert_eq!(default_thinking_level("gpt-5.4"), "none");
    }

    #[test]
    fn config_for_anthropic_4_6_is_adaptive() {
        let config = thinking_config_for_model("claude-opus-4-6", "high");
        assert!(matches!(
            config,
            Some(crate::ThinkingConfig::Adaptive { .. })
        ));
    }

    #[test]
    fn config_for_anthropic_4_5_is_manual() {
        let config = thinking_config_for_model("claude-sonnet-4-5-20250929", "high");
        assert!(matches!(
            config,
            Some(crate::ThinkingConfig::Enabled { .. })
        ));
    }

    #[test]
    fn config_for_openai_is_reasoning() {
        let config = thinking_config_for_model("gpt-5.4", "xhigh");
        assert!(matches!(
            config,
            Some(crate::ThinkingConfig::Reasoning { .. })
        ));
    }

    #[test]
    fn config_off_returns_off() {
        let config = thinking_config_for_model("claude-opus-4-6", "off");
        assert!(matches!(config, Some(crate::ThinkingConfig::Off)));
    }

    #[test]
    fn o1_off_returns_off() {
        let config = thinking_config_for_model("o1", "off");
        assert!(matches!(config, Some(crate::ThinkingConfig::Off)));
    }

    #[test]
    fn adaptive_alias_maps_to_medium_effort_for_openai() {
        let config = thinking_config_for_model("gpt-5.4", "adaptive");
        assert_eq!(
            config,
            Some(crate::ThinkingConfig::Reasoning {
                effort: "medium".to_string(),
            })
        );
    }

    #[test]
    fn legacy_claude_unknown_level_falls_back_to_default_budget() {
        // Unexpected level like "adaptive" on a 4.5 model gets default budget (4096)
        // and a tracing::warn is emitted (not asserted here — side effect).
        let config = thinking_config_for_model("claude-sonnet-4-5-20250929", "adaptive");
        assert_eq!(
            config,
            Some(crate::ThinkingConfig::Enabled {
                budget_tokens: 4_096,
            })
        );
    }
}
