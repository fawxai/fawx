//! Skill management commands.

use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

/// Get the skills directory path.
fn get_skills_dir() -> Result<PathBuf> {
    let home = dirs::home_dir().context("Failed to get home directory")?;
    let skills_dir = home.join(".nova").join("skills");

    // Create the directory if it doesn't exist
    fs::create_dir_all(&skills_dir)
        .with_context(|| format!("Failed to create skills directory: {:?}", skills_dir))?;

    Ok(skills_dir)
}

/// Install a skill from a WASM file and manifest.
pub async fn install(path: &str) -> Result<()> {
    let input_path = Path::new(path);

    // Validate input path
    if !input_path.exists() {
        anyhow::bail!("File not found: {}", path);
    }

    // Determine WASM and manifest paths
    let (wasm_path, manifest_path) = if input_path.is_dir() {
        // Directory: look for WASM and manifest files
        let dir_name = input_path
            .file_name()
            .and_then(|n| n.to_str())
            .context("Invalid directory name")?;

        let wasm_name = format!("{}.wasm", dir_name.replace("-skill", ""));
        let wasm = input_path.join(&wasm_name);
        let manifest = input_path.join("manifest.toml");

        if !wasm.exists() {
            anyhow::bail!("WASM file not found: {:?}", wasm);
        }
        if !manifest.exists() {
            anyhow::bail!("Manifest not found: {:?}", manifest);
        }

        (wasm, manifest)
    } else if input_path.extension().and_then(|e| e.to_str()) == Some("wasm") {
        // WASM file: look for adjacent manifest
        let wasm = input_path.to_path_buf();
        let manifest = input_path.with_extension("toml");

        if !manifest.exists() {
            let parent = input_path.parent().context("No parent directory")?;
            let manifest_alt = parent.join("manifest.toml");
            if manifest_alt.exists() {
                (wasm, manifest_alt)
            } else {
                anyhow::bail!(
                    "Manifest not found. Expected at {:?} or {:?}",
                    manifest,
                    manifest_alt
                );
            }
        } else {
            (wasm, manifest)
        }
    } else {
        anyhow::bail!("Expected a .wasm file or skill directory");
    };

    // Load and parse manifest
    let manifest_content = fs::read_to_string(&manifest_path)
        .with_context(|| format!("Failed to read manifest: {:?}", manifest_path))?;

    let manifest: nv_skills::manifest::SkillManifest =
        toml::from_str(&manifest_content).context("Failed to parse manifest")?;

    // Validate manifest
    nv_skills::manifest::validate_manifest(&manifest).context("Manifest validation failed")?;

    // Validate manifest fields
    const MAX_NAME_LEN: usize = 64;
    const MAX_DESCRIPTION_LEN: usize = 1024;
    const MAX_WASM_SIZE: usize = 10 * 1024 * 1024; // 10 MB
    const MAX_CAPABILITIES: usize = 10;

    if manifest.name.len() > MAX_NAME_LEN {
        anyhow::bail!("Skill name too long (max {} chars)", MAX_NAME_LEN);
    }
    if manifest.name.contains("..") || manifest.name.contains('/') || manifest.name.contains('\\') {
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

    // Load WASM bytes
    let wasm_bytes = fs::read(&wasm_path)
        .with_context(|| format!("Failed to read WASM file: {:?}", wasm_path))?;

    if wasm_bytes.len() > MAX_WASM_SIZE {
        anyhow::bail!(
            "WASM file too large: {} bytes (max {} MB)",
            wasm_bytes.len(),
            MAX_WASM_SIZE / (1024 * 1024)
        );
    }

    // Validate WASM by trying to compile it
    let loader = nv_skills::loader::SkillLoader::new(vec![]);
    loader
        .load(&wasm_bytes, &manifest, None)
        .context("Failed to load/validate WASM module")?;

    // Install to skills directory
    let skills_dir = get_skills_dir()?;
    let skill_dir = skills_dir.join(&manifest.name);

    // Create skill directory
    fs::create_dir_all(&skill_dir)
        .with_context(|| format!("Failed to create skill directory: {:?}", skill_dir))?;

    // Copy WASM file
    let dest_wasm = skill_dir.join(format!("{}.wasm", manifest.name));
    fs::copy(&wasm_path, &dest_wasm)
        .with_context(|| format!("Failed to copy WASM to {:?}", dest_wasm))?;

    // Copy manifest
    let dest_manifest = skill_dir.join("manifest.toml");
    fs::copy(&manifest_path, &dest_manifest)
        .with_context(|| format!("Failed to copy manifest to {:?}", dest_manifest))?;

    println!(
        "✓ Installed skill '{}' v{}",
        manifest.name, manifest.version
    );
    println!("  Location: {:?}", skill_dir);

    if !manifest.capabilities.is_empty() {
        println!(
            "  Capabilities: {}",
            manifest
                .capabilities
                .iter()
                .map(|c| c.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        );
    }

    Ok(())
}

/// List installed skills.
pub async fn list() -> Result<()> {
    let skills_dir = get_skills_dir()?;

    // Check if directory is empty
    let entries: Vec<_> = fs::read_dir(&skills_dir)
        .context("Failed to read skills directory")?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .collect();

    if entries.is_empty() {
        println!("No skills installed.");
        println!();
        println!("To install a skill:");
        println!("  nova skill install <path-to-skill>");
        return Ok(());
    }

    println!("Installed skills:\n");

    for entry in entries {
        let skill_dir = entry.path();
        let manifest_path = skill_dir.join("manifest.toml");

        if !manifest_path.exists() {
            eprintln!("⚠ Skipping {:?}: manifest.toml not found", skill_dir);
            continue;
        }

        // Read and parse manifest
        let manifest_content = match fs::read_to_string(&manifest_path) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("⚠ Failed to read manifest at {:?}: {}", manifest_path, e);
                continue;
            }
        };

        let manifest: nv_skills::manifest::SkillManifest = match toml::from_str(&manifest_content) {
            Ok(m) => m,
            Err(e) => {
                eprintln!("⚠ Failed to parse manifest at {:?}: {}", manifest_path, e);
                continue;
            }
        };

        // Print skill info
        println!("  {} v{}", manifest.name, manifest.version);
        println!("    {}", manifest.description);

        if !manifest.capabilities.is_empty() {
            println!(
                "    Capabilities: {}",
                manifest
                    .capabilities
                    .iter()
                    .map(|c| c.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }

        println!();
    }

    Ok(())
}

/// Remove an installed skill.
pub async fn remove(name: &str) -> Result<()> {
    // Validate name to prevent path traversal attacks
    if name.contains("..") || name.contains('/') || name.contains('\\') {
        anyhow::bail!("Invalid skill name: must not contain path separators or '..'");
    }

    let skills_dir = get_skills_dir()?;
    let skill_dir = skills_dir.join(name);

    if !skill_dir.exists() {
        anyhow::bail!("Skill '{}' is not installed", name);
    }

    // Remove the skill directory
    fs::remove_dir_all(&skill_dir)
        .with_context(|| format!("Failed to remove skill directory: {:?}", skill_dir))?;

    println!("✓ Removed skill '{}'", name);

    Ok(())
}
