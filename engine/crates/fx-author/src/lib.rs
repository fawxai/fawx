//! Skill authoring pipeline: build, sign, install, and scaffold WASM skills.

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use fx_skills::manifest::{parse_manifest, validate_manifest};
use fx_skills::signing::sign_skill;

// ── Error type ──────────────────────────────────────────────────────────────

/// Errors that can occur during skill authoring.
#[derive(Debug)]
pub enum AuthorError {
    ManifestNotFound(PathBuf),
    ManifestInvalid(String),
    CargoTomlNotFound(PathBuf),
    CargoTomlInvalid(String),
    BuildFailed(String),
    InstallFailed(String),
    SigningFailed(String),
}

impl fmt::Display for AuthorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ManifestNotFound(p) => write!(f, "manifest.toml not found: {}", p.display()),
            Self::ManifestInvalid(msg) => write!(f, "Invalid manifest: {msg}"),
            Self::CargoTomlNotFound(p) => write!(f, "Cargo.toml not found: {}", p.display()),
            Self::CargoTomlInvalid(msg) => write!(f, "Invalid Cargo.toml: {msg}"),
            Self::BuildFailed(msg) => write!(f, "Build failed: {msg}"),
            Self::InstallFailed(msg) => write!(f, "Install failed: {msg}"),
            Self::SigningFailed(msg) => write!(f, "Signing failed: {msg}"),
        }
    }
}

impl std::error::Error for AuthorError {}

// ── Public types ────────────────────────────────────────────────────────────

/// Configuration for building a skill.
pub struct BuildConfig {
    /// Path to skill project (contains Cargo.toml + manifest.toml).
    pub project_path: PathBuf,
    /// Fawx data directory (~/.fawx).
    pub data_dir: PathBuf,
    /// Skip signing even if key exists.
    pub no_sign: bool,
    /// Skip install (build only).
    pub no_install: bool,
}

/// Result of a successful skill build.
#[derive(Debug)]
pub struct BuildResult {
    pub skill_name: String,
    pub version: String,
    pub wasm_size_bytes: u64,
    pub signed: bool,
    pub install_path: Option<PathBuf>,
}

// ── Build pipeline ──────────────────────────────────────────────────────────

/// Build a WASM skill from source.
///
/// Steps: validate manifest → validate Cargo.toml → cargo build → install → sign.
pub fn build_skill(config: &BuildConfig) -> Result<BuildResult, AuthorError> {
    let manifest = read_and_validate_manifest(&config.project_path)?;
    let crate_name = read_and_validate_cargo_toml(&config.project_path)?;

    run_cargo_build(&config.project_path)?;

    let wasm_path = locate_wasm_output(&config.project_path, &crate_name)?;
    let wasm_size = fs::metadata(&wasm_path)
        .map_err(|e| AuthorError::BuildFailed(format!("Cannot stat WASM output: {e}")))?
        .len();

    if config.no_install {
        return Ok(BuildResult {
            skill_name: manifest.name,
            version: manifest.version,
            wasm_size_bytes: wasm_size,
            signed: false,
            install_path: None,
        });
    }

    let install_dir = install_skill_files(config, &manifest.name, &wasm_path)?;
    let signed = maybe_sign(config, &install_dir, &manifest.name)?;

    Ok(BuildResult {
        skill_name: manifest.name,
        version: manifest.version,
        wasm_size_bytes: wasm_size,
        signed,
        install_path: Some(install_dir),
    })
}

// ── Scaffold ────────────────────────────────────────────────────────────────

