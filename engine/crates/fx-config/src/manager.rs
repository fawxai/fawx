//! Configuration manager for runtime config read/write with validation.
//!
//! Provides [`ConfigManager`] which wraps [`FawxConfig`] with safe get/set
//! operations, validation before write, and comment-preserving persistence
//! using `toml_edit`.

use crate::{
    parse_config_document, parse_log_level, set_typed_field, validate_synthesis_instruction,
    write_config_file, FawxConfig, VALID_LOG_LEVELS,
};
use serde_json::Value as JsonValue;
use std::path::{Path, PathBuf};

/// Fields that cannot be modified at runtime. Changing these requires
/// a manual edit + full restart.
const IMMUTABLE_FIELDS: &[&str] = &[
    "general.data_dir",
    "security.require_signatures",
    "self_modify.paths.deny",
];

/// Maximum valid port number.
const MAX_PORT: u16 = 65535;

/// Manages Fawx configuration with validated read/write and
/// comment-preserving persistence.
#[derive(Debug)]
pub struct ConfigManager {
    config_path: PathBuf,
    current: FawxConfig,
}

impl ConfigManager {
    /// Create a new manager from a config directory.
    ///
    /// Loads the existing `config.toml` or uses defaults if the file
    /// does not exist.
    pub fn new(data_dir: &Path) -> Result<Self, String> {
        let config_path = data_dir.join("config.toml");
        let current = FawxConfig::load(data_dir)?;
        Ok(Self {
            config_path,
            current,
        })
    }

    /// Create a manager from an already-loaded config and explicit path.
    pub fn from_config(config: FawxConfig, config_path: PathBuf) -> Self {
        Self {
            config_path,
            current: config,
        }
    }

    /// Return a reference to the current in-memory configuration.
    pub fn config(&self) -> &FawxConfig {
        &self.current
    }

    /// Read a config key or section as JSON. Use `"all"` for the entire config.
    pub fn get(&self, section: &str) -> Result<JsonValue, String> {
        serialize_selection(&self.current, section)
    }

    /// Update a config value by dot-separated key path.
    ///
    /// Validates the new config before writing to disk.
    pub fn set(&mut self, key: &str, value: &str) -> Result<(), String> {
        reject_immutable(key)?;
        let (sections, field) = parse_key_path(key)?;
        validate_field_value(key, value)?;

        // Apply to disk via toml_edit (preserves comments).
        self.apply_to_file(&sections, field, value)?;

        // Reload to pick up the change and re-validate the full config.
        self.reload()
    }

    /// Remove a config key by dot-separated path.
    pub fn clear(&mut self, key: &str) -> Result<(), String> {
        reject_immutable(key)?;
        let (sections, field) = parse_key_path(key)?;
        self.clear_from_file(&sections, field)?;
        self.reload()
    }

    /// Persist the current in-memory config to disk.
    ///
    /// If a config file already exists on disk, the write is done via
    /// `toml_edit` so that comments and formatting are preserved. When no
    /// file exists yet, a fresh `toml::to_string_pretty` serialization is
    /// used.
    pub fn save(&self) -> Result<(), String> {
        let existing = read_or_default(&self.config_path)?;
        if existing.is_empty() {
            let content = toml::to_string_pretty(&self.current)
                .map_err(|e| format!("failed to serialize config: {e}"))?;
            return write_config_file(&self.config_path, content);
        }
        // Re-serialize via toml_edit to preserve comments/formatting.
        let fresh = toml::to_string_pretty(&self.current)
            .map_err(|e| format!("failed to serialize config: {e}"))?;
        let fresh_table: toml::Value =
            toml::from_str(&fresh).map_err(|e| format!("failed to parse fresh config: {e}"))?;
        let mut doc = parse_config_document(&existing)?;
        merge_toml_value_into_doc(&mut doc, &fresh_table)?;
        write_config_file(&self.config_path, doc.to_string())
    }

    /// Reload config from disk into memory.
    pub fn reload(&mut self) -> Result<(), String> {
        let dir = self
            .config_path
            .parent()
            .ok_or_else(|| "config path has no parent directory".to_string())?;
        self.current = FawxConfig::load(dir)?;
        Ok(())
    }

    /// Path to the underlying config file.
    pub fn config_path(&self) -> &Path {
        &self.config_path
    }

    // ── Internal helpers ────────────────────────────────────────────────

