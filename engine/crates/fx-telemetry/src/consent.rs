use crate::error::TelemetryError;
use crate::SignalCategory;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

const CONSENT_FILE_NAME: &str = "telemetry-consent.json";

struct ConsentStore {
    path: PathBuf,
}

impl ConsentStore {
    fn new(data_dir: &Path) -> Self {
        Self {
            path: data_dir.join(CONSENT_FILE_NAME),
        }
    }

    fn load(&self) -> Option<TelemetryConsent> {
        std::fs::read_to_string(&self.path)
            .ok()
            .and_then(|contents| serde_json::from_str(&contents).ok())
    }

    fn save(&self, consent: &TelemetryConsent) -> Result<(), TelemetryError> {
        let json = serde_json::to_string_pretty(consent)?;
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, json)?;
        std::fs::rename(&tmp, &self.path)?;
        Ok(())
    }
}

/// User's telemetry consent configuration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TelemetryConsent {
    pub enabled: bool,
    pub categories: HashMap<SignalCategory, bool>,
    pub updated_at: DateTime<Utc>,
}

impl Default for TelemetryConsent {
    fn default() -> Self {
        Self {
            enabled: false,
            categories: HashMap::new(),
            updated_at: Utc::now(),
        }
    }
}

impl TelemetryConsent {
    pub fn load(data_dir: &Path) -> Self {
        ConsentStore::new(data_dir).load().unwrap_or_default()
    }

    pub fn save(&self, data_dir: &Path) -> Result<(), TelemetryError> {
        ConsentStore::new(data_dir).save(self)
    }

    /// Check if a specific category is consented.
    pub fn is_category_enabled(&self, category: &SignalCategory) -> bool {
        self.enabled && self.categories.get(category).copied().unwrap_or(false)
    }

    /// Enable all categories.
    pub fn enable_all(&mut self) {
        self.enabled = true;
        for category in SignalCategory::all() {
            self.categories.insert(category, true);
        }
        self.updated_at = Utc::now();
    }

    /// Disable everything. Clears per-category map so re-enabling
    /// the master switch doesn't silently re-enable all categories.
    pub fn disable_all(&mut self) {
        self.enabled = false;
        self.categories.clear();
        self.updated_at = Utc::now();
    }

    /// Enable a specific category.
    pub fn enable_category(&mut self, category: SignalCategory) {
        self.categories.insert(category, true);
        self.updated_at = Utc::now();
    }

    /// Disable a specific category.
    pub fn disable_category(&mut self, category: SignalCategory) {
        self.categories.insert(category, false);
        self.updated_at = Utc::now();
    }

    /// Count enabled categories.
    pub fn enabled_count(&self) -> usize {
        if !self.enabled {
            return 0;
        }
        self.categories.values().filter(|value| **value).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SignalCollector;
    use std::path::{Path, PathBuf};
    use uuid::Uuid;

    struct TestDataDir {
        path: PathBuf,
    }

    impl TestDataDir {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!("fx-telemetry-{}", Uuid::new_v4()));
            std::fs::create_dir_all(&path).expect("create temp dir");
            Self { path }
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TestDataDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn default_consent_is_disabled() {
        let consent = TelemetryConsent::default();
        assert!(!consent.enabled);
        assert!(consent.categories.is_empty());
        assert_eq!(consent.enabled_count(), 0);
    }

    #[test]
    fn load_returns_default_when_no_file() {
        let temp = TestDataDir::new();
        let consent = TelemetryConsent::load(temp.path());
        assert!(!consent.enabled);
        assert!(consent.categories.is_empty());
        assert_eq!(consent.enabled_count(), 0);
    }

    #[test]
    fn enable_all_enables_everything() {
        let mut consent = TelemetryConsent::default();
        consent.enable_all();
        assert!(consent.enabled);
        for category in SignalCategory::all() {
            assert!(consent.is_category_enabled(&category));
        }
        assert_eq!(consent.enabled_count(), 6);
    }

    #[test]
    fn disable_all_disables_master() {
        let mut consent = TelemetryConsent::default();
        consent.enable_all();
        consent.disable_all();
        assert!(!consent.enabled);
        assert_eq!(consent.enabled_count(), 0);
    }

    #[test]
    fn disable_all_clears_categories_so_reenable_is_clean() {
        let mut consent = TelemetryConsent::default();
        consent.enable_all();
        consent.disable_all();
        // Re-enabling master without explicitly enabling categories
        // should NOT silently re-enable anything.
        consent.enabled = true;
        assert_eq!(consent.enabled_count(), 0);
        assert!(!consent.is_category_enabled(&SignalCategory::ToolUsage));
    }

    #[test]
    fn master_switch_overrides_categories() {
        let mut consent = TelemetryConsent::default();
        consent.enable_category(SignalCategory::ToolUsage);
        assert!(!consent.is_category_enabled(&SignalCategory::ToolUsage));
        consent.enabled = true;
        assert!(consent.is_category_enabled(&SignalCategory::ToolUsage));
    }

    #[test]
    fn per_category_control() {
        let mut consent = TelemetryConsent {
            enabled: true,
            ..TelemetryConsent::default()
        };
        consent.enable_category(SignalCategory::ToolUsage);
        consent.enable_category(SignalCategory::Errors);
        assert!(consent.is_category_enabled(&SignalCategory::ToolUsage));
        assert!(consent.is_category_enabled(&SignalCategory::Errors));
        assert!(!consent.is_category_enabled(&SignalCategory::ModelUsage));
        assert_eq!(consent.enabled_count(), 2);
    }

    #[test]
    fn save_and_load_roundtrip() {
        let temp = TestDataDir::new();
        let mut consent = TelemetryConsent::default();
        consent.enable_all();
        consent.disable_category(SignalCategory::Errors);
        consent.save(temp.path()).expect("save consent");
        let loaded = TelemetryConsent::load(temp.path());
        assert_eq!(loaded, consent);
    }

    #[test]
    fn update_consent_persists_to_disk() {
        let temp = TestDataDir::new();
        let collector = SignalCollector::new_with_persistence(
            TelemetryConsent::default(),
            temp.path().to_path_buf(),
        );
        let mut consent = TelemetryConsent::default();
        consent.enable_all();
        consent.disable_category(SignalCategory::Performance);
        collector
            .update_consent(consent.clone())
            .expect("update consent");
        let loaded = TelemetryConsent::load(temp.path());
        assert_eq!(loaded, consent);
    }

    #[test]
    fn consent_roundtrip_serde() {
        let mut consent = TelemetryConsent::default();
        consent.enable_all();
        let json = serde_json::to_string(&consent).unwrap();
        let decoded: TelemetryConsent = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.enabled, consent.enabled);
        assert_eq!(decoded.enabled_count(), 6);
    }
}