/// Scaffold a new skill project.
///
/// Creates `<parent_dir>/<name>/` with Cargo.toml, manifest.toml, and src/lib.rs.
pub fn scaffold_skill(name: &str, parent_dir: &Path) -> Result<PathBuf, AuthorError> {
    let project_dir = parent_dir.join(name);
    if project_dir.exists() {
        return Err(AuthorError::InstallFailed(format!(
            "Directory already exists: {}",
            project_dir.display()
        )));
    }

    let src_dir = project_dir.join("src");
    fs::create_dir_all(&src_dir)
        .map_err(|e| AuthorError::InstallFailed(format!("Failed to create directory: {e}")))?;

    write_scaffold_file(&project_dir.join("Cargo.toml"), &scaffold_cargo_toml(name))?;
    write_scaffold_file(
        &project_dir.join("manifest.toml"),
        &scaffold_manifest_toml(name),
    )?;
    write_scaffold_file(&src_dir.join("lib.rs"), &scaffold_lib_rs(name))?;

    Ok(project_dir)
}

// ── Internal helpers ────────────────────────────────────────────────────────

fn read_and_validate_manifest(
    project_path: &Path,
) -> Result<fx_skills::manifest::SkillManifest, AuthorError> {
    let manifest_path = project_path.join("manifest.toml");
    if !manifest_path.exists() {
        return Err(AuthorError::ManifestNotFound(manifest_path));
    }

    let content = fs::read_to_string(&manifest_path)
        .map_err(|e| AuthorError::ManifestInvalid(format!("Failed to read manifest: {e}")))?;

    let manifest =
        parse_manifest(&content).map_err(|e| AuthorError::ManifestInvalid(e.to_string()))?;

    validate_manifest(&manifest).map_err(|e| AuthorError::ManifestInvalid(e.to_string()))?;

    Ok(manifest)
}

fn read_and_validate_cargo_toml(project_path: &Path) -> Result<String, AuthorError> {
    let cargo_path = project_path.join("Cargo.toml");
    if !cargo_path.exists() {
        return Err(AuthorError::CargoTomlNotFound(cargo_path));
    }

    let content = fs::read_to_string(&cargo_path)
        .map_err(|e| AuthorError::CargoTomlInvalid(format!("Failed to read: {e}")))?;

    let parsed: toml::Value =
        toml::from_str(&content).map_err(|e| AuthorError::CargoTomlInvalid(e.to_string()))?;

    let crate_name = extract_crate_name(&parsed)?;
    verify_cdylib_crate_type(&parsed)?;

    Ok(crate_name)
}

fn extract_crate_name(cargo_toml: &toml::Value) -> Result<String, AuthorError> {
    cargo_toml
        .get("package")
        .and_then(|p| p.get("name"))
        .and_then(|n| n.as_str())
        .map(String::from)
        .ok_or_else(|| AuthorError::CargoTomlInvalid("Missing [package] name field".to_string()))
}

fn verify_cdylib_crate_type(cargo_toml: &toml::Value) -> Result<(), AuthorError> {
    let crate_types = cargo_toml
        .get("lib")
        .and_then(|lib| lib.get("crate-type"))
        .and_then(|ct| ct.as_array());

    let has_cdylib =
        crate_types.is_some_and(|types| types.iter().any(|t| t.as_str() == Some("cdylib")));

    if !has_cdylib {
        return Err(AuthorError::CargoTomlInvalid(
            "Missing crate-type = [\"cdylib\"] in [lib] section. \
             Skills must be compiled as cdylib."
                .to_string(),
        ));
    }

    Ok(())
}

