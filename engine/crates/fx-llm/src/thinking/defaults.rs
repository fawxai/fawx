use super::matching::ModelMapping;
use super::profile::{ApiStyle, LevelParams, ThinkingProfile};
use std::collections::BTreeMap;

pub fn default_profiles() -> BTreeMap<String, ThinkingProfile> {
    let mut profiles = BTreeMap::new();
    profiles.insert("anthropic-adaptive".to_owned(), anthropic_adaptive());
    profiles.insert(
        "anthropic-adaptive-no-max".to_owned(),
        anthropic_adaptive_no_max(),
    );
    profiles.insert("anthropic-legacy".to_owned(), anthropic_legacy());
    profiles.insert("openai-reasoning".to_owned(), openai_reasoning());
    profiles.insert("none".to_owned(), none_profile());
    profiles
}

pub fn default_model_mappings() -> Vec<ModelMapping> {
    vec![
        // Exact matches (highest priority)
        ModelMapping::exact("claude-opus-4-6", "anthropic-adaptive"),
        ModelMapping::exact("claude-sonnet-4-6", "anthropic-adaptive-no-max"),
        // Prefix globs
        ModelMapping::prefix("claude-opus-4-5", "anthropic-legacy"),
        ModelMapping::prefix("claude-sonnet-4-5", "anthropic-legacy"),
        ModelMapping::prefix("claude-haiku-4-5", "anthropic-legacy"),
        ModelMapping::prefix("gpt-5", "openai-reasoning"),
        ModelMapping::prefix("codex-", "openai-reasoning"),
        ModelMapping::prefix("o1", "openai-reasoning"),
        ModelMapping::prefix("o3", "openai-reasoning"),
        // Wildcard fallbacks
        ModelMapping::wildcard("claude-", "anthropic-adaptive-no-max"),
        ModelMapping::wildcard("gpt-", "openai-reasoning"),
    ]
}

fn anthropic_adaptive() -> ThinkingProfile {
    ThinkingProfile {
        levels: vec![
            "off".to_owned(),
            "low".to_owned(),
            "medium".to_owned(),
            "high".to_owned(),
            "max".to_owned(),
        ],
        default: "high".to_owned(),
        api_style: ApiStyle::AdaptiveEffort,
        level_params: BTreeMap::new(),
    }
}

fn anthropic_adaptive_no_max() -> ThinkingProfile {
    ThinkingProfile {
        levels: vec![
            "off".to_owned(),
            "low".to_owned(),
            "medium".to_owned(),
            "high".to_owned(),
        ],
        default: "high".to_owned(),
        api_style: ApiStyle::AdaptiveEffort,
        level_params: BTreeMap::new(),
    }
}

fn anthropic_legacy() -> ThinkingProfile {
    let mut level_params = BTreeMap::new();
    level_params.insert(
        "low".to_owned(),
        LevelParams {
            budget_tokens: Some(1_024),
            ..Default::default()
        },
    );
    level_params.insert(
        "high".to_owned(),
        LevelParams {
            budget_tokens: Some(10_000),
            ..Default::default()
        },
    );
    ThinkingProfile {
        levels: vec!["off".to_owned(), "low".to_owned(), "high".to_owned()],
        default: "high".to_owned(),
        api_style: ApiStyle::BudgetTokens,
        level_params,
    }
}

fn openai_reasoning() -> ThinkingProfile {
    ThinkingProfile {
        levels: vec![
            "off".to_owned(),
            "low".to_owned(),
            "medium".to_owned(),
            "high".to_owned(),
            "xhigh".to_owned(),
        ],
        default: "medium".to_owned(),
        api_style: ApiStyle::ReasoningEffort,
        level_params: BTreeMap::new(),
    }
}

fn none_profile() -> ThinkingProfile {
    ThinkingProfile {
        levels: vec!["off".to_owned()],
        default: "off".to_owned(),
        api_style: ApiStyle::Disabled,
        level_params: BTreeMap::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_default_profiles_have_off_level() {
        for (name, profile) in default_profiles() {
            assert!(
                profile.levels.contains(&"off".to_owned()),
                "profile {name} missing 'off' level"
            );
        }
    }

    #[test]
    fn all_default_profiles_have_valid_default() {
        for (name, profile) in default_profiles() {
            assert!(
                profile.levels.contains(&profile.default),
                "profile {name} default '{}' not in levels",
                profile.default
            );
        }
    }

    #[test]
    fn none_profile_only_has_off() {
        let profiles = default_profiles();
        let none = profiles.get("none").unwrap();
        assert_eq!(none.levels, vec!["off"]);
        assert_eq!(none.default, "off");
    }

    #[test]
    fn anthropic_adaptive_has_max() {
        let profiles = default_profiles();
        let adaptive = profiles.get("anthropic-adaptive").unwrap();
        assert!(adaptive.levels.contains(&"max".to_owned()));
    }

    #[test]
    fn anthropic_adaptive_no_max_excludes_max() {
        let profiles = default_profiles();
        let no_max = profiles.get("anthropic-adaptive-no-max").unwrap();
        assert!(!no_max.levels.contains(&"max".to_owned()));
    }

    #[test]
    fn default_mappings_cover_common_models() {
        let mappings = default_model_mappings();
        assert!(mappings.iter().any(|m| m.matches("claude-opus-4-6")));
        assert!(mappings.iter().any(|m| m.matches("claude-sonnet-4-6")));
        assert!(mappings.iter().any(|m| m.matches("gpt-5.4")));
    }
}
