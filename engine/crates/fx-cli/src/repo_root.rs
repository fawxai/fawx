use anyhow::{anyhow, Context};
use std::path::{Path, PathBuf};

pub(crate) fn detect_repo_root() -> anyhow::Result<PathBuf> {
    let current_dir = std::env::current_dir().context("failed to read current directory")?;
    let current_exe = std::env::current_exe().context("failed to read current executable")?;
    resolve_repo_root(&current_dir, &current_exe)
}

pub(crate) fn resolve_repo_root(current_dir: &Path, current_exe: &Path) -> anyhow::Result<PathBuf> {
    find_repo_root(current_dir)
        .or_else(|| find_repo_root_from_exe(current_exe))
        .ok_or_else(|| {
            anyhow!(
                "failed to locate the fawx repository root from current directory ({}) or executable ({})",
                current_dir.display(),
                current_exe.display()
            )
        })
}

fn find_repo_root(current_dir: &Path) -> Option<PathBuf> {
    current_dir
        .ancestors()
        .find(|path| is_repo_root(path))
        .map(Path::to_path_buf)
}

fn find_repo_root_from_exe(current_exe: &Path) -> Option<PathBuf> {
    current_exe.parent().and_then(find_repo_root)
}

fn is_repo_root(path: &Path) -> bool {
    path.join(".git").exists()
        && path.join("Cargo.toml").is_file()
        && path.join("engine/crates/fx-cli/Cargo.toml").is_file()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn resolve_repo_root_prefers_current_directory() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let repo_root = temp_dir.path().join("repo");
        create_repo_root(&repo_root);
        let current_dir = repo_root.join("engine").join("crates");
        let current_exe = repo_root.join("target/release/fawx");

        let discovered = resolve_repo_root(&current_dir, &current_exe).expect("repo root");

        assert_eq!(discovered, repo_root);
    }

    #[test]
    fn resolve_repo_root_falls_back_to_executable_path() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let repo_root = temp_dir.path().join("repo");
        create_repo_root(&repo_root);
        let current_dir = temp_dir.path().join("outside");
        fs::create_dir_all(&current_dir).expect("outside dir");
        let current_exe = repo_root.join("target/release/fawx");

        let discovered = resolve_repo_root(&current_dir, &current_exe).expect("repo root");

        assert_eq!(discovered, repo_root);
    }

    #[test]
    fn resolve_repo_root_errors_when_no_repository_markers_exist() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let current_dir = temp_dir.path().join("outside");
        let current_exe = temp_dir.path().join("bin/fawx");
        fs::create_dir_all(&current_dir).expect("outside dir");
        fs::create_dir_all(current_exe.parent().expect("parent")).expect("bin dir");

        let error = resolve_repo_root(&current_dir, &current_exe).expect_err("missing repo");

        assert!(error
            .to_string()
            .contains("failed to locate the fawx repository root"));
    }

    fn create_repo_root(path: &Path) {
        fs::create_dir_all(path.join("engine/crates/fx-cli")).expect("crate dir");
        fs::write(path.join(".git"), "gitdir: /tmp/worktree\n").expect("git marker");
        fs::write(path.join("Cargo.toml"), "[workspace]\n").expect("workspace file");
        fs::write(
            path.join("engine/crates/fx-cli/Cargo.toml"),
            "[package]\nname = \"fx-cli\"\n",
        )
        .expect("crate manifest");
    }
}
