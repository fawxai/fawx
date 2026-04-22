use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const PROVIDER_MODEL_CACHE_FILE: &str = "provider-model-cache.json";

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct ProviderModelCache {
    providers: BTreeMap<String, ProviderModelCacheEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProviderModelCacheEntry {
    pub models: Vec<String>,
    pub updated_at: u64,
}

impl ProviderModelCache {
    pub fn models_for(&self, provider: &str) -> Option<Vec<String>> {
        self.providers
            .get(&normalize_provider(provider))
            .map(|entry| entry.models.clone())
    }

    pub fn set_models(&mut self, provider: &str, models: &[String]) {
        self.providers.insert(
            normalize_provider(provider),
            ProviderModelCacheEntry {
                models: normalize_models(models),
                updated_at: current_unix_timestamp_secs(),
            },
        );
    }

    pub fn clear_provider(&mut self, provider: &str) -> bool {
        self.providers
            .remove(&normalize_provider(provider))
            .is_some()
    }
}

pub fn load_provider_model_cache(data_dir: &Path) -> Result<ProviderModelCache, String> {
    let path = provider_model_cache_path(data_dir);
    if !path.exists() {
        return Ok(ProviderModelCache::default());
    }

    let content = fs::read_to_string(&path)
        .map_err(|error| format!("failed to read model cache: {error}"))?;
    if content.trim().is_empty() {
        return Ok(ProviderModelCache::default());
    }

    serde_json::from_str(&content).map_err(|error| format!("invalid model cache: {error}"))
}

pub fn save_provider_model_cache(
    data_dir: &Path,
    cache: &ProviderModelCache,
) -> Result<(), String> {
    fs::create_dir_all(data_dir)
        .map_err(|error| format!("failed to write model cache: {error}"))?;
    let path = provider_model_cache_path(data_dir);
    let content = serde_json::to_string_pretty(cache)
        .map_err(|error| format!("failed to serialize model cache: {error}"))?;
    fs::write(path, content).map_err(|error| format!("failed to write model cache: {error}"))
}

pub fn update_provider_model_cache(
    data_dir: &Path,
    provider: &str,
    models: &[String],
) -> Result<(), String> {
    let mut cache = load_provider_model_cache(data_dir)?;
    cache.set_models(provider, models);
    save_provider_model_cache(data_dir, &cache)
}

pub fn clear_provider_model_cache(data_dir: &Path, provider: &str) -> Result<(), String> {
    let path = provider_model_cache_path(data_dir);
    if !path.exists() {
        return Ok(());
    }

    let mut cache = load_provider_model_cache(data_dir)?;
    if !cache.clear_provider(provider) {
        return Ok(());
    }

    save_provider_model_cache(data_dir, &cache)
}

fn provider_model_cache_path(data_dir: &Path) -> PathBuf {
    data_dir.join(PROVIDER_MODEL_CACHE_FILE)
}

fn normalize_provider(provider: &str) -> String {
    provider.trim().to_ascii_lowercase()
}

fn normalize_models(models: &[String]) -> Vec<String> {
    let mut normalized = models
        .iter()
        .map(|model| model.trim().to_string())
        .filter(|model| !model.is_empty())
        .collect::<Vec<_>>();
    normalized.sort();
    normalized.dedup();
    normalized
}

fn current_unix_timestamp_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::{
        clear_provider_model_cache, load_provider_model_cache, save_provider_model_cache,
        update_provider_model_cache, ProviderModelCache,
    };

    #[test]
    fn load_missing_model_cache_returns_default() {
        let temp = tempfile::TempDir::new().expect("tempdir");

        let cache = load_provider_model_cache(temp.path()).expect("load cache");

        assert_eq!(cache, ProviderModelCache::default());
    }

    #[test]
    fn update_provider_model_cache_sorts_and_deduplicates_models() {
        let temp = tempfile::TempDir::new().expect("tempdir");

        update_provider_model_cache(
            temp.path(),
            "Fireworks",
            &[
                " accounts/fireworks/routers/kimi-k2p5-turbo ".to_string(),
                "accounts/fireworks/models/deepseek-v3".to_string(),
                "accounts/fireworks/routers/kimi-k2p5-turbo".to_string(),
            ],
        )
        .expect("update cache");

        let cache = load_provider_model_cache(temp.path()).expect("reload cache");
        assert_eq!(
            cache.models_for("fireworks"),
            Some(vec![
                "accounts/fireworks/models/deepseek-v3".to_string(),
                "accounts/fireworks/routers/kimi-k2p5-turbo".to_string(),
            ])
        );
    }

    #[test]
    fn clear_provider_model_cache_removes_only_target_provider() {
        let temp = tempfile::TempDir::new().expect("tempdir");
        let mut cache = ProviderModelCache::default();
        cache.set_models("openai", &["gpt-5.4".to_string()]);
        cache.set_models(
            "fireworks",
            &["accounts/fireworks/routers/kimi-k2p5-turbo".to_string()],
        );
        save_provider_model_cache(temp.path(), &cache).expect("save cache");

        clear_provider_model_cache(temp.path(), "fireworks").expect("clear cache");

        let reloaded = load_provider_model_cache(temp.path()).expect("reload cache");
        assert_eq!(reloaded.models_for("fireworks"), None);
        assert_eq!(
            reloaded.models_for("openai"),
            Some(vec!["gpt-5.4".to_string()])
        );
    }
}
