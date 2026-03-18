use crate::ThinkingConfig;

/// Return valid thinking levels for a model.
pub fn valid_thinking_levels(model_id: &str) -> &'static [&'static str] {
    let model = model_id.split('/').next_back().unwrap_or(model_id);
    if model.contains("opus-4-6") {
        return &["off", "adaptive", "low", "medium", "high", "max"];
    }
    if model.contains("sonnet-4-6") {
        return &["off", "adaptive", "low", "medium", "high"];
    }
    if model.starts_with("claude-opus-4-5")
        || model.starts_with("claude-sonnet-4-5")
        || model.starts_with("claude-haiku-4-5")
    {
        return &["off", "low", "high"];
    }
    if model.starts_with("claude-") {
        return &["off", "low", "high"];
    }
    if model.starts_with("gpt-5.4") {
        return &["none", "low", "medium", "high", "xhigh"];
    }
    if model.starts_with("gpt-5.2") {
        return &["none", "low", "medium", "high"];
    }
    if model.starts_with("gpt-5") {
        return &["minimal", "low", "medium", "high"];
    }
    if model.starts_with("o1") || model.starts_with("o3") {
        return &["low", "medium", "high"];
    }
    &["off"]
}

/// Return the default thinking level for a model.
pub fn default_thinking_level(model_id: &str) -> &'static str {
    let model = model_id.split('/').next_back().unwrap_or(model_id);
    if model.contains("opus-4-6") || model.contains("sonnet-4-6") {
        return "high";
    }
    if model.starts_with("claude-") {
        return "high";
    }
    if model.starts_with("gpt-5.4") || model.starts_with("gpt-5.2") {
        return "none";
    }
    if model.starts_with("gpt-5") {
        return "medium";
    }
    "off"
}

/// Build ThinkingConfig from a model ID and user-selected level.
pub fn thinking_config_for_model(model_id: &str, level: &str) -> Option<ThinkingConfig> {
    let model = model_id.split('/').next_back().unwrap_or(model_id);
    if level == "off" || level == "none" {
        return Some(ThinkingConfig::Off);
    }
    if model.contains("opus-4-6") || model.contains("sonnet-4-6") {
        let effort = if level == "adaptive" {
            "high".to_string()
        } else {
            level.to_string()
        };
        return Some(ThinkingConfig::Adaptive { effort });
    }
    if model.starts_with("claude-") {
        let budget = match level {
            "low" => 1_024,
            "high" => 10_000,
            _ => 4_096,
        };
        return Some(ThinkingConfig::Enabled {
            budget_tokens: budget,
        });
    }
    if model.starts_with("gpt-")
        || model.starts_with("o1")
        || model.starts_with("o3")
        || model.starts_with("codex-")
    {
        let effort = if level == "adaptive" {
            default_thinking_level(model).to_string()
        } else {
            level.to_string()
        };
        return Some(ThinkingConfig::Reasoning { effort });
    }
    None
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
    fn adaptive_alias_maps_to_model_default_effort() {
        let config = thinking_config_for_model("gpt-5.4", "adaptive");
        assert_eq!(
            config,
            Some(crate::ThinkingConfig::Reasoning {
                effort: "none".to_string(),
            })
        );
    }
}
