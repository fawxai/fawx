use super::*;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;

#[test]
fn creates_valid_backup_archive() {
    let data = TempDir::new().expect("tempdir");
    seed_fawx_data(data.path());

    let summary = execute_backup(&test_options(data.path(), data.path().join("backups")))
        .expect("backup");

    assert!(summary.archive_path.exists());
    assert!(summary.breakdown.has_config);
    assert_eq!(summary.breakdown.memory_files, 1);
    assert_eq!(summary.breakdown.context_files, 1);
    assert_eq!(summary.breakdown.session_count, 1);
    let listing = list_archive(&summary.archive_path);
    assert!(listing.contains("./config.toml"));
    assert!(listing.contains("./memory/MEMORY.md"));
    assert!(listing.contains("./context/SOUL.md"));
    assert!(listing.contains("./sessions/session.json"));
}

#[test]
fn reports_categorized_backup_summary() {
    let data = TempDir::new().expect("tempdir");
    seed_fawx_data(data.path());
    write_file(data.path().join("auth.db"), "encrypted");
    write_file(data.path().join("memory/2026-03-10.md"), "day two");
    write_file(data.path().join("context/USER.md"), "user");
    write_file(data.path().join("skills/weather/manifest.toml"), "name = 'weather'");
    write_file(data.path().join("skills/weather/weather.wasm"), "wasm");
    write_file(data.path().join("skills/github/manifest.toml"), "name = 'github'");
    write_file(data.path().join("sessions/second.json"), "{}");
    write_file(data.path().join("audit.log"), "audit");

    let summary = execute_backup(&test_options(data.path(), data.path().join("backups")))
        .expect("backup");

    assert!(summary.breakdown.has_config);
    assert!(summary.breakdown.has_credentials);
    assert_eq!(summary.breakdown.memory_files, 2);
    assert_eq!(summary.breakdown.context_files, 2);
    assert_eq!(summary.breakdown.skill_count, 2);
    assert_eq!(summary.breakdown.session_count, 2);
}

#[test]
fn excludes_backups_directory() {
    let data = TempDir::new().expect("tempdir");
    seed_fawx_data(data.path());
    write_file(data.path().join("backups/old.tar.gz"), "old backup");

    let summary = execute_backup(&test_options(data.path(), data.path().join("backups")))
        .expect("backup");

    let listing = list_archive(&summary.archive_path);
    assert!(!listing.contains("./backups/old.tar.gz"));
}

#[test]
fn excludes_pid_file() {
    let data = TempDir::new().expect("tempdir");
    seed_fawx_data(data.path());
    write_file(data.path().join("fawx.pid"), "12345");

    let summary = execute_backup(&test_options(data.path(), data.path().join("backups")))
        .expect("backup");

    let listing = list_archive(&summary.archive_path);
    assert!(!listing.contains("./fawx.pid"));
}

#[test]
fn writes_backup_to_custom_output_directory() {
    let data = TempDir::new().expect("tempdir");
    let output = TempDir::new().expect("tempdir");
    seed_fawx_data(data.path());

    let summary = execute_backup(&test_options(data.path(), output.path().to_path_buf()))
        .expect("backup");

    assert!(summary.archive_path.starts_with(output.path()));
    assert!(summary.archive_path.exists());
}

#[test]
fn missing_data_directory_fails_cleanly() {
    let data = TempDir::new().expect("tempdir");
    let missing = data.path().join("missing");
    let output = data.path().join("out");

    let error = execute_backup(&test_options(&missing, output)).expect_err("error");

    assert!(error.to_string().contains("No Fawx data directory found"));
}

fn test_options(data_dir: &Path, output_dir: PathBuf) -> BackupOptions {
    BackupOptions {
        data_dir: data_dir.to_path_buf(),
        output_dir,
        timestamp: "2026-03-10-045200".to_string(),
    }
}

fn seed_fawx_data(data_dir: &Path) {
    write_file(data_dir.join("config.toml"), "[general]\n");
    write_file(data_dir.join("memory/MEMORY.md"), "long memory");
    write_file(data_dir.join("context/SOUL.md"), "soul");
    write_file(data_dir.join("sessions/session.json"), "{}");
}

fn write_file(path: PathBuf, body: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent");
    }
    fs::write(path, body).expect("write file");
}

fn list_archive(archive_path: &Path) -> String {
    let output = Command::new("tar")
        .arg("-tzf")
        .arg(archive_path)
        .output()
        .expect("list archive");
    assert!(output.status.success(), "tar listing failed");
    String::from_utf8(output.stdout).expect("utf8")
}
