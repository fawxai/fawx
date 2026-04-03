use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use fx_skills::signing::generate_keypair;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

const SIGNING_KEY_RELATIVE_PATH: &str = "keys/signing_key.pem";

#[derive(Debug, Clone, Subcommand)]
pub enum KeysCommands {
    /// Generate a local WASM signing keypair and trust its public key
    Generate(GenerateKeysArgs),
    /// List trusted WASM signing public keys
    List(ListKeysArgs),
    /// Trust a public key for local WASM signature verification
    Trust(TrustKeyArgs),
    /// Revoke a trusted public key by fingerprint
    Revoke(RevokeKeyArgs),
}

#[derive(Debug, Clone, Args)]
pub struct GenerateKeysArgs {
    /// Replace an existing signing key
    #[arg(long)]
    pub(crate) force: bool,
    /// Override data directory (default: configured data dir or ~/.fawx)
    #[arg(long)]
    pub(crate) data_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, Args)]
pub struct ListKeysArgs {
    /// Override data directory (default: configured data dir or ~/.fawx)
    #[arg(long)]
    pub(crate) data_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, Args)]
pub struct TrustKeyArgs {
    /// Path to a 32-byte Ed25519 public key file
    pub(crate) path: PathBuf,
    /// Override data directory (default: configured data dir or ~/.fawx)
    #[arg(long)]
    pub(crate) data_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, Args)]
pub struct RevokeKeyArgs {
    /// Trusted key fingerprint shown by `fawx keys list`
    pub(crate) fingerprint: String,
    /// Override data directory (default: configured data dir or ~/.fawx)
    #[arg(long)]
    pub(crate) data_dir: Option<PathBuf>,
}

#[derive(Debug, Clone)]
struct TrustedKeyEntry {
    path: PathBuf,
    file_name: String,
    fingerprint: String,
    file_size: u64,
}

pub fn run(command: KeysCommands) -> Result<i32> {
    let output = match command {
        KeysCommands::Generate(args) => generate_output(args.force, args.data_dir.as_deref())?,
        KeysCommands::List(args) => list_output(args.data_dir.as_deref())?,
        KeysCommands::Trust(args) => trust_output(&args.path, args.data_dir.as_deref())?,
        KeysCommands::Revoke(args) => revoke_output(&args.fingerprint, args.data_dir.as_deref())?,
    };
    println!("{output}");
    Ok(0)
}

pub fn generate_output(force: bool, data_dir: Option<&Path>) -> Result<String> {
    let root = resolve_data_dir(data_dir);
    let signing_key_path = signing_key_path(&root);
    ensure_key_can_be_generated(&signing_key_path, force)?;
    let (private_key, public_key) = generate_keypair()
        .map_err(|error| anyhow::anyhow!("Failed to generate keypair: {error}"))?;
    let trusted_key_path = trusted_key_path(&root, &public_key);
    write_generated_keys(
        &signing_key_path,
        &trusted_key_path,
        &private_key,
        &public_key,
    )?;
    Ok(render_generate_output(
        &signing_key_path,
        &trusted_key_path,
        &public_key,
    ))
}

pub fn list_output(data_dir: Option<&Path>) -> Result<String> {
    let root = resolve_data_dir(data_dir);
    let entries = trusted_key_entries_from_dir(&trusted_keys_dir(&root))?;
    if entries.is_empty() {
        return Ok("No trusted public keys.".to_string());
    }

    let mut lines = vec!["Trusted public keys:".to_string()];
    lines.extend(entries.into_iter().map(render_trusted_key_line));
    Ok(lines.join("\n"))
}

pub fn trust_output(path: &Path, data_dir: Option<&Path>) -> Result<String> {
    let public_key = read_public_key_file(path)?;
    let root = resolve_data_dir(data_dir);
    let destination = trusted_key_path(&root, &public_key);
    write_trusted_key(&destination, &public_key)?;
    Ok(render_trust_output(&destination, &public_key))
}

pub fn revoke_output(fingerprint: &str, data_dir: Option<&Path>) -> Result<String> {
    let root = resolve_data_dir(data_dir);
    let matches = matching_trusted_keys(&trusted_keys_dir(&root), fingerprint)?;
    if matches.is_empty() {
        anyhow::bail!("No trusted key matched fingerprint '{fingerprint}'");
    }

    for entry in &matches {
        fs::remove_file(&entry.path)
            .with_context(|| format!("Failed to remove {}", entry.path.display()))?;
    }

    Ok(render_revoke_output(fingerprint, matches.len()))
}

fn resolve_data_dir(data_dir: Option<&Path>) -> PathBuf {
    let Some(data_dir) = data_dir else {
        return configured_data_dir();
    };
    data_dir.to_path_buf()
}

