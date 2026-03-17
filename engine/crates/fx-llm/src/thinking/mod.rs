//! Thinking capability registry for per-model thinking level management.
//!
//! Replaces the hardcoded `supported_thinking_levels()` function with a
//! data-driven registry that maps model IDs to thinking profiles.

mod defaults;
mod matching;
mod profile;
mod rejection;

use matching::{find_best_match, ModelMapping};
use rejection::RejectionCache;
use std::collections::BTreeMap;

pub use profile::{ApiStyle, LevelParams, ThinkingParams, ThinkingProfile};

/// Registry of thinking profiles and model mappings.
pub struct ThinkingRegistry {
    profiles: BTreeMap<String, ThinkingProfile>,
    mappings: Vec<ModelMapping>,
    rejections: RejectionCache,
}

impl ThinkingRegistry {
    /// Create a registry with bundled defaults for known models.
    pub fn with_defaults() -> Self {
        Self {
            profiles: defaults::default_profiles(),
            mappings: defaults::default_model_mappings(),
            rejections: RejectionCache::new(),
        }
    }

    /// Look up the thinking profile for a model ID.
    pub fn profile_for_model(&self, model_id: &str) -> &ThinkingProfile {
        let normalized = normalize_model_id(model_id);
        let profile_name = find_best_match(&self.mappings, &normalized).unwrap_or("none");
        self.profiles
            .get(profile_name)
            .unwrap_or_else(|| self.profiles.get("none").expect("none profile must exist"))
    }

    /// Get available levels for a model, excluding runtime-rejected levels.
    pub fn available_levels(&self, model_id: &str) -> Vec<String> {
        let normalized = normalize_model_id(model_id);
        let profile = self.profile_for_model(&normalized);
        profile
            .levels
            .iter()
            .filter(|level| !self.rejections.is_rejected(&normalized, level))
            .cloned()
            .collect()
    }

    /// Get the default thinking level for a model.
    pub fn default_level(&self, model_id: &str) -> String {
        let profile = self.profile_for_model(model_id);
        let available = self.available_levels(model_id);
        if available.contains(&profile.default) {
            profile.default.clone()
        } else {
            available
                .into_iter()
                .next()
                .unwrap_or_else(|| "off".to_owned())
        }
    }

    /// Translate a user-facing level into provider API parameters.
    pub fn translate(&self, model_id: &str, level: &str) -> ThinkingParams {
        let profile = self.profile_for_model(model_id);
        profile.translate(level)
    }

    /// Record that a thinking level was rejected by the provider at runtime.
    pub fn record_rejection(&self, model_id: &str, level: &str) {
        let normalized = normalize_model_id(model_id);
        self.rejections.record(&normalized, level);
    }

    /// Check if a level has been runtime-rejected for a model.
    pub fn is_rejected(&self, model_id: &str, level: &str) -> bool {
        let normalized = normalize_model_id(model_id);
        self.rejections.is_rejected(&normalized, level)
    }
}

/// Strip provider prefix from model ID.
/// `"anthropic/claude-opus-4-6"` → `"claude-opus-4-6"`
fn normalize_model_id(model_id: &str) -> String {
    match model_id.split_once('/') {
        Some((_, model)) => model.to_owned(),
        None => model_id.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_strips_provider_prefix() {
        assert_eq!(
            normalize_model_id("anthropic/claude-opus-4-6"),
            "claude-opus-4-6"
        );
        assert_eq!(normalize_model_id("claude-opus-4-6"), "claude-opus-4-6");
        assert_eq!(normalize_model_id("openai/gpt-5.4"), "gpt-5.4");
    }

    #[test]
    fn registry_returns_correct_profile_for_known_models() {
        let registry = ThinkingRegistry::with_defaults();

        let opus = registry.profile_for_model("claude-opus-4-6");
        assert!(opus.levels.contains(&"adaptive".to_owned()));
        assert!(!opus.levels.contains(&"max".to_owned()));
        assert_eq!(opus.default, "high");

        let sonnet = registry.profile_for_model("claude-sonnet-4-6");
        assert!(sonnet.levels.contains(&"adaptive".to_owned()));
        assert!(!sonnet.levels.contains(&"max".to_owned()));
        assert!(sonnet.levels.contains(&"high".to_owned()));

        let gpt = registry.profile_for_model("gpt-5.4");
        assert!(gpt.levels.contains(&"xhigh".to_owned()));
    }

    #[test]
    fn registry_returns_none_profile_for_unknown_model() {
        let registry = ThinkingRegistry::with_defaults();
        let profile = registry.profile_for_model("totally-unknown-model");
        assert_eq!(profile.levels, vec!["off"]);
    }

    #[test]
    fn available_levels_excludes_rejected() {
        let registry = ThinkingRegistry::with_defaults();
        registry.record_rejection("claude-opus-4-6", "adaptive");

        let available = registry.available_levels("claude-opus-4-6");
        assert!(!available.contains(&"adaptive".to_owned()));
        assert!(available.contains(&"high".to_owned()));
    }

    #[test]
    fn default_level_falls_back_when_default_rejected() {
        let registry = ThinkingRegistry::with_defaults();
        registry.record_rejection("claude-opus-4-6", "high");

        let default = registry.default_level("claude-opus-4-6");
        assert_ne!(default, "high");
        assert!(!default.is_empty());
    }

    #[test]
    fn provider_prefix_stripped_for_lookup() {
        let registry = ThinkingRegistry::with_defaults();
        let direct = registry.available_levels("claude-opus-4-6");
        let prefixed = registry.available_levels("anthropic/claude-opus-4-6");
        assert_eq!(direct, prefixed);
    }

    #[test]
    fn translate_returns_correct_params() {
        let registry = ThinkingRegistry::with_defaults();

        let params = registry.translate("claude-opus-4-6", "high");
        assert!(matches!(params, ThinkingParams::Anthropic { .. }));

        let params = registry.translate("gpt-5.4", "medium");
        assert!(matches!(params, ThinkingParams::OpenAi { .. }));

        let params = registry.translate("claude-opus-4-6", "off");
        assert!(matches!(params, ThinkingParams::Disabled));
    }
}
