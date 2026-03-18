//! Skill registry client for the Fawx agentic engine.
//!
//! Provides search, install, and list functionality against a GitHub-based
//! skill registry. Skills are fetched over HTTPS and their Ed25519 signatures
//! are verified before installation.

use std::fmt;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Error types
// ---------------------------------------------------------------------------

/// Errors returned by marketplace operations.
#[derive(Debug)]
pub enum MarketplaceError {
    /// Failed to fetch from registry.
    NetworkError(String),
    /// Skill not found in registry.
    SkillNotFound(String),
    /// Index format invalid.
    InvalidIndex(String),
    /// Signature verification failed.
    SignatureInvalid(String),
    /// Manifest validation failed.
    ManifestInvalid(String),
    /// Install I/O error.
    InstallError(String),
    /// Registry URL not HTTPS.
    InsecureRegistry(String),
}

impl fmt::Display for MarketplaceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NetworkError(msg) => write!(f, "network error: {msg}"),
            Self::SkillNotFound(msg) => write!(f, "skill not found: {msg}"),
            Self::InvalidIndex(msg) => write!(f, "invalid index: {msg}"),
            Self::SignatureInvalid(msg) => write!(f, "signature invalid: {msg}"),
            Self::ManifestInvalid(msg) => write!(f, "manifest invalid: {msg}"),
            Self::InstallError(msg) => write!(f, "install error: {msg}"),
            Self::InsecureRegistry(msg) => write!(f, "insecure registry: {msg}"),
        }
    }
}

impl std::error::Error for MarketplaceError {}

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Registry configuration.
pub struct RegistryConfig {
    /// Base URL for the registry (raw GitHub content URL).
    /// Must start with `https://`.
    pub registry_url: String,
    /// Local data directory (e.g. `~/.fawx`).
    pub data_dir: PathBuf,
    /// Trusted Ed25519 public keys for signature verification.
    pub trusted_keys: Vec<Vec<u8>>,
}

/// A skill entry from the registry index.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillEntry {
    pub name: String,
    pub version: String,
    pub description: String,
    pub author: String,
    #[serde(default)]
    pub capabilities: Vec<String>,
    pub size_bytes: Option<u64>,
}

/// Result of a successful install operation.
///
/// A successful install implies signature verification passed — there is no
/// separate `signature_verified` field because install always verifies.
#[derive(Debug)]
pub struct InstallResult {
    pub name: String,
    pub version: String,
    pub size_bytes: u64,
    pub install_path: PathBuf,
}

/// Metadata about a locally installed skill.
#[derive(Debug)]
pub struct InstalledSkill {
    pub name: String,
    pub version: String,
    pub capabilities: Vec<String>,
}

// ---------------------------------------------------------------------------
// Internal index structure
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct RegistryIndex {
    #[allow(dead_code)]
    version: u32,
    skills: Vec<SkillEntry>,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Parse raw JSON bytes into a list of skill entries.
pub fn parse_index(json: &str) -> Result<Vec<SkillEntry>, MarketplaceError> {
    let index: RegistryIndex =
        serde_json::from_str(json).map_err(|e| MarketplaceError::InvalidIndex(e.to_string()))?;
    Ok(index.skills)
}

/// Validate that a skill name contains only safe characters.
///
/// Rejects names that contain path separators, `..`, or any characters
/// that could lead to path traversal when used in filesystem paths or URLs.
/// Only alphanumeric characters, hyphens, and underscores are allowed.
pub fn validate_skill_name(name: &str) -> Result<(), MarketplaceError> {
    if name.is_empty() {
        return Err(MarketplaceError::InstallError(
            "skill name must not be empty".to_string(),
        ));
    }
    if name.contains('/') || name.contains('\\') || name.contains("..") || name.contains('\0') {
        return Err(MarketplaceError::InstallError(format!(
            "skill name contains forbidden characters: '{name}'"
        )));
    }
    // Allow only alphanumeric, hyphens, and underscores.
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(MarketplaceError::InstallError(format!(
            "skill name contains invalid characters (only alphanumeric, '-', '_' allowed): '{name}'"
        )));
    }
    Ok(())
}

/// Validate that a registry URL uses HTTPS.
fn require_https(url: &str) -> Result<(), MarketplaceError> {
    if !url.starts_with("https://") {
        return Err(MarketplaceError::InsecureRegistry(format!(
            "registry URL must use HTTPS, got: {url}"
        )));
    }
    Ok(())
}