fn run_cargo_build(project_path: &Path) -> Result<(), AuthorError> {
    check_wasm_target()?;

    let output = Command::new("cargo")
        .args(["build", "--target", "wasm32-wasip1", "--release"])
        .current_dir(project_path)
        .output()
        .map_err(|e| AuthorError::BuildFailed(format!("Failed to execute cargo: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(AuthorError::BuildFailed(stderr.to_string()));
    }

    Ok(())
}

fn check_wasm_target() -> Result<(), AuthorError> {
    let output = Command::new("rustup")
        .args(["target", "list", "--installed"])
        .output()
        .map_err(|e| AuthorError::BuildFailed(format!("Failed to run rustup: {e}")))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    if !stdout.lines().any(|line| line.trim() == "wasm32-wasip1") {
        return Err(AuthorError::BuildFailed(
            "Missing wasm32-wasip1 target. Run: rustup target add wasm32-wasip1".to_string(),
        ));
    }

    Ok(())
}

fn locate_wasm_output(project_path: &Path, crate_name: &str) -> Result<PathBuf, AuthorError> {
    let wasm_name = format!("{}.wasm", crate_name.replace('-', "_"));
    let wasm_path = project_path
        .join("target")
        .join("wasm32-wasip1")
        .join("release")
        .join(&wasm_name);

    if !wasm_path.exists() {
        return Err(AuthorError::BuildFailed(format!(
            "Expected WASM output not found: {}",
            wasm_path.display()
        )));
    }

    Ok(wasm_path)
}

fn install_skill_files(
    config: &BuildConfig,
    skill_name: &str,
    wasm_path: &Path,
) -> Result<PathBuf, AuthorError> {
    let install_dir = config.data_dir.join("skills").join(skill_name);
    fs::create_dir_all(&install_dir)
        .map_err(|e| AuthorError::InstallFailed(format!("Failed to create install dir: {e}")))?;

    let dest_wasm = install_dir.join(format!("{skill_name}.wasm"));
    fs::copy(wasm_path, &dest_wasm)
        .map_err(|e| AuthorError::InstallFailed(format!("Failed to copy WASM: {e}")))?;

    let src_manifest = config.project_path.join("manifest.toml");
    let dest_manifest = install_dir.join("manifest.toml");
    fs::copy(&src_manifest, &dest_manifest)
        .map_err(|e| AuthorError::InstallFailed(format!("Failed to copy manifest: {e}")))?;

    Ok(install_dir)
}

fn maybe_sign(
    config: &BuildConfig,
    install_dir: &Path,
    skill_name: &str,
) -> Result<bool, AuthorError> {
    if config.no_sign {
        return Ok(false);
    }

    let key_path = config.data_dir.join("keys").join("signing_key.pem");
    if !key_path.exists() {
        eprintln!(
            "⚠ Signing key not found at {}; skipping signing.",
            key_path.display()
        );
        return Ok(false);
    }

    let key_bytes = fs::read(&key_path)
        .map_err(|e| AuthorError::SigningFailed(format!("Failed to read signing key: {e}")))?;

    let wasm_path = install_dir.join(format!("{skill_name}.wasm"));
    let wasm_bytes = fs::read(&wasm_path)
        .map_err(|e| AuthorError::SigningFailed(format!("Failed to read WASM for signing: {e}")))?;

    let signature = sign_skill(&wasm_bytes, &key_bytes)
        .map_err(|e| AuthorError::SigningFailed(e.to_string()))?;

    let sig_path = install_dir.join(format!("{skill_name}.wasm.sig"));
    fs::write(&sig_path, &signature)
        .map_err(|e| AuthorError::SigningFailed(format!("Failed to write signature: {e}")))?;

    Ok(true)
}

fn write_scaffold_file(path: &Path, content: &str) -> Result<(), AuthorError> {
    fs::write(path, content)
        .map_err(|e| AuthorError::InstallFailed(format!("Failed to write {}: {e}", path.display())))
}

// ── Scaffold templates ──────────────────────────────────────────────────────

fn scaffold_cargo_toml(name: &str) -> String {
    format!(
        r#"[package]
name = "{name}"
version = "0.1.0"
edition = "2021"

[workspace]

[lib]
crate-type = ["cdylib", "lib"]

[dependencies]
serde = {{ version = "1.0", features = ["derive"] }}
serde_json = "1.0"

[profile.release]
opt-level = "z"
lto = true
strip = true
"#
    )
}

fn scaffold_manifest_toml(name: &str) -> String {
    format!(
        r#"name = "{name}"
version = "0.1.0"
description = "A Fawx skill"
author = "You"
api_version = "host_api_v1"
capabilities = []
entry_point = "run"
"#
    )
}

fn scaffold_lib_rs(name: &str) -> String {
    format!(
        r#"use serde_json::{{json, Value}};

#[no_mangle]
pub extern "C" fn run() {{
    let input = read_input();
    let args: Value = serde_json::from_str(&input).unwrap_or(json!({{}}));
    let action = args["action"].as_str().unwrap_or("hello");

    let result = match action {{
        "hello" => json!({{
            "message": "Hello from {name}!"
        }}),
        _ => json!({{
            "error": format!("Unknown action: {{}}", action)
        }}),
    }};

    write_output(&serde_json::to_string(&result).unwrap());
}}

fn read_input() -> String {{
    use std::io::Read;
    let mut buf = String::new();
    unsafe {{
        use std::os::fd::FromRawFd;
        let mut f = std::fs::File::from_raw_fd(3);
        f.read_to_string(&mut buf).ok();
        std::mem::forget(f);
    }}
    buf
}}

fn write_output(s: &str) {{
    use std::io::Write;
    unsafe {{
        use std::os::fd::FromRawFd;
        let mut f = std::fs::File::from_raw_fd(4);
        f.write_all(s.as_bytes()).ok();
        std::mem::forget(f);
    }}
}}
"#
    )
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn valid_manifest_toml() -> &'static str {
        r#"
name = "test-skill"
version = "0.1.0"
description = "A test skill"
author = "Tester"
api_version = "host_api_v1"
entry_point = "run"
"#
    }

    fn cargo_toml_without_cdylib() -> &'static str {
        r#"
[package]
name = "test-skill"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["lib"]
"#
    }

    // 1. build_validates_manifest
    #[test]
    fn build_validates_manifest() {
        let tmp = TempDir::new().unwrap();
        let config = BuildConfig {
            project_path: tmp.path().to_path_buf(),
            data_dir: tmp.path().join("data"),
            no_sign: true,
            no_install: true,
        };

        // No manifest at all → ManifestNotFound
        let err = build_skill(&config).unwrap_err();
        assert!(matches!(err, AuthorError::ManifestNotFound(_)));

        // Invalid manifest content → ManifestInvalid
        fs::write(tmp.path().join("manifest.toml"), "invalid { toml").unwrap();
        let err = build_skill(&config).unwrap_err();
        assert!(matches!(err, AuthorError::ManifestInvalid(_)));
    }

    // 2. build_validates_cargo_toml
    #[test]
    fn build_validates_cargo_toml() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("manifest.toml"), valid_manifest_toml()).unwrap();

        let config = BuildConfig {
            project_path: tmp.path().to_path_buf(),
            data_dir: tmp.path().join("data"),
            no_sign: true,
            no_install: true,
        };

        let err = build_skill(&config).unwrap_err();
        assert!(matches!(err, AuthorError::CargoTomlNotFound(_)));
    }

    // 3. build_detects_missing_cdylib
    #[test]
    fn build_detects_missing_cdylib() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("manifest.toml"), valid_manifest_toml()).unwrap();
        fs::write(tmp.path().join("Cargo.toml"), cargo_toml_without_cdylib()).unwrap();

        let config = BuildConfig {
            project_path: tmp.path().to_path_buf(),
            data_dir: tmp.path().join("data"),
            no_sign: true,
            no_install: true,
        };

        let err = build_skill(&config).unwrap_err();
        assert!(matches!(err, AuthorError::CargoTomlInvalid(_)));
        assert!(err.to_string().contains("cdylib"));
    }

    // 4. scaffold_creates_project_structure
    #[test]
    fn scaffold_creates_project_structure() {
        let tmp = TempDir::new().unwrap();
        let result = scaffold_skill("my-skill", tmp.path()).unwrap();

        assert_eq!(result, tmp.path().join("my-skill"));
        assert!(result.join("Cargo.toml").exists());
        assert!(result.join("manifest.toml").exists());
        assert!(result.join("src").join("lib.rs").exists());

        // Verify content has the skill name
        let cargo = fs::read_to_string(result.join("Cargo.toml")).unwrap();
        assert!(cargo.contains("name = \"my-skill\""));
        assert!(cargo.contains("cdylib"));

        let manifest = fs::read_to_string(result.join("manifest.toml")).unwrap();
        assert!(manifest.contains("name = \"my-skill\""));
        assert!(manifest.contains("host_api_v1"));

        let lib = fs::read_to_string(result.join("src/lib.rs")).unwrap();
        assert!(lib.contains("my-skill"));
    }

    // 5. scaffold_refuses_existing_directory
    #[test]
    fn scaffold_refuses_existing_directory() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir(tmp.path().join("existing")).unwrap();

        let err = scaffold_skill("existing", tmp.path()).unwrap_err();
        assert!(err.to_string().contains("already exists"));
    }

    // 6. install_copies_wasm_and_manifest
    #[test]
    fn install_copies_wasm_and_manifest() {
        let tmp = TempDir::new().unwrap();
        let project = tmp.path().join("project");
        let data = tmp.path().join("data");
        fs::create_dir_all(&project).unwrap();

        fs::write(project.join("manifest.toml"), valid_manifest_toml()).unwrap();

        let mock_wasm = vec![0x00, 0x61, 0x73, 0x6D]; // WASM magic bytes
        fs::write(project.join("mock.wasm"), &mock_wasm).unwrap();

        let config = BuildConfig {
            project_path: project.clone(),
            data_dir: data.clone(),
            no_sign: true,
            no_install: false,
        };

        let install_path =
            install_skill_files(&config, "test-skill", &project.join("mock.wasm")).unwrap();

        assert!(install_path.join("test-skill.wasm").exists());
        assert!(install_path.join("manifest.toml").exists());

        let installed_wasm = fs::read(install_path.join("test-skill.wasm")).unwrap();
        assert_eq!(installed_wasm, mock_wasm);
    }

    // 7. install_creates_signature_when_key_exists
    #[test]
    fn install_creates_signature_when_key_exists() {
        let tmp = TempDir::new().unwrap();
        let project = tmp.path().join("project");
        let data = tmp.path().join("data");
        fs::create_dir_all(&project).unwrap();
        fs::create_dir_all(data.join("keys")).unwrap();

        // Generate a real keypair for testing
        let (private_key, _) = fx_skills::signing::generate_keypair().unwrap();
        fs::write(data.join("keys").join("signing_key.pem"), &private_key).unwrap();

        // Create mock wasm in install dir
        let install_dir = data.join("skills").join("test-skill");
        fs::create_dir_all(&install_dir).unwrap();
        fs::write(install_dir.join("test-skill.wasm"), b"fake wasm").unwrap();

        // Write manifest in project dir (needed by install_skill_files)
        fs::write(project.join("manifest.toml"), valid_manifest_toml()).unwrap();

        let config = BuildConfig {
            project_path: project,
            data_dir: data,
            no_sign: false,
            no_install: false,
        };

        let signed = maybe_sign(&config, &install_dir, "test-skill").unwrap();
        assert!(signed);
        assert!(install_dir.join("test-skill.wasm.sig").exists());

        let sig = fs::read(install_dir.join("test-skill.wasm.sig")).unwrap();
        assert!(!sig.is_empty());
    }

    // 8. install_skips_signature_with_no_sign
    #[test]
    fn install_skips_signature_with_no_sign() {
        let tmp = TempDir::new().unwrap();
        let data = tmp.path().join("data");
        fs::create_dir_all(data.join("keys")).unwrap();

        // Even with key present, no_sign should skip
        let (private_key, _) = fx_skills::signing::generate_keypair().unwrap();
        fs::write(data.join("keys").join("signing_key.pem"), &private_key).unwrap();

        let install_dir = data.join("skills").join("test-skill");
        fs::create_dir_all(&install_dir).unwrap();
        fs::write(install_dir.join("test-skill.wasm"), b"fake wasm").unwrap();

        let config = BuildConfig {
            project_path: tmp.path().to_path_buf(),
            data_dir: data,
            no_sign: true,
            no_install: false,
        };

        let signed = maybe_sign(&config, &install_dir, "test-skill").unwrap();
        assert!(!signed);
        assert!(!install_dir.join("test-skill.wasm.sig").exists());
    }
}
