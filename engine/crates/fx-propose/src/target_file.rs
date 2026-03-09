use sha2::{Digest, Sha256};
use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Resolve a proposal target path against the working directory.
#[must_use]
pub(crate) fn resolve_target_path(working_dir: &Path, target_path: &Path) -> PathBuf {
    if target_path.is_absolute() {
        target_path.to_path_buf()
    } else {
        working_dir.join(target_path)
    }
}

/// Compute a SHA-256 digest encoded as lowercase hex.
#[must_use]
pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    format!("{digest:x}")
}

/// Compute the current on-disk hash for a proposal target, if it exists.
pub fn current_file_hash(
    working_dir: &Path,
    target_path: &Path,
) -> Result<Option<String>, io::Error> {
    let resolved = resolve_target_path(working_dir, target_path);
    match fs::read(resolved) {
        Ok(bytes) => Ok(Some(format!("sha256:{}", sha256_hex(&bytes)))),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

/// Resolve and canonicalize a proposal target, ensuring it remains inside the working directory.
pub fn checked_target_path(working_dir: &Path, target_path: &Path) -> Result<PathBuf, io::Error> {
    let canonical_working_dir = fs::canonicalize(working_dir)?;
    let resolved = resolve_target_path(working_dir, target_path);
    let canonical_target = canonicalize_existing_or_parent(&resolved)?;
    if canonical_target.starts_with(&canonical_working_dir) {
        return Ok(canonical_target);
    }
    Err(io::Error::new(
        io::ErrorKind::PermissionDenied,
        format!(
            "target path escapes working directory: {}",
            target_path.display()
        ),
    ))
}

fn canonicalize_existing_or_parent(path: &Path) -> Result<PathBuf, io::Error> {
    if path.exists() {
        return fs::canonicalize(path);
    }

    let (existing_parent, missing_parts) = missing_path_parts(path)?;
    let mut resolved = fs::canonicalize(existing_parent)?;
    append_missing_parts(&mut resolved, missing_parts);
    Ok(resolved)
}

fn missing_path_parts(path: &Path) -> Result<(&Path, Vec<OsString>), io::Error> {
    let mut missing_parts = Vec::new();
    let mut cursor = path;

    while !cursor.exists() {
        let name = cursor.file_name().ok_or_else(invalid_target_path)?;
        missing_parts.push(name.to_os_string());
        cursor = cursor.parent().ok_or_else(invalid_target_path)?;
    }

    Ok((cursor, missing_parts))
}

fn append_missing_parts(path: &mut PathBuf, mut missing_parts: Vec<OsString>) {
    while let Some(part) = missing_parts.pop() {
        path.push(part);
    }
}

fn invalid_target_path() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, "invalid target path")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn current_file_hash_returns_sha256_for_existing_file() {
        let temp = TempDir::new().expect("tempdir");
        let target = temp.path().join("config/settings.toml");
        fs::create_dir_all(target.parent().expect("parent")).expect("mkdir");
        fs::write(&target, "enabled = true\n").expect("write");

        let hash = current_file_hash(temp.path(), Path::new("config/settings.toml"))
            .expect("hash")
            .expect("hash exists");

        assert_eq!(hash, format!("sha256:{}", sha256_hex(b"enabled = true\n")));
    }

    #[test]
    fn checked_target_path_rejects_traversal_escape() {
        let temp = TempDir::new().expect("tempdir");
        let error = checked_target_path(temp.path(), Path::new("../../etc/passwd"))
            .expect_err("escape should fail");

        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
        assert!(error.to_string().contains("escapes working directory"));
    }

    #[test]
    fn checked_target_path_accepts_missing_file_within_working_dir() {
        let temp = TempDir::new().expect("tempdir");
        let target = checked_target_path(temp.path(), Path::new("config/new.toml"))
            .expect("path within working dir");

        assert!(target.starts_with(fs::canonicalize(temp.path()).expect("canonical working dir")));
        assert!(target.ends_with(Path::new("config/new.toml")));
    }
}