/// Fetch the registry index over HTTPS.
fn fetch_index(registry_url: &str) -> Result<Vec<SkillEntry>, MarketplaceError> {
    require_https(registry_url)?;
    let url = format!("{registry_url}/index.json");
    let body = http_get_string(&url)?;
    parse_index(&body)
}

/// Download bytes from a URL.
fn http_get_bytes(url: &str) -> Result<Vec<u8>, MarketplaceError> {
    let response = ureq::get(url)
        .call()
        .map_err(|e| MarketplaceError::NetworkError(format!("{url}: {e}")))?;
    let mut buf = Vec::new();
    response
        .into_reader()
        .read_to_end(&mut buf)
        .map_err(|e| MarketplaceError::NetworkError(format!("reading {url}: {e}")))?;
    Ok(buf)
}

/// Download a string from a URL.
fn http_get_string(url: &str) -> Result<String, MarketplaceError> {
    let response = ureq::get(url)
        .call()
        .map_err(|e| MarketplaceError::NetworkError(format!("{url}: {e}")))?;
    response
        .into_string()
        .map_err(|e| MarketplaceError::NetworkError(format!("reading {url}: {e}")))
}

/// Search the registry for skills matching `query` (case-insensitive).
///
/// Matches against skill name and description fields.
pub fn search(config: &RegistryConfig, query: &str) -> Result<Vec<SkillEntry>, MarketplaceError> {
    let entries = fetch_index(&config.registry_url)?;
    Ok(filter_entries(&entries, query))
}

/// Filter skill entries by a case-insensitive query against name and description.
pub fn filter_entries(entries: &[SkillEntry], query: &str) -> Vec<SkillEntry> {
    let query_lower = query.to_lowercase();
    entries
        .iter()
        .filter(|e| {
            e.name.to_lowercase().contains(&query_lower)
                || e.description.to_lowercase().contains(&query_lower)
        })
        .cloned()
        .collect()
}

/// Downloaded skill files from the registry.
struct DownloadedSkill {
    manifest: String,
    wasm: Vec<u8>,
    signature: Vec<u8>,
}

/// Download skill files (manifest, WASM binary, signature) from the registry.
fn download_skill_files(
    registry_url: &str,
    skill_name: &str,
) -> Result<DownloadedSkill, MarketplaceError> {
    let manifest_url = format!("{registry_url}/skills/{skill_name}/manifest.toml");
    let wasm_url = format!("{registry_url}/skills/{skill_name}/{skill_name}.wasm");
    let sig_url = format!("{registry_url}/skills/{skill_name}/{skill_name}.wasm.sig");

    Ok(DownloadedSkill {
        manifest: http_get_string(&manifest_url)?,
        wasm: http_get_bytes(&wasm_url)?,
        signature: http_get_bytes(&sig_url)?,
    })
}

/// Install a skill from the registry.
///
/// Downloads the WASM binary and its signature, verifies the signature
/// against the configured trusted keys, validates the manifest, and
/// copies files into the local skills directory.
pub fn install(
    config: &RegistryConfig,
    skill_name: &str,
) -> Result<InstallResult, MarketplaceError> {
    require_https(&config.registry_url)?;
    validate_skill_name(skill_name)?;

    let entries = fetch_index(&config.registry_url)?;
    let name_lower = skill_name.to_lowercase();
    let entry = entries
        .iter()
        .find(|e| e.name.to_lowercase() == name_lower)
        .ok_or_else(|| {
            MarketplaceError::SkillNotFound(format!("'{skill_name}' not in registry"))
        })?;

    validate_skill_name(&entry.name)?;

    let files = download_skill_files(&config.registry_url, &entry.name)?;
    verify_against_trusted_keys(&files.wasm, &files.signature, &config.trusted_keys)?;
    validate_manifest_toml(&files.manifest)?;

    let install_dir = config.data_dir.join("skills").join(&entry.name);
    // Verify the resolved install path is actually under the skills directory.
    let skills_dir = config.data_dir.join("skills");
    let canonical_skills = skills_dir
        .canonicalize()
        .unwrap_or_else(|_| skills_dir.clone());
    let canonical_install = install_dir.canonicalize().unwrap_or_else(|_| {
        // If install_dir doesn't exist yet, canonicalize the parent and append.
        let parent = install_dir.parent().unwrap_or(&skills_dir);
        let canonical_parent = parent
            .canonicalize()
            .unwrap_or_else(|_| parent.to_path_buf());
        canonical_parent.join(install_dir.file_name().unwrap_or_default())
    });
    if !canonical_install.starts_with(&canonical_skills) {
        return Err(MarketplaceError::InstallError(format!(
            "install path escapes skills directory: {}",
            install_dir.display()
        )));
    }
    write_skill_files(
        &install_dir,
        &entry.name,
        &files.manifest,
        &files.wasm,
        &files.signature,
    )?;

    Ok(InstallResult {
        name: entry.name.clone(),
        version: entry.version.clone(),
        size_bytes: files.wasm.len() as u64,
        install_path: install_dir,
    })
}