fn configured_data_dir() -> PathBuf {
    let base = crate::startup::fawx_data_dir();
    let config = crate::startup::load_config().unwrap_or_default();
    crate::startup::configured_data_dir(&base, &config)
}

fn signing_key_path(root: &Path) -> PathBuf {
    root.join(SIGNING_KEY_RELATIVE_PATH)
}

fn trusted_keys_dir(root: &Path) -> PathBuf {
    root.join("trusted_keys")
}

fn trusted_key_path(root: &Path, public_key: &[u8]) -> PathBuf {
    let fingerprint = public_key_fingerprint(public_key);
    trusted_keys_dir(root).join(format!("{fingerprint}.pub"))
}

fn ensure_key_can_be_generated(signing_key_path: &Path, force: bool) -> Result<()> {
    if signing_key_path.exists() && !force {
        anyhow::bail!(
            "Signing key already exists at {}. Re-run with --force to replace it.",
            signing_key_path.display()
        );
    }

    let parent = signing_key_path
        .parent()
        .context("Signing key path must have a parent directory")?;
    fs::create_dir_all(parent).with_context(|| format!("Failed to create {}", parent.display()))?;
    Ok(())
}

fn write_generated_keys(
    signing_key_path: &Path,
    trusted_key_path: &Path,
    private_key: &[u8],
    public_key: &[u8],
) -> Result<()> {
    fs::write(signing_key_path, private_key)
        .with_context(|| format!("Failed to write {}", signing_key_path.display()))?;
    tighten_private_key_permissions(signing_key_path)?;
    write_trusted_key(trusted_key_path, public_key)
}

fn write_trusted_key(path: &Path, public_key: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .context("Trusted key path must have a parent directory")?;
    fs::create_dir_all(parent).with_context(|| format!("Failed to create {}", parent.display()))?;
    fs::write(path, public_key).with_context(|| format!("Failed to write {}", path.display()))
}

#[cfg(unix)]
fn tighten_private_key_permissions(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let permissions = fs::Permissions::from_mode(0o600);
    fs::set_permissions(path, permissions)
        .with_context(|| format!("Failed to set permissions on {}", path.display()))
}

#[cfg(not(unix))]
fn tighten_private_key_permissions(_path: &Path) -> Result<()> {
    Ok(())
}

fn render_generate_output(
    signing_key_path: &Path,
    trusted_key_path: &Path,
    public_key: &[u8],
) -> String {
    let fingerprint = public_key_fingerprint(public_key);
    format!(
        "Generated signing key\n  Private key: {}\n  Trusted public key: {}\n  Fingerprint: {}\n  Restart the server if it is already running so trusted keys reload before signature status is rechecked.",
        signing_key_path.display(),
        trusted_key_path.display(),
        fingerprint
    )
}

fn render_trust_output(destination: &Path, public_key: &[u8]) -> String {
    let fingerprint = public_key_fingerprint(public_key);
    format!(
        "Trusted public key\n  Path: {}\n  Fingerprint: {}\n  Restart the server if it is already running so trusted keys reload before signature status is rechecked.",
        destination.display(),
        fingerprint
    )
}

fn render_revoke_output(fingerprint: &str, removed: usize) -> String {
    format!("Revoked {removed} trusted key file(s) matching {fingerprint}.")
}

fn trusted_key_entries_from_dir(trusted_dir: &Path) -> Result<Vec<TrustedKeyEntry>> {
    let mut keys = Vec::new();
    if !trusted_dir.exists() {
        return Ok(keys);
    }

    for entry in fs::read_dir(trusted_dir).context("Failed to read trusted keys directory")? {
        let path = entry?.path();
        if is_public_key_path(&path) {
            keys.push(trusted_key_entry_from_path(&path)?);
        }
    }

    keys.sort_by(|left, right| left.file_name.cmp(&right.file_name));
    Ok(keys)
}

fn matching_trusted_keys(trusted_dir: &Path, fingerprint: &str) -> Result<Vec<TrustedKeyEntry>> {
    let entries = trusted_key_entries_from_dir(trusted_dir)?;
    Ok(entries
        .into_iter()
        .filter(|entry| entry.fingerprint == fingerprint)
        .collect())
}

fn trusted_key_entry_from_path(path: &Path) -> Result<TrustedKeyEntry> {
    let public_key = read_public_key_file(path)?;
    let file_name = display_file_name(path);
    let file_size = fs::metadata(path)
        .with_context(|| format!("Failed to read metadata for {}", path.display()))?
        .len();
    Ok(TrustedKeyEntry {
        path: path.to_path_buf(),
        file_name,
        fingerprint: public_key_fingerprint(&public_key),
        file_size,
    })
}

