use crate::process::format_process_output;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Output;
use tokio::fs;
use tokio::process::Command;

#[derive(Debug, Clone)]
pub struct VenvManager {
    root: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PackageInfo {
    pub name: String,
    pub version: String,
}

impl VenvManager {
    #[must_use]
    pub fn new(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
        }
    }

    #[must_use]
    pub fn venv_path(&self, name: &str) -> PathBuf {
        self.root.join(name)
    }

    #[must_use]
    pub fn python_path(&self, name: &str) -> PathBuf {
        self.venv_path(name).join("bin").join("python")
    }

    #[must_use]
    pub fn pip_path(&self, name: &str) -> PathBuf {
        self.venv_path(name).join("bin").join("pip")
    }

    pub async fn ensure_venv(&self, name: &str) -> Result<PathBuf, String> {
        validate_venv_name(name)?;
        self.ensure_root().await?;

        let venv_path = self.venv_path(name);
        if path_exists(&self.python_path(name)).await? {
            self.ensure_pip_entrypoint(name).await?;
            return Ok(venv_path);
        }

        let python = detect_python_binary().await?;
        create_venv(&python, &venv_path, name).await?;
        self.ensure_pip_entrypoint(name).await?;
        Ok(venv_path)
    }

    pub async fn list_venvs(&self) -> Result<Vec<String>, String> {
        if !path_exists(&self.root).await? {
            return Ok(Vec::new());
        }

        let mut entries = fs::read_dir(&self.root)
            .await
            .map_err(|error| format!("failed to read '{}': {error}", self.root.display()))?;
        let mut names = Vec::new();

        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|error| format!("failed to read venv entry: {error}"))?
        {
            if entry
                .file_type()
                .await
                .map_err(|error| format!("failed to inspect venv entry: {error}"))?
                .is_dir()
            {
                names.push(entry.file_name().to_string_lossy().into_owned());
            }
        }

        names.sort();
        Ok(names)
    }

    pub async fn delete_venv(&self, name: &str) -> Result<(), String> {
        validate_venv_name(name)?;
        let path = self.venv_path(name);
        if !path_exists(&path).await? {
            return Ok(());
        }

        fs::remove_dir_all(&path)
            .await
            .map_err(|error| format!("failed to delete venv '{name}': {error}"))
    }

    pub async fn info(&self, name: &str) -> Result<Vec<PackageInfo>, String> {
        validate_venv_name(name)?;
        ensure_existing_venv(self, name).await?;

        let output = Command::new(self.pip_path(name))
            .args(["list", "--format=json"])
            .output()
            .await
            .map_err(|error| format!("failed to inspect venv '{name}': {error}"))?;
        let output = require_success(output, &format!("inspect venv '{name}'"))?;
        parse_package_list(&output.stdout)
    }

    async fn ensure_root(&self) -> Result<(), String> {
        fs::create_dir_all(&self.root).await.map_err(|error| {
            format!(
                "failed to create venv root '{}': {error}",
                self.root.display()
            )
        })
    }

    async fn ensure_pip_entrypoint(&self, name: &str) -> Result<(), String> {
        let pip_path = self.pip_path(name);
        if path_exists(&pip_path).await? {
            return Ok(());
        }

        write_pip_shim(&self.python_path(name), &pip_path).await
    }
}

async fn ensure_existing_venv(manager: &VenvManager, name: &str) -> Result<(), String> {
    if path_exists(&manager.python_path(name)).await? {
        return Ok(());
    }

    Err(format!("venv '{name}' does not exist"))
}

fn validate_venv_name(name: &str) -> Result<(), String> {
    let valid = !name.is_empty()
        && name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_');
    if valid {
        return Ok(());
    }

    Err("venv names must use only letters, numbers, '-' or '_'".to_string())
}

async fn detect_python_binary() -> Result<String, String> {
    for candidate in ["python3", "python"] {
        if supports_python_three(candidate).await? {
            return Ok(candidate.to_string());
        }
    }

    Err("python 3 interpreter not found; tried python3 and python".to_string())
}

