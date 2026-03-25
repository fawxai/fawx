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

/// Print a list of skill entries from search results.
fn print_search_results(entries: &[SkillEntry]) {
    if entries.is_empty() {
        println!("No skills found.");
        return;
    }
    for e in entries {
        let size = e
            .size_bytes
            .map(|b| format!("{} KB", b / 1024))
            .unwrap_or_else(|| "unknown".to_string());
        let caps = e.capabilities.join(", ");
        println!("  {} v{} — {}", e.name, e.version, e.description);
        println!("    by {} | capabilities: {} | {}", e.author, caps, size);
    }
    let n = entries.len();
    let noun = if n == 1 { "skill" } else { "skills" };
    println!("\n{n} {noun} found.");
}

/// Print a list of installed skills.
fn print_installed(skills: &[InstalledSkill]) {
    if skills.is_empty() {
        println!("No installed skills.");
        return;
    }
    println!("Installed skills:");
    for s in skills {
        let caps = if s.capabilities.is_empty() {
            String::new()
        } else {
            format!("  ({})", s.capabilities.join(", "))
        };
        println!("  {:16} v{}{}", s.name, s.version, caps);
    }
}

/// `fawx search <query>`
pub fn search_cmd(query: &str) -> anyhow::Result<()> {
    let config = build_config()?;
    println!("Registry: fawxai/registry\n");
    let results = fx_marketplace::search(&config, query)?;
    print_search_results(&results);
    Ok(())
}

/// `fawx install <name>`
pub fn install_cmd(name: &str) -> anyhow::Result<()> {
    let config = build_config()?;
    println!("Installing {name}...");
    let result = fx_marketplace::install(&config, name)?;
    println!("  Downloaded: {} KB", result.size_bytes / 1024);
    println!("  Signature: verified ✓");
    println!("  Installed to: {}", result.install_path.display());
    Ok(())
}

/// `fawx list`
pub fn list_cmd() -> anyhow::Result<()> {
    let data = data_dir();
    let skills = fx_marketplace::list_installed(&data)?;
    print_installed(&skills);
    Ok(())
}
