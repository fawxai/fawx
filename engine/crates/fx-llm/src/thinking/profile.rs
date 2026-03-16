use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// A thinking capability profile for a model or model family.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThinkingProfile {
    /// User-facing levels available in the picker.
    pub levels: Vec<String>,
    /// Default level when user hasn't chosen.
    pub default: String,
    /// How to translate level names into provider API parameters.
    pub api_style: ApiStyle,
    /// Optional per-level parameter overrides.
    #[serde(default)]
    pub level_params: BTreeMap<String, LevelParams>,
}

/// How to build the API request for a thinking level.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ApiStyle {
    /// Anthropic adaptive effort (effort string).
    AdaptiveEffort,
    /// Anthropic legacy (fixed budget_tokens per level).
    BudgetTokens,
    /// OpenAI reasoning_effort parameter.
    ReasoningEffort,
    /// No thinking support.
    Disabled,
}

/// Per-level parameter configuration.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LevelParams {
    /// For BudgetTokens style: token count for this level.
    pub budget_tokens: Option<u32>,
    /// For AdaptiveEffort style: effort string sent to API.
    pub effort: Option<String>,
    /// For ReasoningEffort style: effort string for OpenAI.
    pub reasoning_effort: Option<String>,
}

/// Provider-specific thinking parameters produced by translation.
#[derive(Debug, Clone, PartialEq)]
pub enum ThinkingParams {
    /// Thinking disabled.
    Disabled,
    /// Anthropic-style parameters.
    Anthropic {
        budget_tokens: Option<u32>,
        effort: Option<String>,
    },
    /// OpenAI-style parameters.
    OpenAi { reasoning_effort: String },
}

impl ThinkingProfile {
    /// Translate a user-facing level name into provider API parameters.
    pub fn translate(&self, level: &str) -> ThinkingParams {
        if level == "off" {
            return ThinkingParams::Disabled;
        }
        if let Some(params) = self.level_params.get(level) {
            return translate_with_params(&self.api_style, params);
        }
        translate_default(&self.api_style, level)
    }
}

fn translate_with_params(style: &ApiStyle, params: &LevelParams) -> ThinkingParams {
    match style {
        ApiStyle::AdaptiveEffort => ThinkingParams::Anthropic {
            budget_tokens: params.budget_tokens,
            effort: params.effort.clone(),
        },
        ApiStyle::BudgetTokens => ThinkingParams::Anthropic {
            budget_tokens: params.budget_tokens,
            effort: None,
        },
        ApiStyle::ReasoningEffort => ThinkingParams::OpenAi {
            reasoning_effort: params
                .reasoning_effort
                .clone()
                .unwrap_or_else(|| "medium".to_owned()),
        },
        ApiStyle::Disabled => ThinkingParams::Disabled,
    }
}

fn translate_default(style: &ApiStyle, level: &str) -> ThinkingParams {
    match style {
        ApiStyle::AdaptiveEffort => ThinkingParams::Anthropic {
            budget_tokens: default_budget_for_level(level),
            effort: Some(level.to_owned()),
        },
        ApiStyle::BudgetTokens => ThinkingParams::Anthropic {
            budget_tokens: default_budget_for_level(level),
            effort: None,
        },
        ApiStyle::ReasoningEffort => ThinkingParams::OpenAi {
            reasoning_effort: level.to_owned(),
        },
        ApiStyle::Disabled => ThinkingParams::Disabled,
    }
}

fn default_budget_for_level(level: &str) -> Option<u32> {
    match level {
        "low" => Some(1_024),
        "medium" => Some(4_096),
        "high" => Some(10_000),
        "max" => Some(32_000),
        "xhigh" => Some(32_000),
        _ => Some(4_096),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn adaptive_profile() -> ThinkingProfile {
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

    fn openai_profile() -> ThinkingProfile {
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

    fn budget_profile() -> ThinkingProfile {
        let mut params = BTreeMap::new();
        params.insert(
            "low".to_owned(),
            LevelParams {
                budget_tokens: Some(1_024),
                ..Default::default()
            },
        );
        params.insert(
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
            level_params: params,
        }
    }

    #[test]
    fn off_always_returns_disabled() {
        assert_eq!(
            adaptive_profile().translate("off"),
            ThinkingParams::Disabled
        );
        assert_eq!(openai_profile().translate("off"), ThinkingParams::Disabled);
        assert_eq!(budget_profile().translate("off"), ThinkingParams::Disabled);
    }

    #[test]
    fn adaptive_effort_returns_anthropic_params() {
        let params = adaptive_profile().translate("high");
        assert_eq!(
            params,
            ThinkingParams::Anthropic {
                budget_tokens: Some(10_000),
                effort: Some("high".to_owned()),
            }
        );
    }

    #[test]
    fn adaptive_max_returns_high_budget() {
        let params = adaptive_profile().translate("max");
        assert_eq!(
            params,
            ThinkingParams::Anthropic {
                budget_tokens: Some(32_000),
                effort: Some("max".to_owned()),
            }
        );
    }

    #[test]
    fn openai_returns_reasoning_effort() {
        let params = openai_profile().translate("medium");
        assert_eq!(
            params,
            ThinkingParams::OpenAi {
                reasoning_effort: "medium".to_owned(),
            }
        );
    }

    #[test]
    fn budget_tokens_uses_level_params() {
        let params = budget_profile().translate("low");
        assert_eq!(
            params,
            ThinkingParams::Anthropic {
                budget_tokens: Some(1_024),
                effort: None,
            }
        );
    }

    #[test]
    fn profile_serialization_roundtrip() {
        let profile = adaptive_profile();
        let json = serde_json::to_string(&profile).unwrap();
        let decoded: ThinkingProfile = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.levels, profile.levels);
        assert_eq!(decoded.default, profile.default);
        assert_eq!(decoded.api_style, profile.api_style);
    }
}