    fn apply_to_file(&self, sections: &[&str], field: &str, value: &str) -> Result<(), String> {
        let content = read_or_default(&self.config_path)?;
        let mut document = parse_config_document(&content)?;
        set_typed_field(&mut document, sections, field, value)?;
        write_config_file(&self.config_path, document.to_string())
    }

    fn clear_from_file(&self, sections: &[&str], field: &str) -> Result<(), String> {
        let content = read_or_default(&self.config_path)?;
        if content.is_empty() {
            return Ok(());
        }
        let mut document = parse_config_document(&content)?;
        remove_field(document.as_table_mut(), sections, field)?;
        write_config_file(&self.config_path, document.to_string())
    }
}

// ── Free functions ──────────────────────────────────────────────────────────

/// Reject writes to immutable fields.
fn reject_immutable(key: &str) -> Result<(), String> {
    if IMMUTABLE_FIELDS.contains(&key) {
        return Err(format!(
            "field '{key}' is immutable at runtime; edit config.toml manually and restart"
        ));
    }
    Ok(())
}

/// Parse a dot-separated key into (sections, field).
///
/// Example: `"model.default_model"` → `(["model"], "default_model")`
fn parse_key_path(key: &str) -> Result<(Vec<&str>, &str), String> {
    let parts: Vec<&str> = key.split('.').collect();
    if parts.len() < 2 || parts.iter().any(|part| part.is_empty()) {
        return Err(format!(
            "key must be dot-separated (e.g. 'model.default_model'), got '{key}'"
        ));
    }
    let field = parts.last().ok_or_else(|| "empty key path".to_string())?;
    let sections = &parts[..parts.len() - 1];
    Ok((sections.to_vec(), field))
}

/// Field-specific value validation (pre-write checks).
fn validate_field_value(key: &str, value: &str) -> Result<(), String> {
    match key {
        "http.port" => validate_port(value),
        "general.max_iterations" => validate_positive_u32(key, value),
        "general.max_history" => validate_positive_usize(key, value),
        "tools.max_read_size" => validate_min_u64(key, value, 1024),
        "memory.max_entries" => validate_positive_usize(key, value),
        "model.default_model" => validate_model_name(value),
        "model.synthesis_instruction" => validate_synthesis_instruction(value),
        "logging.max_files" => validate_positive_usize(key, value),
        "logging.file_level" | "logging.stderr_level" => validate_log_level(value),
        _ => Ok(()),
    }
}

fn validate_log_level(value: &str) -> Result<(), String> {
    if parse_log_level(value).is_some() {
        return Ok(());
    }
    Err(format!("log level must be one of: {VALID_LOG_LEVELS}"))
}

fn validate_model_name(value: &str) -> Result<(), String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err("model name must not be empty".to_string());
    }
    // Basic format check: model names are alphanumeric with hyphens,
    // dots, underscores, and forward slashes (for provider/model format).
    if !trimmed
        .chars()
        .all(|c| c.is_alphanumeric() || "-_./:".contains(c))
    {
        return Err(format!(
            "model name contains invalid characters: '{trimmed}'"
        ));
    }
    Ok(())
}

fn validate_port(value: &str) -> Result<(), String> {
    let port: u16 = value
        .parse()
        .map_err(|_| format!("invalid port number: '{value}'"))?;
    if port == 0 {
        return Err(format!("port must be between 1 and {MAX_PORT}"));
    }
    Ok(())
}

fn validate_positive_u32(key: &str, value: &str) -> Result<(), String> {
    let n: u32 = value
        .parse()
        .map_err(|_| format!("{key}: expected a positive integer, got '{value}'"))?;
    if n == 0 {
        return Err(format!("{key} must be >= 1"));
    }
    Ok(())
}

fn validate_positive_usize(key: &str, value: &str) -> Result<(), String> {
    let n: usize = value
        .parse()
        .map_err(|_| format!("{key}: expected a positive integer, got '{value}'"))?;
    if n == 0 {
        return Err(format!("{key} must be >= 1"));
    }
    Ok(())
}

fn validate_min_u64(key: &str, value: &str, min: u64) -> Result<(), String> {
    let n: u64 = value
        .parse()
        .map_err(|_| format!("{key}: expected an integer, got '{value}'"))?;
    if n < min {
        return Err(format!("{key} must be >= {min}"));
    }
    Ok(())
}

/// Recursively merge a `toml::Value` (table tree) into a `toml_edit::DocumentMut`,
/// preserving comments and formatting on existing keys.
fn merge_toml_value_into_doc(
    doc: &mut toml_edit::DocumentMut,
    fresh: &toml::Value,
) -> Result<(), String> {
    let fresh_table = fresh
        .as_table()
        .ok_or_else(|| "top-level value must be a table".to_string())?;
    merge_table(doc.as_table_mut(), fresh_table);
    Ok(())
}

