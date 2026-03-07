//! Skill management commands.

use anyhow::{Context, Result};
use fx_author::{BuildConfig, BuildResult};
use std::fs;
use std::path::{Path, PathBuf};

const MAX_NAME_LEN: usize = 64;
const MAX_DESCRIPTION_LEN: usize = 1024;
const MAX_WASM_SIZE: usize = 10 * 1024 * 1024;
const MAX_CAPABILITIES: usize = 10;

/// Get the skills directory path.
fn get_skills_dir() -> Result<PathBuf> {
    let home = dirs::home_dir().context("Failed to get home directory")?;
    let skills_dir = home.join(".fawx").join("skills");
    fs::create_dir_all(&skills_dir)
        .with_context(|| format!("Failed to create skills directory: {:?}", skills_dir))?;
    Ok(skills_dir)
}

/// Install a skill from a WASM file and manifest.
pub async fn install(path: &str) -> Result<()> {
    let input_path = Path::new(path);
    ensure_input_exists(path, input_path)?;

    let (wasm_path, manifest_path) = resolve_skill_paths(input_path)?;
    let manifest = load_manifest(&manifest_path)?;
    validate_manifest_fields(&manifest)?;

    let wasm_bytes = fs::read(&wasm_path)
        .with_context(|| format!("Failed to read WASM file: {:?}", wasm_path))?;
    validate_wasm(&manifest, &wasm_bytes)?;

    install_skill_files(&manifest, &wasm_path, &manifest_path)?;
    Ok(())
}

fn ensure_input_exists(path: &str, input_path: &Path) -> Result<()> {
    if input_path.exists() {
        return Ok(());
    }
    anyhow::bail!("File not found: {}", path);
}

fn resolve_skill_paths(input_path: &Path) -> Result<(PathBuf, PathBuf)> {
    if input_path.is_dir() {
        return resolve_paths_from_directory(input_path);
    }

    if input_path.extension().and_then(|e| e.to_str()) == Some("wasm") {
        return resolve_paths_from_wasm(input_path);
    }

    anyhow::bail!("Expected a .wasm file or skill directory");
}

fn resolve_paths_from_directory(input_path: &Path) -> Result<(PathBuf, PathBuf)> {
    let dir_name = input_path
        .file_name()
        .and_then(|n| n.to_str())
        .context("Invalid directory name")?;

    let wasm_name = format!("{}.wasm", dir_name.replace("-skill", ""));
    let wasm = input_path.join(&wasm_name);
    let manifest = input_path.join("manifest.toml");

    ensure_file_exists(&wasm, "WASM file")?;
    ensure_file_exists(&manifest, "Manifest")?;
    Ok((wasm, manifest))
}

fn resolve_paths_from_wasm(input_path: &Path) -> Result<(PathBuf, PathBuf)> {
    let wasm = input_path.to_path_buf();
    let manifest = input_path.with_extension("toml");

    if manifest.exists() {
        return Ok((wasm, manifest));
    }

    let parent = input_path.parent().context("No parent directory")?;
    let manifest_alt = parent.join("manifest.toml");
    if manifest_alt.exists() {
        return Ok((wasm, manifest_alt));
    }

    anyhow::bail!(
        "Manifest not found. Expected at {:?} or {:?}",
        manifest,
        manifest_alt
    );
}

fn ensure_file_exists(path: &Path, label: &str) -> Result<()> {
    if path.exists() {
        return Ok(());
    }
    anyhow::bail!("{} not found: {:?}", label, path);
}

fn load_manifest(manifest_path: &Path) -> Result<fx_skills::manifest::SkillManifest> {
    let manifest_content = fs::read_to_string(manifest_path)
        .with_context(|| format!("Failed to read manifest: {:?}", manifest_path))?;

    let manifest: fx_skills::manifest::SkillManifest =
        toml::from_str(&manifest_content).context("Failed to parse manifest")?;

    fx_skills::manifest::validate_manifest(&manifest).context("Manifest validation failed")?;
    Ok(manifest)
}

fn validate_manifest_fields(manifest: &fx_skills::manifest::SkillManifest) -> Result<()> {
    if manifest.name.len() > MAX_NAME_LEN {
        anyhow::bail!("Skill name too long (max {} chars)", MAX_NAME_LEN);
    }

    if has_invalid_skill_name(&manifest.name) {
        anyhow::bail!("Invalid skill name: must not contain path separators or '..'");
    }

    if manifest.description.len() > MAX_DESCRIPTION_LEN {
        anyhow::bail!(
            "Skill description too long (max {} chars)",
            MAX_DESCRIPTION_LEN
        );
    }

    if manifest.capabilities.len() > MAX_CAPABILITIES {
        anyhow::bail!("Too many capabilities (max {})", MAX_CAPABILITIES);
    }

    Ok(())
}

fn has_invalid_skill_name(name: &str) -> bool {
    name.contains("..") || name.contains('/') || name.contains('\\')
}

fn validate_wasm(manifest: &fx_skills::manifest::SkillManifest, wasm_bytes: &[u8]) -> Result<()> {
    if wasm_bytes.len() > MAX_WASM_SIZE {
        anyhow::bail!(
            "WASM file too large: {} bytes (max {} MB)",
            wasm_bytes.len(),
            MAX_WASM_SIZE / (1024 * 1024)
        );
    }

    let loader = fx_skills::loader::SkillLoader::new(vec![]);
    loader
        .load(wasm_bytes, manifest, None)
        .context("Failed to load/validate WASM module")?;
    Ok(())
}