/// Verify WASM bytes against a signature using any of the trusted keys.
fn verify_against_trusted_keys(
    wasm_bytes: &[u8],
    sig_bytes: &[u8],
    trusted_keys: &[Vec<u8>],
) -> Result<(), MarketplaceError> {
    if trusted_keys.is_empty() {
        return Err(MarketplaceError::SignatureInvalid(
            "no trusted keys configured".to_string(),
        ));
    }
    for key in trusted_keys {
        match fx_skills::signing::verify_skill(wasm_bytes, sig_bytes, key) {
            Ok(true) => return Ok(()),
            Ok(false) => continue,
            Err(_) => continue,
        }
    }
    Err(MarketplaceError::SignatureInvalid(
        "no trusted key verified the signature".to_string(),
    ))
}

/// Validate a manifest TOML string via fx-skills.
fn validate_manifest_toml(toml_str: &str) -> Result<(), MarketplaceError> {
    let manifest = fx_skills::manifest::parse_manifest(toml_str)
        .map_err(|e| MarketplaceError::ManifestInvalid(e.to_string()))?;
    fx_skills::manifest::validate_manifest(&manifest)
        .map_err(|e| MarketplaceError::ManifestInvalid(e.to_string()))?;
    Ok(())
}

/// Write downloaded skill files into the install directory.
fn write_skill_files(
    install_dir: &Path,
    skill_name: &str,
    manifest_toml: &str,
    wasm_bytes: &[u8],
    sig_bytes: &[u8],
) -> Result<(), MarketplaceError> {
    fs::create_dir_all(install_dir).map_err(|e| {
        MarketplaceError::InstallError(format!("mkdir {}: {e}", install_dir.display()))
    })?;

    let write = |name: &str, data: &[u8]| -> Result<(), MarketplaceError> {
        let path = install_dir.join(name);
        fs::write(&path, data)
            .map_err(|e| MarketplaceError::InstallError(format!("write {}: {e}", path.display())))
    };

    write("manifest.toml", manifest_toml.as_bytes())?;
    write(&format!("{skill_name}.wasm"), wasm_bytes)?;
    write(&format!("{skill_name}.wasm.sig"), sig_bytes)?;
    Ok(())
}

/// List locally installed skills by reading `<data_dir>/skills/*/manifest.toml`.
pub fn list_installed(data_dir: &Path) -> Result<Vec<InstalledSkill>, MarketplaceError> {
    let skills_dir = data_dir.join("skills");
    if !skills_dir.exists() {
        return Ok(Vec::new());
    }

    let mut installed = Vec::new();
    let entries = fs::read_dir(&skills_dir)
        .map_err(|e| MarketplaceError::InstallError(format!("read dir: {e}")))?;

    for entry in entries {
        let entry =
            entry.map_err(|e| MarketplaceError::InstallError(format!("read entry: {e}")))?;
        if !entry.path().is_dir() {
            continue;
        }
        if let Some(skill) = read_installed_skill(&entry.path()) {
            installed.push(skill);
        }
    }

    installed.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(installed)
}

