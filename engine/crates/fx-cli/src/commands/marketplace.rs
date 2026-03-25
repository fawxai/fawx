//! CLI commands for the skill marketplace (search, install, list).

use std::path::PathBuf;

use crate::startup;
use fx_marketplace::{InstalledSkill, SkillEntry};

/// Resolve the Fawx data directory.
fn data_dir() -> PathBuf {
    startup::fawx_data_dir()
}

/// Build a `RegistryConfig` from defaults.
fn build_config() -> anyhow::Result<fx_marketplace::RegistryConfig> {
    let data = data_dir();
    Ok(fx_marketplace::default_config(&data)?)
}

/// Render a list of skill entries from search results.
fn render_search_results(entries: &[SkillEntry]) -> String {
    if entries.is_empty() {
        return "No skills found.".to_string();
    }

    let mut lines = Vec::new();
    for entry in entries {
        let size = entry
            .size_bytes
            .map(|bytes| format!("{} KB", bytes / 1024))
            .unwrap_or_else(|| "unknown".to_string());
        let capabilities = entry.capabilities.join(", ");
        lines.push(format!(
            "  {} v{}: {}",
            entry.name, entry.version, entry.description
        ));
        lines.push(format!(
            "    by {} | capabilities: {} | {}",
            entry.author, capabilities, size
        ));
    }

    let count = entries.len();
    let noun = if count == 1 { "skill" } else { "skills" };
    lines.push(String::new());
    lines.push(format!("{count} {noun} found."));
    lines.join("\n")
}

/// Render a list of installed skills.
fn render_installed(skills: &[InstalledSkill]) -> String {
    if skills.is_empty() {
        return "No installed skills.".to_string();
    }

    let mut lines = vec!["Installed skills:".to_string()];
    for skill in skills {
        let capabilities = if skill.capabilities.is_empty() {
            String::new()
        } else {
            format!("  ({})", skill.capabilities.join(", "))
        };
        lines.push(format!(
            "  {:16} v{}{}",
            skill.name, skill.version, capabilities
        ));
    }
    lines.join("\n")
}

pub fn search_output(query: &str) -> anyhow::Result<String> {
    let config = build_config()?;
    let results = fx_marketplace::search(&config, query)?;
    Ok(format!(
        "Registry: fawxai/registry\n\n{}",
        render_search_results(&results)
    ))
}

pub fn install_output(name: &str) -> anyhow::Result<String> {
    let config = build_config()?;
    let result = fx_marketplace::install(&config, name)?;
    Ok(format!(
        "Installing {name}...\n  Downloaded: {} KB\n  Signature: verified ✓\n  Installed to: {}",
        result.size_bytes / 1024,
        result.install_path.display()
    ))
}

pub fn list_output() -> anyhow::Result<String> {
    let data = data_dir();
    let skills = fx_marketplace::list_installed(&data)?;
    Ok(render_installed(&skills))
}