fn install_skill_files(
    manifest: &fx_skills::manifest::SkillManifest,
    wasm_path: &Path,
    manifest_path: &Path,
) -> Result<()> {
    let skills_dir = get_skills_dir()?;
    let skill_dir = skills_dir.join(&manifest.name);
    fs::create_dir_all(&skill_dir)
        .with_context(|| format!("Failed to create skill directory: {:?}", skill_dir))?;

    let dest_wasm = skill_dir.join(format!("{}.wasm", manifest.name));
    fs::copy(wasm_path, &dest_wasm)
        .with_context(|| format!("Failed to copy WASM to {:?}", dest_wasm))?;

    let dest_manifest = skill_dir.join("manifest.toml");
    fs::copy(manifest_path, &dest_manifest)
        .with_context(|| format!("Failed to copy manifest to {:?}", dest_manifest))?;

    print_install_summary(manifest, &skill_dir);
    Ok(())
}

fn print_install_summary(manifest: &fx_skills::manifest::SkillManifest, skill_dir: &Path) {
    println!(
        "✓ Installed skill '{}' v{}",
        manifest.name, manifest.version
    );
    println!("  Location: {:?}", skill_dir);

    if !manifest.capabilities.is_empty() {
        println!(
            "  Capabilities: {}",
            format_capabilities(&manifest.capabilities)
        );
    }
}

/// List installed skills.
pub async fn list() -> Result<()> {
    let skills_dir = get_skills_dir()?;
    let entries = list_skill_directories(&skills_dir)?;

    if entries.is_empty() {
        print_empty_skills_message();
        return Ok(());
    }

    println!("Installed skills:\n");
    for entry in entries {
        print_skill_entry(&entry.path());
    }

    Ok(())
}

fn list_skill_directories(skills_dir: &Path) -> Result<Vec<fs::DirEntry>> {
    let entries = fs::read_dir(skills_dir)
        .context("Failed to read skills directory")?
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.path().is_dir())
        .collect();
    Ok(entries)
}

fn print_empty_skills_message() {
    println!("No skills installed.");
    println!();
    println!("To install a skill:");
    println!("  fawx skill install <path-to-skill>");
}

fn print_skill_entry(skill_dir: &Path) {
    let manifest_path = skill_dir.join("manifest.toml");
    if !manifest_path.exists() {
        eprintln!("⚠ Skipping {:?}: manifest.toml not found", skill_dir);
        return;
    }

    match read_manifest_for_listing(&manifest_path) {
        Ok(manifest) => print_manifest_for_listing(&manifest),
        Err(error) => eprintln!("⚠ {}", error),
    }
}

fn read_manifest_for_listing(
    manifest_path: &Path,
) -> Result<fx_skills::manifest::SkillManifest, String> {
    let manifest_content = fs::read_to_string(manifest_path)
        .map_err(|error| format!("Failed to read manifest at {:?}: {}", manifest_path, error))?;

    toml::from_str(&manifest_content)
        .map_err(|error| format!("Failed to parse manifest at {:?}: {}", manifest_path, error))
}

fn print_manifest_for_listing(manifest: &fx_skills::manifest::SkillManifest) {
    println!("  {} v{}", manifest.name, manifest.version);
    println!("    {}", manifest.description);

    if !manifest.capabilities.is_empty() {
        println!(
            "    Capabilities: {}",
            format_capabilities(&manifest.capabilities)
        );
    }

    println!();
}

fn format_capabilities(capabilities: &[fx_skills::manifest::Capability]) -> String {
    capabilities
        .iter()
        .map(|capability| capability.to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

/// Remove an installed skill.
pub async fn remove(name: &str) -> Result<()> {
    if has_invalid_skill_name(name) {
        anyhow::bail!("Invalid skill name: must not contain path separators or '..'");
    }

    let skills_dir = get_skills_dir()?;
    let skill_dir = skills_dir.join(name);

    if !skill_dir.exists() {
        anyhow::bail!("Skill '{}' is not installed", name);
    }

    fs::remove_dir_all(&skill_dir)
        .with_context(|| format!("Failed to remove skill directory: {:?}", skill_dir))?;

    println!("✓ Removed skill '{}'", name);
    Ok(())
}

/// Build a skill from source.
pub fn build(path: &str, no_sign: bool, no_install: bool) -> Result<()> {
    let project_path = PathBuf::from(path)
        .canonicalize()
        .with_context(|| format!("Invalid project path: {path}"))?;

    let data_dir = resolve_data_dir()?;

    let config = BuildConfig {
        project_path,
        data_dir,
        no_sign,
        no_install,
    };

    let result = fx_author::build_skill(&config).map_err(|e| anyhow::anyhow!("{e}"))?;
    print_build_summary(&result);
    Ok(())
}

/// Scaffold a new skill project.
pub fn scaffold(name: &str) -> Result<()> {
    let cwd = std::env::current_dir().context("Failed to get current directory")?;
    let path = fx_author::scaffold_skill(name, &cwd).map_err(|e| anyhow::anyhow!("{e}"))?;
    println!("✓ Created skill project at {}", path.display());
    println!("  Next steps:");
    println!("    cd {name}");
    println!("    # edit src/lib.rs");
    println!("    fawx skill build .");
    Ok(())
}

fn resolve_data_dir() -> Result<PathBuf> {
    let home = dirs::home_dir().context("Failed to get home directory")?;
    Ok(home.join(".fawx"))
}

fn print_build_summary(result: &BuildResult) {
    let size_kb = result.wasm_size_bytes / 1024;
    let signed_str = if result.signed { "signed" } else { "unsigned" };

    println!(
        "✓ Built {} v{} ({} KB, {})",
        result.skill_name, result.version, size_kb, signed_str
    );

    if let Some(ref path) = result.install_path {
        println!("  Installed to: {}", path.display());
    }
}
