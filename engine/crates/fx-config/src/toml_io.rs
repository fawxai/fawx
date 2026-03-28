//! TOML parsing, serialization, and targeted config persistence helpers.

use crate::{FawxConfig, ThinkingBudget, DEFAULT_CONFIG_TEMPLATE};
use std::fs;
use std::path::{Path, PathBuf};
use toml_edit::{value, DocumentMut, Item, Table};

impl FawxConfig {
    pub fn load(data_dir: &Path) -> Result<Self, String> {
        let config_path = data_dir.join("config.toml");
        if !config_path.exists() {
            return Ok(Self::default());
        }
        let content = fs::read_to_string(&config_path)
            .map_err(|error| format!("failed to read config: {error}"))?;
        let mut config: Self =
            toml::from_str(&content).map_err(|error| format!("invalid config: {error}"))?;
        config.validate()?;
        config.expand_paths();
        Ok(config)
    }

    pub fn save(&self, data_dir: &Path) -> Result<(), String> {
        let config_path = data_dir.join("config.toml");
        fs::create_dir_all(data_dir).map_err(|error| format!("failed to write config: {error}"))?;
        if config_path.exists() {
            return Err("config.toml already exists; use targeted update helpers".to_string());
        }
        let content = toml::to_string_pretty(self)
            .map_err(|error| format!("failed to serialize config: {error}"))?;
        write_config_file(&config_path, content)
    }

    pub fn write_default(data_dir: &Path) -> Result<PathBuf, String> {
        let config_path = data_dir.join("config.toml");
        if config_path.exists() {
            return Err("config.toml already exists".to_string());
        }
        fs::create_dir_all(data_dir).map_err(|error| format!("failed to write config: {error}"))?;
        fs::write(&config_path, DEFAULT_CONFIG_TEMPLATE)
            .map_err(|error| format!("failed to write config: {error}"))?;
        Ok(config_path)
    }
}

pub fn save_default_model(data_dir: &Path, default_model: &str) -> Result<(), String> {
    let config_path = data_dir.join("config.toml");
    fs::create_dir_all(data_dir).map_err(|error| format!("failed to write config: {error}"))?;
    if config_path.exists() {
        return update_default_model(&config_path, default_model);
    }
    create_model_config(data_dir, default_model)
}

/// Persist the thinking budget to `config.toml`, preserving comments.
pub fn save_thinking_budget(data_dir: &Path, budget: ThinkingBudget) -> Result<(), String> {
    let config_path = data_dir.join("config.toml");
    fs::create_dir_all(data_dir).map_err(|error| format!("failed to write config: {error}"))?;
    if config_path.exists() {
        return update_thinking_budget(&config_path, budget);
    }
    let mut config = FawxConfig::default();
    config.general.thinking = Some(budget);
    config.save(data_dir)
}

fn update_thinking_budget(config_path: &Path, budget: ThinkingBudget) -> Result<(), String> {
    let content = fs::read_to_string(config_path)
        .map_err(|error| format!("failed to read config: {error}"))?;
    let mut document = parse_config_document(&content)?;
    set_string_field(&mut document, &["general"], "thinking", &budget.to_string())?;
    write_config_file(config_path, document.to_string())
}

fn create_model_config(data_dir: &Path, default_model: &str) -> Result<(), String> {
    let mut config = FawxConfig::default();
    config.model.default_model = Some(default_model.to_string());
    config.save(data_dir)
}

fn update_default_model(config_path: &Path, default_model: &str) -> Result<(), String> {
    let content = fs::read_to_string(config_path)
        .map_err(|error| format!("failed to read config: {error}"))?;
    let mut document = parse_config_document(&content)?;
    set_string_field(&mut document, &["model"], "default_model", default_model)?;
    write_config_file(config_path, document.to_string())
}

pub(crate) fn parse_config_document(content: &str) -> Result<DocumentMut, String> {
    content
        .parse::<DocumentMut>()
        .map_err(|error| format!("invalid config: {error}"))
}

/// Set a field in a TOML document, inferring the correct value type.
///
/// Attempts to parse `field_value` as an integer, float, or boolean before
/// falling back to a string. When updating an existing key the original
/// value's type is preferred (e.g. an existing integer stays integer even
/// if the new value could be read as a string). Inline comments/decor on
/// the original value are preserved.
pub(crate) fn set_typed_field(
    document: &mut DocumentMut,
    sections: &[&str],
    key: &str,
    field_value: &str,
) -> Result<(), String> {
    let table = get_or_insert_table(document, sections)?;
    if let Some(item) = table.get_mut(key) {
        return update_typed_item(item, key, field_value);
    }
    // New key - infer type from the raw string.
    table[key] = infer_typed_value(field_value);
    Ok(())
}

/// Infer a `toml_edit::Value` from a raw string, trying integer -> bool -> string.
fn infer_typed_value(raw: &str) -> Item {
    if let Ok(n) = raw.parse::<i64>() {
        return value(n);
    }
    match raw {
        "true" => return value(true),
        "false" => return value(false),
        _ => {}
    }
    value(raw)
}

fn update_typed_item(item: &mut Item, key: &str, field_value: &str) -> Result<(), String> {
    let existing = item
        .as_value()
        .ok_or_else(|| format!("config field '{key}' must be a value"))?;
    let decor = existing.decor().clone();

    // Match the existing value's type when possible.
    let new_item = if existing.is_integer() {
        if let Ok(n) = field_value.parse::<i64>() {
            value(n)
        } else {
            // Fall back to string if the new value isn't numeric.
            value(field_value)
        }
    } else if existing.is_bool() {
        match field_value {
            "true" => value(true),
            "false" => value(false),
            _ => value(field_value),
        }
    } else {
        value(field_value)
    };

    *item = new_item;
    item.as_value_mut()
        .ok_or_else(|| format!("config field '{key}' must be a value"))?
        .decor_mut()
        .clone_from(&decor);
    Ok(())
}

// Keep the old name as a thin wrapper for callers that always want strings.
pub(crate) fn set_string_field(
    document: &mut DocumentMut,
    sections: &[&str],
    key: &str,
    field_value: &str,
) -> Result<(), String> {
    let table = get_or_insert_table(document, sections)?;
    if let Some(item) = table.get_mut(key) {
        let decor = item
            .as_value()
            .ok_or_else(|| format!("config field '{key}' must be a value"))?
            .decor()
            .clone();
        *item = value(field_value);
        item.as_value_mut()
            .ok_or_else(|| format!("config field '{key}' must be a value"))?
            .decor_mut()
            .clone_from(&decor);
        return Ok(());
    }
    table[key] = value(field_value);
    Ok(())
}

fn get_or_insert_table<'a>(
    document: &'a mut DocumentMut,
    sections: &[&str],
) -> Result<&'a mut Table, String> {
    get_or_insert_table_in(document.as_table_mut(), sections)
}

fn get_or_insert_table_in<'a>(
    table: &'a mut Table,
    sections: &[&str],
) -> Result<&'a mut Table, String> {
    let Some((section, rest)) = sections.split_first() else {
        return Ok(table);
    };
    if !table.contains_key(section) {
        table[*section] = Item::Table(Table::new());
    }
    let child = table[*section]
        .as_table_mut()
        .ok_or_else(|| format!("config section '{section}' must be a table"))?;
    get_or_insert_table_in(child, rest)
}

pub(crate) fn write_config_file(config_path: &Path, content: String) -> Result<(), String> {
    fs::write(config_path, content).map_err(|error| format!("failed to write config: {error}"))
}
