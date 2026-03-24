//! CLI commands for the skill marketplace (search, install, list).

use std::path::{Path, PathBuf};

use fx_marketplace::{InstalledSkill, RegistryConfig, SkillEntry};

/// Default registry URL (raw GitHub content).
const DEFAULT_REGISTRY: &str = "https://raw.githubusercontent.com/fawxai/registry/main";

/// Official fawxai publisher Ed25519 public key (32 bytes).
const FAWXAI_PUBLIC_KEY: [u8; 32] = [
    62, 38, 70, 230, 12, 59, 226, 179, 11, 150, 52, 48, 238, 181, 159, 188, 106, 55, 109, 208, 1,
    191, 157, 233, 161, 111, 154, 212, 209, 133, 28, 68,
];

/// Resolve the Fawx data directory (`~/.fawx`).
fn data_dir() -> anyhow::Result<PathBuf> {
    let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("cannot determine home dir"))?;
    Ok(home.join(".fawx"))
}

/// Load trusted keys from `~/.fawx/trusted_keys/`.
fn load_trusted_keys(data: &Path) -> anyhow::Result<Vec<Vec<u8>>> {
    let mut keys = vec![FAWXAI_PUBLIC_KEY.to_vec()];
    let keys_dir = data.join("trusted_keys");
    if keys_dir.exists() {
        for entry in std::fs::read_dir(&keys_dir)? {
            let path = entry?.path();
            if path.is_file() {
                keys.push(std::fs::read(&path)?);
            }
        }
    }
    Ok(keys)
}

/// Build a `RegistryConfig` from defaults.
fn build_config() -> anyhow::Result<RegistryConfig> {
    let data = data_dir()?;
    let trusted_keys = load_trusted_keys(&data)?;
    Ok(RegistryConfig {
        registry_url: DEFAULT_REGISTRY.to_string(),
        data_dir: data,
        trusted_keys,
    })
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
    println!("Registry: fawxai/fawx-skills\n");
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
    let data = data_dir()?;
    let skills = fx_marketplace::list_installed(&data)?;
    print_installed(&skills);
    Ok(())
}
