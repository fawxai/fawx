//! CLI commands for the skill marketplace (search, install, list).

use std::path::{Path, PathBuf};

use crate::startup;
use fx_loadable::{write_source_metadata, SkillSource};
use fx_marketplace::{InstalledSkill, SkillEntry};

fn resolved_data_dir(data_dir: Option<&Path>) -> PathBuf {
    data_dir
        .map(Path::to_path_buf)
        .unwrap_or_else(startup::fawx_data_dir)
}

/// Build a `RegistryConfig` from defaults.
fn build_config(data_dir: Option<&Path>) -> anyhow::Result<fx_marketplace::RegistryConfig> {
    let data = resolved_data_dir(data_dir);
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
    let config = build_config(None)?;
    let results = fx_marketplace::search(&config, query)?;
    Ok(format!(
        "Registry: fawxai/registry\n\n{}",
        render_search_results(&results)
    ))
}

pub fn install_output(name: &str, data_dir: Option<&Path>) -> anyhow::Result<String> {
    #[cfg(test)]
    if let Some(output) = take_test_install_output(name, data_dir) {
        return Ok(output);
    }

    let config = build_config(data_dir)?;
    let result = fx_marketplace::install(&config, name)?;
    let publisher = fx_marketplace::search(&config, name)?
        .into_iter()
        .find(|entry| entry.name == result.name)
        .map(|entry| entry.author)
        .unwrap_or_else(|| "unknown".to_string());
    let source = SkillSource::Published {
        publisher,
        registry_url: config.registry_url.clone(),
    };
    write_source_metadata(&result.install_path, &source).map_err(anyhow::Error::msg)?;
    Ok(format!(
        "Installing {name}...\n  Downloaded: {} KB\n  Signature: verified ✓\n  Installed to: {}",
        result.size_bytes / 1024,
        result.install_path.display()
    ))
}

pub fn list_output() -> anyhow::Result<String> {
    let data = resolved_data_dir(None);
    let skills = fx_marketplace::list_installed(&data)?;
    Ok(render_installed(&skills))
}

#[cfg(test)]
#[derive(Default)]
struct TestInstallState {
    next_output: Option<String>,
    last_request: Option<(String, Option<PathBuf>)>,
}

#[cfg(test)]
fn test_install_state() -> &'static std::sync::Mutex<TestInstallState> {
    static STATE: std::sync::OnceLock<std::sync::Mutex<TestInstallState>> =
        std::sync::OnceLock::new();
    STATE.get_or_init(|| std::sync::Mutex::new(TestInstallState::default()))
}

#[cfg(test)]
fn take_test_install_output(name: &str, data_dir: Option<&Path>) -> Option<String> {
    let mut state = test_install_state()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let output = state.next_output.take()?;
    state.last_request = Some((name.to_string(), data_dir.map(Path::to_path_buf)));
    Some(output)
}

#[cfg(test)]
#[allow(dead_code)]
pub(crate) fn set_test_install_output(output: Option<String>) {
    let mut state = test_install_state()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    state.next_output = output;
    state.last_request = None;
}

#[cfg(test)]
#[allow(dead_code)]
pub(crate) fn take_last_install_request() -> Option<(String, Option<PathBuf>)> {
    test_install_state()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .last_request
        .take()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_config_uses_explicit_data_dir() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let config = build_config(Some(tmp.path())).expect("config");

        assert_eq!(config.data_dir, tmp.path());
    }
}