fn render_trusted_key_line(key: TrustedKeyEntry) -> String {
    format!(
        "  {} {} {} bytes",
        key.file_name, key.fingerprint, key.file_size
    )
}

fn read_public_key_file(path: &Path) -> Result<Vec<u8>> {
    let public_key =
        fs::read(path).with_context(|| format!("Failed to read {}", path.display()))?;
    if public_key.len() != 32 {
        anyhow::bail!(
            "invalid public key length at {}: expected 32 bytes, found {}",
            path.display(),
            public_key.len()
        );
    }
    Ok(public_key)
}

fn is_public_key_path(path: &Path) -> bool {
    path.extension().and_then(|ext| ext.to_str()) == Some("pub")
}

fn public_key_fingerprint(public_key: &[u8]) -> String {
    let digest = Sha256::digest(public_key);
    hex_encode(&digest[..8])
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn display_file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| path.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::skill_sign::{sign_output, SignSelection};
    use fx_skills::signing::verify_skill;
    use tempfile::TempDir;

    fn install_skill(temp: &TempDir, name: &str, wasm_bytes: &[u8]) {
        let skill_dir = temp.path().join("skills").join(name);
        fs::create_dir_all(&skill_dir).expect("create skill dir");
        fs::write(
            skill_dir.join("manifest.toml"),
            format!(
                "name = \"{name}\"\nversion = \"1.0.0\"\ndescription = \"test\"\nauthor = \"tester\"\napi_version = \"host_api_v1\"\ncapabilities = []\n"
            ),
        )
        .expect("write manifest");
        fs::write(skill_dir.join(format!("{name}.wasm")), wasm_bytes).expect("write wasm");
    }

    fn read_trusted_public_key(temp: &TempDir) -> Vec<u8> {
        let trusted_dir = temp.path().join("trusted_keys");
        let trusted_key = fs::read_dir(&trusted_dir)
            .expect("read trusted dir")
            .next()
            .expect("trusted key entry")
            .expect("dir entry")
            .path();
        fs::read(trusted_key).expect("read trusted key")
    }

    #[test]
    fn generate_output_creates_private_and_trusted_keys() {
        let temp = TempDir::new().expect("tempdir");

        let output = generate_output(false, Some(temp.path())).expect("generate");

        assert!(output.contains("Generated signing key"));
        assert!(temp.path().join("keys/signing_key.pem").exists());
        assert_eq!(
            fs::read_dir(temp.path().join("trusted_keys"))
                .expect("trusted dir")
                .count(),
            1
        );
        let listing = list_output(Some(temp.path())).expect("list");
        assert!(listing.contains("Trusted public keys:"));
    }

    #[test]
    fn generate_output_requires_force_to_replace_existing_key() {
        let temp = TempDir::new().expect("tempdir");
        generate_output(false, Some(temp.path())).expect("generate");

        let error = generate_output(false, Some(temp.path())).expect_err("missing force");

        assert!(error.to_string().contains("Re-run with --force"));
    }

    #[test]
    fn generated_keypair_allows_sign_command_to_succeed() {
        let temp = TempDir::new().expect("tempdir");
        let wasm_bytes = b"weather-wasm";
        install_skill(&temp, "weather", wasm_bytes);
        generate_output(false, Some(temp.path())).expect("generate");

        let output = sign_output(
            SignSelection::Skill("weather".to_string()),
            Some(temp.path()),
        )
        .expect("sign");

        let signature =
            fs::read(temp.path().join("skills/weather/weather.wasm.sig")).expect("read signature");
        let public_key = read_trusted_public_key(&temp);

        assert!(output.contains("Signed skill 'weather'"));
        assert!(verify_skill(wasm_bytes, &signature, &public_key).expect("verify"));
    }

    #[test]
    fn trust_and_revoke_manage_trusted_keys_by_fingerprint() {
        let temp = TempDir::new().expect("tempdir");
        let external = temp.path().join("external.pub");
        let (_, public_key) = generate_keypair().expect("keypair");
        fs::write(&external, &public_key).expect("write public key");

        let trust_output = trust_output(&external, Some(temp.path())).expect("trust");
        let fingerprint = public_key_fingerprint(&public_key);
        let revoke_output = revoke_output(&fingerprint, Some(temp.path())).expect("revoke");

        assert!(trust_output.contains(&fingerprint));
        assert!(revoke_output.contains(&fingerprint));
        assert_eq!(
            list_output(Some(temp.path())).expect("list"),
            "No trusted public keys."
        );
    }
}