async fn supports_python_three(candidate: &str) -> Result<bool, String> {
    match Command::new(candidate).arg("--version").output().await {
        Ok(output) => Ok(matches!(parse_python_major_version(&output), Some(major) if major >= 3)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(format!("failed to inspect {candidate}: {error}")),
    }
}

fn parse_python_major_version(output: &Output) -> Option<u64> {
    let version = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let line = version.lines().next()?.trim();
    let version = line.strip_prefix("Python ")?;
    version.split('.').next()?.parse().ok()
}

async fn create_venv(python: &str, venv_path: &Path, name: &str) -> Result<(), String> {
    let output = run_venv_command(python, venv_path, false).await?;
    if output.status.success() {
        return Ok(());
    }
    if ensurepip_missing(&output) {
        let fallback = run_venv_command(python, venv_path, true).await?;
        let _ = require_success(fallback, &format!("create venv '{name}'"))?;
        return Ok(());
    }

    Err(format!(
        "create venv '{name}' failed: {}",
        format_process_output(&output)
    ))
}

async fn run_venv_command(
    python: &str,
    venv_path: &Path,
    without_pip: bool,
) -> Result<Output, String> {
    let mut command = Command::new(python);
    command.args(["-m", "venv"]);
    if without_pip {
        command.arg("--without-pip");
    }
    command.arg(venv_path);
    command
        .output()
        .await
        .map_err(|error| format!("failed to create venv '{}': {error}", venv_path.display()))
}

fn ensurepip_missing(output: &Output) -> bool {
    let stderr = String::from_utf8_lossy(&output.stderr).to_ascii_lowercase();
    let stdout = String::from_utf8_lossy(&output.stdout).to_ascii_lowercase();
    stderr.contains("ensurepip") || stdout.contains("ensurepip")
}

async fn write_pip_shim(python_path: &Path, pip_path: &Path) -> Result<(), String> {
    let content = format!(
        "#!/usr/bin/env sh\n\"{}\" -m pip \"$@\"\n",
        python_path.display()
    );
    fs::write(pip_path, content)
        .await
        .map_err(|error| format!("failed to write pip shim '{}': {error}", pip_path.display()))?;
    set_executable_permissions(pip_path).await
}

#[cfg(unix)]
async fn set_executable_permissions(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
        .await
        .map_err(|error| format!("failed to chmod '{}': {error}", path.display()))
}

#[cfg(not(unix))]
async fn set_executable_permissions(_path: &Path) -> Result<(), String> {
    Ok(())
}

fn require_success(output: Output, action: &str) -> Result<Output, String> {
    if output.status.success() {
        return Ok(output);
    }

    Err(format!(
        "{action} failed: {}",
        format_process_output(&output)
    ))
}

fn parse_package_list(stdout: &[u8]) -> Result<Vec<PackageInfo>, String> {
    serde_json::from_slice(stdout)
        .map_err(|error| format!("failed to parse pip list output: {error}"))
}

async fn path_exists(path: &Path) -> Result<bool, String> {
    fs::try_exists(path)
        .await
        .map_err(|error| format!("failed to inspect '{}': {error}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn create_venv() {
        let temp_dir = TempDir::new().expect("tempdir");
        let manager = VenvManager::new(temp_dir.path());

        let path = manager.ensure_venv("alpha").await.expect("venv created");

        assert!(path.exists());
        assert!(manager.python_path("alpha").exists());
    }

    #[tokio::test]
    async fn list_venvs() {
        let temp_dir = TempDir::new().expect("tempdir");
        let manager = VenvManager::new(temp_dir.path());
        manager.ensure_venv("alpha").await.expect("alpha created");
        manager.ensure_venv("beta").await.expect("beta created");

        let venvs = manager.list_venvs().await.expect("venvs listed");

        assert_eq!(venvs, vec!["alpha".to_string(), "beta".to_string()]);
    }

    #[tokio::test]
    async fn delete_venv() {
        let temp_dir = TempDir::new().expect("tempdir");
        let manager = VenvManager::new(temp_dir.path());
        let path = manager.ensure_venv("alpha").await.expect("venv created");

        manager.delete_venv("alpha").await.expect("venv deleted");

        assert!(!path.exists());
    }

    #[test]
    fn parse_python_major_version_accepts_python_three() {
        let output = Output {
            status: std::process::ExitStatus::default(),
            stdout: b"Python 3.12.2\n".to_vec(),
            stderr: Vec::new(),
        };

        assert_eq!(parse_python_major_version(&output), Some(3));
    }

    #[test]
    fn parse_python_major_version_rejects_python_two() {
        let output = Output {
            status: std::process::ExitStatus::default(),
            stdout: Vec::new(),
            stderr: b"Python 2.7.18\n".to_vec(),
        };

        assert_eq!(parse_python_major_version(&output), Some(2));
    }
}