fn merge_table(target: &mut toml_edit::Table, source: &toml::value::Table) {
    for (key, val) in source {
        match val {
            toml::Value::Table(sub) => {
                if !target.contains_key(key) {
                    target[key] = toml_edit::Item::Table(toml_edit::Table::new());
                }
                if let Some(child) = target[key].as_table_mut() {
                    merge_table(child, sub);
                }
            }
            _ => {
                let new_item = toml_value_to_edit_item(val);
                if let Some(existing) = target.get(key) {
                    if let Some(old_val) = existing.as_value() {
                        let decor = old_val.decor().clone();
                        target[key] = new_item;
                        if let Some(v) = target[key].as_value_mut() {
                            v.decor_mut().clone_from(&decor);
                        }
                        continue;
                    }
                }
                target[key] = new_item;
            }
        }
    }
}

fn toml_value_to_edit_item(val: &toml::Value) -> toml_edit::Item {
    match val {
        toml::Value::String(s) => toml_edit::value(s.as_str()),
        toml::Value::Integer(n) => toml_edit::value(*n),
        toml::Value::Float(f) => toml_edit::value(*f),
        toml::Value::Boolean(b) => toml_edit::value(*b),
        toml::Value::Array(arr) => {
            let mut a = toml_edit::Array::new();
            for v in arr {
                if let Ok(edit_val) = toml_value_to_edit_item(v).into_value() {
                    a.push(edit_val);
                }
            }
            toml_edit::value(a)
        }
        toml::Value::Table(_) => {
            // Nested tables handled by merge_table, shouldn't reach here.
            toml_edit::Item::None
        }
        toml::Value::Datetime(dt) => toml_edit::value(dt.to_string().as_str()),
    }
}

fn remove_field(
    table: &mut toml_edit::Table,
    sections: &[&str],
    field: &str,
) -> Result<(), String> {
    if let Some(target) = table_for_path(table, sections)? {
        target.remove(field);
    }
    Ok(())
}

fn table_for_path<'a>(
    table: &'a mut toml_edit::Table,
    sections: &[&str],
) -> Result<Option<&'a mut toml_edit::Table>, String> {
    let Some((section, rest)) = sections.split_first() else {
        return Ok(Some(table));
    };
    let Some(item) = table.get_mut(section) else {
        return Ok(None);
    };
    let child = item
        .as_table_mut()
        .ok_or_else(|| format!("config section '{section}' must be a table"))?;
    table_for_path(child, rest)
}

/// Read config file contents, or return an empty string if it doesn't exist.
fn read_or_default(path: &Path) -> Result<String, String> {
    if path.exists() {
        std::fs::read_to_string(path).map_err(|e| format!("failed to read config: {e}"))
    } else {
        Ok(String::new())
    }
}

/// Serialize a config selection to JSON.
fn serialize_selection(config: &FawxConfig, selection: &str) -> Result<JsonValue, String> {
    let full =
        serde_json::to_value(config).map_err(|e| format!("failed to serialize config: {e}"))?;
    if selection == "all" {
        return Ok(full);
    }
    lookup_selection(&full, selection)
        .cloned()
        .ok_or_else(|| format!("unknown config key or section: '{selection}'"))
}

