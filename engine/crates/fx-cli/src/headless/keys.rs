use super::*;

pub(super) fn handle_headless_keys_command(
    base_dir: &Path,
    subcommand: Option<&str>,
    value: Option<&str>,
    option: Option<&str>,
    has_extra_args: bool,
) -> anyhow::Result<String> {
    match subcommand {
        Some("list") if value.is_none() && option.is_none() && !has_extra_args => {
            render_trusted_key_list(base_dir)
        }
        Some("list") => Ok("Usage: /keys list".to_string()),
        Some(other) => Ok(keys_redirect_message(other)),
        None => Ok("Usage: /keys list".to_string()),
    }
}

fn keys_redirect_message(subcommand: &str) -> String {
    format!("Use `fawx keys {subcommand}` CLI for key management.")
}

fn render_trusted_key_list(base_dir: &Path) -> anyhow::Result<String> {
    let keys = trusted_key_entries_from_dir(&trusted_keys_dir(base_dir))?;
    if keys.is_empty() {
        return Ok("No trusted public keys.".to_string());
    }

    let mut lines = vec!["Trusted public keys:".to_string()];
    lines.extend(keys.into_iter().map(render_trusted_key_line));
    Ok(lines.join("\n"))
}

fn render_trusted_key_line(key: TrustedKeyEntry) -> String {
    format!(
        "  {} {} {} bytes",
        key.file_name, key.fingerprint, key.file_size
    )
}

fn trusted_keys_dir(base_dir: &Path) -> PathBuf {
    base_dir.join("trusted_keys")
}

fn trusted_key_entries_from_dir(trusted_dir: &Path) -> anyhow::Result<Vec<TrustedKeyEntry>> {
    let mut keys = Vec::new();
    if !trusted_dir.exists() {
        return Ok(keys);
    }

    for entry in std::fs::read_dir(trusted_dir)? {
        let path = entry?.path();
        if is_public_key_path(&path) {
            keys.push(trusted_key_entry_from_path(&path)?);
        }
    }

    keys.sort_by(|left, right| left.file_name.cmp(&right.file_name));
    Ok(keys)
}

fn trusted_key_entry_from_path(path: &Path) -> anyhow::Result<TrustedKeyEntry> {
    let public_key = read_public_key_file(path)?;
    let file_name = display_file_name(path);
    Ok(TrustedKeyEntry {
        file_name,
        fingerprint: public_key_fingerprint(&public_key),
        file_size: std::fs::metadata(path)?.len(),
    })
}

fn read_public_key_file(path: &Path) -> anyhow::Result<Vec<u8>> {
    let public_key = std::fs::read(path)?;
    if public_key.len() != 32 {
        return Err(anyhow::anyhow!(
            "invalid public key length at {}: expected 32 bytes, found {}",
            path.display(),
            public_key.len()
        ));
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