/// Try to read an installed skill from its directory.
fn read_installed_skill(dir: &Path) -> Option<InstalledSkill> {
    let manifest_path = dir.join("manifest.toml");
    let toml_str = fs::read_to_string(&manifest_path).ok()?;
    let manifest = fx_skills::manifest::parse_manifest(&toml_str).ok()?;
    Some(InstalledSkill {
        name: manifest.name,
        version: manifest.version,
        capabilities: manifest
            .capabilities
            .iter()
            .map(|c| c.to_string())
            .collect(),
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    const VALID_INDEX: &str = r#"{
        "version": 1,
        "skills": [
            {
                "name": "weather",
                "version": "1.0.0",
                "description": "Weather lookup via wttr.in",
                "author": "Fawx Team",
                "capabilities": ["network"],
                "size_bytes": 42000
            },
            {
                "name": "github",
                "version": "1.1.0",
                "description": "GitHub integration and PR management",
                "author": "Fawx Team",
                "capabilities": ["network", "storage"],
                "size_bytes": 85000
            }
        ]
    }"#;

    fn sample_entries() -> Vec<SkillEntry> {
        parse_index(VALID_INDEX).expect("valid test index")
    }

    fn write_test_manifest(dir: &Path, name: &str, version: &str) {
        let manifest = format!(
            r#"name = "{name}"
version = "{version}"
description = "A test skill"
author = "Test"
api_version = "host_api_v1"
capabilities = ["network"]
"#
        );
        fs::create_dir_all(dir).expect("create dir");
        fs::write(dir.join("manifest.toml"), manifest).expect("write manifest");
    }

    // 1. search_filters_by_name
    #[test]
    fn search_filters_by_name() {
        let entries = sample_entries();
        let results = filter_entries(&entries, "weather");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "weather");
    }

    // 2. search_filters_by_description
    #[test]
    fn search_filters_by_description() {
        let entries = sample_entries();
        let results = filter_entries(&entries, "PR management");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "github");
    }

    // 3. search_returns_empty_on_no_match
    #[test]
    fn search_returns_empty_on_no_match() {
        let entries = sample_entries();
        let results = filter_entries(&entries, "nonexistent-xyz");
        assert!(results.is_empty());
    }

    // 4. parse_valid_index
    #[test]
    fn parse_valid_index() {
        let entries = parse_index(VALID_INDEX).expect("should parse");
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].name, "weather");
        assert_eq!(entries[1].name, "github");
        assert_eq!(entries[0].size_bytes, Some(42000));
    }

    // 5. parse_invalid_index
    #[test]
    fn parse_invalid_index() {
        let result = parse_index("not json at all {{{");
        assert!(result.is_err());
        assert!(matches!(result, Err(MarketplaceError::InvalidIndex(_))));
    }

    // 6. reject_http_registry
    #[test]
    fn reject_http_registry() {
        let result = require_https("http://example.com/registry");
        assert!(result.is_err());
        assert!(matches!(result, Err(MarketplaceError::InsecureRegistry(_))));

        // HTTPS should pass
        let result = require_https("https://example.com/registry");
        assert!(result.is_ok());
    }

    // 7. list_installed_finds_skills
    #[test]
    fn list_installed_finds_skills() {
        let tmp = TempDir::new().expect("tempdir");
        let skills_dir = tmp.path().join("skills");

        write_test_manifest(&skills_dir.join("weather"), "weather", "1.0.0");
        write_test_manifest(&skills_dir.join("github"), "github", "1.1.0");

        let installed = list_installed(tmp.path()).expect("should list");
        assert_eq!(installed.len(), 2);
        // sorted alphabetically
        assert_eq!(installed[0].name, "github");
        assert_eq!(installed[1].name, "weather");
        assert_eq!(installed[1].version, "1.0.0");
    }

    // 8. verify rejects empty trusted keys
    #[test]
    fn install_rejects_without_trusted_keys() {
        let result = verify_against_trusted_keys(b"wasm data", b"sig data", &[]);
        assert!(result.is_err());
        match result {
            Err(MarketplaceError::SignatureInvalid(msg)) => {
                assert!(msg.contains("no trusted keys"), "unexpected msg: {msg}");
            }
            other => panic!("expected SignatureInvalid, got: {other:?}"),
        }
    }

    // 9. validate_skill_name rejects path traversal
    #[test]
    fn validate_skill_name_rejects_path_traversal() {
        assert!(validate_skill_name("../../etc").is_err());
        assert!(validate_skill_name("skill/../../../target").is_err());
        assert!(validate_skill_name("skill/subdir").is_err());
        assert!(validate_skill_name("skill\\subdir").is_err());
        assert!(validate_skill_name("").is_err());
        assert!(validate_skill_name("skill\0name").is_err());
        assert!(validate_skill_name("skill name").is_err());
        assert!(validate_skill_name("skill.name").is_err());
        assert!(validate_skill_name("-o ProxyCommand").is_err());
    }

    // 10. validate_skill_name accepts valid names
    #[test]
    fn validate_skill_name_accepts_valid() {
        assert!(validate_skill_name("weather").is_ok());
        assert!(validate_skill_name("my-skill").is_ok());
        assert!(validate_skill_name("my_skill_v2").is_ok());
        assert!(validate_skill_name("GitHub123").is_ok());
    }

    // 11. list_installed_empty_dir
    #[test]
    fn list_installed_empty_dir() {
        let tmp = TempDir::new().expect("tempdir");
        // No skills dir at all
        let installed = list_installed(tmp.path()).expect("should list");
        assert!(installed.is_empty());

        // Empty skills dir
        fs::create_dir_all(tmp.path().join("skills")).expect("mkdir");
        let installed = list_installed(tmp.path()).expect("should list");
        assert!(installed.is_empty());
    }
}