fn lookup_selection<'a>(value: &'a JsonValue, selection: &str) -> Option<&'a JsonValue> {
    selection
        .split('.')
        .try_fold(value, |current, segment| match current {
            JsonValue::Object(map) => map.get(segment),
            _ => None,
        })
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write_config(dir: &Path, content: &str) {
        std::fs::write(dir.join("config.toml"), content).expect("write config");
    }

    #[test]
    fn new_loads_defaults_when_no_file() {
        let temp = TempDir::new().expect("tempdir");
        let mgr = ConfigManager::new(temp.path()).expect("create manager");
        assert_eq!(mgr.config().general.max_iterations, 10);
    }

    #[test]
    fn new_loads_existing_config() {
        let temp = TempDir::new().expect("tempdir");
        write_config(temp.path(), "[general]\nmax_iterations = 42\n");
        let mgr = ConfigManager::new(temp.path()).expect("create manager");
        assert_eq!(mgr.config().general.max_iterations, 42);
    }

    #[test]
    fn get_returns_section_as_json() {
        let temp = TempDir::new().expect("tempdir");
        write_config(temp.path(), "[model]\ndefault_model = \"test-model\"\n");
        let mgr = ConfigManager::new(temp.path()).expect("manager");
        let val = mgr.get("model").expect("get model");
        assert_eq!(val["default_model"], "test-model");
    }

    #[test]
    fn get_all_returns_full_config() {
        let temp = TempDir::new().expect("tempdir");
        let mgr = ConfigManager::new(temp.path()).expect("manager");
        let val = mgr.get("all").expect("get all");
        assert!(val.get("general").is_some());
        assert!(val.get("model").is_some());
    }

    #[test]
    fn get_unknown_section_returns_error() {
        let temp = TempDir::new().expect("tempdir");
        let mgr = ConfigManager::new(temp.path()).expect("manager");
        let err = mgr.get("nonexistent").unwrap_err();
        assert!(err.contains("unknown config key or section"));
    }

    #[test]
    fn get_returns_nested_key() {
        let temp = TempDir::new().expect("tempdir");
        write_config(temp.path(), "[model]\ndefault_model = \"test-model\"\n");
        let mgr = ConfigManager::new(temp.path()).expect("manager");
        let val = mgr.get("model.default_model").expect("get key");
        assert_eq!(val, JsonValue::String("test-model".to_string()));
    }

    #[test]
    fn set_updates_value_and_persists() {
        let temp = TempDir::new().expect("tempdir");
        write_config(temp.path(), "[model]\ndefault_model = \"old-model\"\n");
        let mut mgr = ConfigManager::new(temp.path()).expect("manager");

        mgr.set("model.default_model", "new-model")
            .expect("set model");

        assert_eq!(
            mgr.config().model.default_model.as_deref(),
            Some("new-model")
        );

        // Verify persisted to disk
        let content = std::fs::read_to_string(temp.path().join("config.toml")).expect("read");
        assert!(content.contains("new-model"));
    }

    #[test]
    fn set_preserves_comments() {
        let temp = TempDir::new().expect("tempdir");
        write_config(
            temp.path(),
            "# header comment\n[model]\n# field comment\ndefault_model = \"old\"\n",
        );
        let mut mgr = ConfigManager::new(temp.path()).expect("manager");

        mgr.set("model.default_model", "new").expect("set");

        let content = std::fs::read_to_string(temp.path().join("config.toml")).expect("read");
        assert!(content.contains("# header comment"));
        assert!(content.contains("# field comment"));
    }

    #[test]
    fn clear_removes_key() {
        let temp = TempDir::new().expect("tempdir");
        write_config(
            temp.path(),
            "[model]\ndefault_model = \"test-model\"\nsynthesis_instruction = \"Stay concise\"\n",
        );
        let mut mgr = ConfigManager::new(temp.path()).expect("manager");

        mgr.clear("model.synthesis_instruction")
            .expect("clear synthesis");

        assert_eq!(mgr.config().model.synthesis_instruction, None);
        let content = std::fs::read_to_string(temp.path().join("config.toml")).expect("read");
        assert!(!content.contains("synthesis_instruction"));
        assert!(content.contains("default_model = \"test-model\""));
    }

    #[test]
    fn clear_preserves_comments() {
        let temp = TempDir::new().expect("tempdir");
        write_config(
            temp.path(),
            "# header comment\n[model]\n# keep this comment\ndefault_model = \"test-model\"\nsynthesis_instruction = \"Stay concise\"\n",
        );
        let mut mgr = ConfigManager::new(temp.path()).expect("manager");

        mgr.clear("model.synthesis_instruction")
            .expect("clear synthesis");

        let content = std::fs::read_to_string(temp.path().join("config.toml")).expect("read");
        assert!(content.contains("# header comment"));
        assert!(content.contains("# keep this comment"));
        assert!(content.contains("default_model = \"test-model\""));
    }

    #[test]
    fn set_rejects_immutable_data_dir() {
        let temp = TempDir::new().expect("tempdir");
        let mut mgr = ConfigManager::new(temp.path()).expect("manager");
        let err = mgr.set("general.data_dir", "/tmp").unwrap_err();
        assert!(err.contains("immutable"));
    }

    #[test]
    fn set_rejects_immutable_security() {
        let temp = TempDir::new().expect("tempdir");
        let mut mgr = ConfigManager::new(temp.path()).expect("manager");
        let err = mgr.set("security.require_signatures", "true").unwrap_err();
        assert!(err.contains("immutable"));
    }

    #[test]
    fn set_rejects_invalid_key_format() {
        let temp = TempDir::new().expect("tempdir");
        let mut mgr = ConfigManager::new(temp.path()).expect("manager");
        let err = mgr.set("nodot", "value").unwrap_err();
        assert!(err.contains("dot-separated"));
    }

    #[test]
    fn set_validates_max_iterations_positive() {
        let temp = TempDir::new().expect("tempdir");
        write_config(temp.path(), "[general]\nmax_iterations = 10\n");
        let mut mgr = ConfigManager::new(temp.path()).expect("manager");

        let err = mgr.set("general.max_iterations", "0").unwrap_err();
        assert!(err.contains("must be >= 1"));
    }

    #[test]
    fn set_rejects_empty_synthesis_instruction() {
        let temp = TempDir::new().expect("tempdir");
        write_config(temp.path(), "[model]\ndefault_model = \"test-model\"\n");
        let mut mgr = ConfigManager::new(temp.path()).expect("manager");

        let err = mgr.set("model.synthesis_instruction", "   ").unwrap_err();
        assert!(err.contains("synthesis_instruction must not be empty"));
    }

    #[test]
    fn set_validates_max_read_size_minimum() {
        let temp = TempDir::new().expect("tempdir");
        write_config(temp.path(), "[tools]\nmax_read_size = 2048\n");
        let mut mgr = ConfigManager::new(temp.path()).expect("manager");

        let err = mgr.set("tools.max_read_size", "100").unwrap_err();
        assert!(err.contains("must be >= 1024"));
    }

    #[test]
    fn set_accepts_supported_log_level() {
        let temp = TempDir::new().expect("tempdir");
        write_config(temp.path(), "[logging]\nfile_level = \"info\"\n");
        let mut mgr = ConfigManager::new(temp.path()).expect("manager");

        mgr.set("logging.file_level", "TRACE")
            .expect("set log level");

        assert_eq!(mgr.config().logging.file_level.as_deref(), Some("TRACE"));
    }

    #[test]
    fn set_rejects_invalid_log_level() {
        let temp = TempDir::new().expect("tempdir");
        write_config(temp.path(), "[logging]\nfile_level = \"info\"\n");
        let mut mgr = ConfigManager::new(temp.path()).expect("manager");

        let err = mgr.set("logging.file_level", "verbose").unwrap_err();
        assert!(err.contains("log level must be one of"));
    }

    #[test]
    fn reload_picks_up_external_changes() {
        let temp = TempDir::new().expect("tempdir");
        write_config(temp.path(), "[general]\nmax_iterations = 5\n");
        let mut mgr = ConfigManager::new(temp.path()).expect("manager");
        assert_eq!(mgr.config().general.max_iterations, 5);

        // Simulate external edit
        write_config(temp.path(), "[general]\nmax_iterations = 99\n");
        mgr.reload().expect("reload");
        assert_eq!(mgr.config().general.max_iterations, 99);
    }

    #[test]
    fn set_creates_missing_section() {
        let temp = TempDir::new().expect("tempdir");
        write_config(temp.path(), "[general]\nmax_iterations = 10\n");
        let mut mgr = ConfigManager::new(temp.path()).expect("manager");

        mgr.set("model.default_model", "new-model")
            .expect("set creates section");

        assert_eq!(
            mgr.config().model.default_model.as_deref(),
            Some("new-model")
        );
    }

    #[test]
    fn set_creates_config_file_when_missing() {
        let temp = TempDir::new().expect("tempdir");
        // No config.toml exists
        let mut mgr = ConfigManager::new(temp.path()).expect("manager");

        mgr.set("model.default_model", "brand-new")
            .expect("set on empty");

        assert_eq!(
            mgr.config().model.default_model.as_deref(),
            Some("brand-new")
        );
        assert!(temp.path().join("config.toml").exists());
    }

    #[test]
    fn parse_key_path_splits_correctly() {
        let (sections, field) = parse_key_path("model.default_model").expect("parse");
        assert_eq!(sections, vec!["model"]);
        assert_eq!(field, "default_model");
    }

    #[test]
    fn parse_key_path_deep_nesting() {
        let (sections, field) = parse_key_path("self_modify.paths.allow").expect("parse");
        assert_eq!(sections, vec!["self_modify", "paths"]);
        assert_eq!(field, "allow");
    }

    #[test]
    fn validate_port_accepts_valid() {
        assert!(validate_port("8400").is_ok());
        assert!(validate_port("1").is_ok());
        assert!(validate_port("65535").is_ok());
    }

    #[test]
    fn validate_port_rejects_invalid() {
        assert!(validate_port("0").is_err());
        assert!(validate_port("70000").is_err());
        assert!(validate_port("abc").is_err());
    }

    #[test]
    fn from_config_preserves_values() {
        let config = FawxConfig {
            general: crate::GeneralConfig {
                max_iterations: 42,
                ..crate::GeneralConfig::default()
            },
            ..FawxConfig::default()
        };
        let mgr =
            ConfigManager::from_config(config.clone(), PathBuf::from("/tmp/test/config.toml"));
        assert_eq!(mgr.config().general.max_iterations, 42);
    }

    // ── B1 regression: numeric fields must not be quoted ────────────────

    #[test]
    fn set_integer_field_writes_unquoted_number() {
        let temp = TempDir::new().expect("tempdir");
        write_config(temp.path(), "[general]\nmax_iterations = 10\n");
        let mut mgr = ConfigManager::new(temp.path()).expect("manager");

        mgr.set("general.max_iterations", "20")
            .expect("set iterations");

        let content = std::fs::read_to_string(temp.path().join("config.toml")).expect("read");
        assert!(
            content.contains("max_iterations = 20"),
            "integer should be unquoted, got: {content}"
        );
        assert!(
            !content.contains("\"20\""),
            "integer should NOT be quoted, got: {content}"
        );
    }

    #[test]
    fn set_port_writes_unquoted_number() {
        let temp = TempDir::new().expect("tempdir");
        write_config(temp.path(), "[http]\nport = 8400\n");
        let mut mgr = ConfigManager::new(temp.path()).expect("manager");

        mgr.set("http.port", "9090").expect("set port");

        let content = std::fs::read_to_string(temp.path().join("config.toml")).expect("read");
        assert!(
            content.contains("port = 9090"),
            "port should be unquoted integer, got: {content}"
        );
    }

    #[test]
    fn set_preserves_bool_type() {
        let temp = TempDir::new().expect("tempdir");
        write_config(temp.path(), "[tools]\njail = true\n");
        let mut mgr = ConfigManager::new(temp.path()).expect("manager");

        mgr.set("tools.jail", "false").expect("set bool");

        let content = std::fs::read_to_string(temp.path().join("config.toml")).expect("read");
        assert!(
            content.contains("jail = false"),
            "bool should be unquoted, got: {content}"
        );
    }

    // ── B1 regression: reload succeeds after numeric set ────────────────

    #[test]
    fn set_numeric_then_reload_succeeds() {
        let temp = TempDir::new().expect("tempdir");
        write_config(temp.path(), "[general]\nmax_iterations = 10\n");
        let mut mgr = ConfigManager::new(temp.path()).expect("manager");

        mgr.set("general.max_iterations", "42")
            .expect("set iterations");

        // This would fail before the fix because "42" was quoted as a string.
        assert_eq!(mgr.config().general.max_iterations, 42);
    }

    // ── B2 regression: save() preserves comments ────────────────────────

    #[test]
    fn save_preserves_comments_in_existing_file() {
        let temp = TempDir::new().expect("tempdir");
        let content = "# important header\n[general]\n# iteration limit\nmax_iterations = 10\n";
        write_config(temp.path(), content);
        let mgr = ConfigManager::new(temp.path()).expect("manager");

        mgr.save().expect("save");

        let saved = std::fs::read_to_string(temp.path().join("config.toml")).expect("read");
        assert!(
            saved.contains("# important header"),
            "header comment missing after save: {saved}"
        );
        assert!(
            saved.contains("# iteration limit"),
            "field comment missing after save: {saved}"
        );
    }

    #[test]
    fn save_creates_fresh_file_when_none_exists() {
        let temp = TempDir::new().expect("tempdir");
        let mgr = ConfigManager::new(temp.path()).expect("manager");

        mgr.save().expect("save");

        assert!(temp.path().join("config.toml").exists());
    }

    // ── Fix 10: model name validation ───────────────────────────────────

    // Model name validation is a soft check — we don't have a model catalog
    // in fx-config, so we just ensure non-empty model names are accepted.
    #[test]
    fn set_model_accepts_valid_name() {
        let temp = TempDir::new().expect("tempdir");
        write_config(temp.path(), "[model]\ndefault_model = \"old\"\n");
        let mut mgr = ConfigManager::new(temp.path()).expect("manager");
        mgr.set("model.default_model", "claude-opus-4-6")
            .expect("valid model name");
    }
}
