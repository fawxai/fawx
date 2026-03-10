use crate::commands::slash::display_path_for_user;
use crate::startup::fawx_data_dir;
use anyhow::{anyhow, Context};
use chrono::Utc;
use clap::Args;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, Args)]
pub struct BackupArgs {
    /// Output directory
    #[arg(long)]
    pub output: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub(crate) struct BackupOptions {
    pub(crate) data_dir: PathBuf,
    pub(crate) output_dir: PathBuf,
    pub(crate) timestamp: String,
}

#[derive(Debug, Clone, Default, Eq, PartialEq)]
pub(crate) struct BackupBreakdown {
    pub(crate) has_config: bool,
    pub(crate) has_credentials: bool,
    pub(crate) memory_files: usize,
    pub(crate) context_files: usize,
    pub(crate) skill_count: usize,
    pub(crate) session_count: usize,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct BackupSummary {
    pub(crate) archive_path: PathBuf,
    pub(crate) breakdown: BackupBreakdown,
    pub(crate) size_bytes: u64,
}

pub fn run(args: &BackupArgs) -> anyhow::Result<i32> {
    let summary = execute_backup(&BackupOptions::from_args(args))?;
    print_backup_summary(&summary);
    Ok(0)
}

impl BackupOptions {
    fn from_args(args: &BackupArgs) -> Self {
        let data_dir = fawx_data_dir();
        Self {
            output_dir: args
                .output
                .clone()
                .unwrap_or_else(|| data_dir.join("backups")),
            data_dir,
            timestamp: backup_timestamp(),
        }
    }
}

fn backup_timestamp() -> String {
    Utc::now().format("%Y-%m-%d-%H%M%S").to_string()
}

pub(crate) fn execute_backup(options: &BackupOptions) -> anyhow::Result<BackupSummary> {
    validate_data_dir(&options.data_dir)?;
    ensure_output_dir(&options.output_dir)?;
    let archive_path = archive_path(options);
    let archive_relative = archive_relative_path(&options.data_dir, &archive_path);
    let files = collect_backup_files(&options.data_dir, archive_relative.as_deref())?;
    let breakdown = summarize_backup_files(&files);
    create_archive(
        &options.data_dir,
        &archive_path,
        archive_relative.as_deref(),
    )?;
    let size_bytes = fs::metadata(&archive_path)
        .with_context(|| format!("failed to read {}", archive_path.display()))?
        .len();
    Ok(BackupSummary {
        archive_path,
        breakdown,
        size_bytes,
    })
}

fn validate_data_dir(data_dir: &Path) -> anyhow::Result<()> {
    if data_dir.is_dir() {
        return Ok(());
    }
    Err(anyhow!("No Fawx data directory found. Nothing to back up."))
}

fn ensure_output_dir(output_dir: &Path) -> anyhow::Result<()> {
    fs::create_dir_all(output_dir)
        .with_context(|| format!("Cannot write to {}", display_path_for_user(output_dir)))?;
    Ok(())
}

fn archive_path(options: &BackupOptions) -> PathBuf {
    let file_name = format!("fawx-backup-{}.tar.gz", options.timestamp);
    options.output_dir.join(file_name)
}

fn archive_relative_path(data_dir: &Path, archive_path: &Path) -> Option<PathBuf> {
    archive_path
        .strip_prefix(data_dir)
        .ok()
        .map(Path::to_path_buf)
}

fn collect_backup_files(
    data_dir: &Path,
    archive_relative: Option<&Path>,
) -> anyhow::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    collect_backup_files_from_dir(data_dir, data_dir, archive_relative, &mut files)?;
    Ok(files)
}

fn collect_backup_files_from_dir(
    root: &Path,
    current: &Path,
    archive_relative: Option<&Path>,
    files: &mut Vec<PathBuf>,
) -> anyhow::Result<()> {
    let entries =
        fs::read_dir(current).with_context(|| format!("failed to read {}", current.display()))?;
    for entry in entries {
        let path = entry?.path();
        let relative = path
            .strip_prefix(root)
            .with_context(|| format!("failed to relativize {}", path.display()))?;
        if should_exclude(relative, archive_relative) {
            continue;
        }
        if path.is_dir() {
            collect_backup_files_from_dir(root, &path, archive_relative, files)?;
        } else if path.is_file() {
            files.push(relative.to_path_buf());
        }
    }
    Ok(())
}

fn should_exclude(relative: &Path, archive_relative: Option<&Path>) -> bool {
    is_backups_path(relative)
        || relative == Path::new("fawx.pid")
        || archive_relative.is_some_and(|archive| relative == archive)
}

fn is_backups_path(relative: &Path) -> bool {
    matches!(relative.components().next(), Some(Component::Normal(name)) if name == "backups")
}

fn summarize_backup_files(files: &[PathBuf]) -> BackupBreakdown {
    let mut breakdown = BackupBreakdown::default();
    let mut skill_names = BTreeSet::new();
    for path in files {
        categorize_backup_path(path, &mut breakdown, &mut skill_names);
    }
    breakdown.skill_count = skill_names.len();
    breakdown
}

fn categorize_backup_path(
    path: &Path,
    breakdown: &mut BackupBreakdown,
    skill_names: &mut BTreeSet<String>,
) {
    if path == Path::new("config.toml") {
        breakdown.has_config = true;
        return;
    }
    if path == Path::new("auth.db") {
        breakdown.has_credentials = true;
        return;
    }
    if is_in_directory(path, "memory") {
        breakdown.memory_files += 1;
        return;
    }
    if is_in_directory(path, "context") {
        breakdown.context_files += 1;
        return;
    }
    if is_in_directory(path, "sessions") {
        breakdown.session_count += 1;
        return;
    }
    if let Some(skill_name) = skill_name(path) {
        skill_names.insert(skill_name.to_string());
    }
}

fn is_in_directory(path: &Path, directory: &str) -> bool {
    matches!(path.components().next(), Some(Component::Normal(name)) if name == directory)
}

fn skill_name(path: &Path) -> Option<&str> {
    let mut components = path.components();
    match (components.next(), components.next()) {
        (Some(Component::Normal(skills)), Some(Component::Normal(name))) if skills == "skills" => {
            name.to_str()
        }
        _ => None,
    }
}

fn create_archive(
    data_dir: &Path,
    archive_path: &Path,
    archive_relative: Option<&Path>,
) -> anyhow::Result<()> {
    let tar = which::which("tar").context("system tar not found")?;
    let mut command = Command::new(tar);
    add_tar_excludes(&mut command, archive_relative);
    let status = command
        .arg("-czf")
        .arg(archive_path)
        .arg("-C")
        .arg(data_dir)
        .arg(".")
        .status()
        .context("failed to run tar")?;
    if status.success() {
        return Ok(());
    }
    Err(anyhow!(
        "Tar creation failed for {}",
        display_path_for_user(archive_path)
    ))
}

fn add_tar_excludes(command: &mut Command, archive_relative: Option<&Path>) {
    command.arg("--exclude=./backups");
    command.arg("--exclude=./fawx.pid");
    if let Some(relative) = archive_relative {
        command.arg(format!("--exclude=./{}", relative.display()));
    }
}

fn print_backup_summary(summary: &BackupSummary) {
    println!("🦊 Backing up ~/.fawx/\n");
    print_summary_item("Config", config_summary(summary));
    print_summary_item("Credentials", credentials_summary(summary));
    print_summary_item(
        "Memory",
        &count_label(summary.breakdown.memory_files, "file", "files"),
    );
    print_summary_item(
        "Context",
        &count_label(summary.breakdown.context_files, "file", "files"),
    );
    print_summary_item(
        "Skills",
        &count_label(summary.breakdown.skill_count, "skill", "skills"),
    );
    print_summary_item(
        "Sessions",
        &count_label(summary.breakdown.session_count, "session", "sessions"),
    );
    println!();
    println!(
        "  ✓ Backup saved: {} ({})",
        display_path_for_user(&summary.archive_path),
        format_bytes(summary.size_bytes)
    );
}

fn config_summary(summary: &BackupSummary) -> &'static str {
    if summary.breakdown.has_config {
        "config.toml"
    } else {
        "not found"
    }
}

fn credentials_summary(summary: &BackupSummary) -> &'static str {
    if summary.breakdown.has_credentials {
        "auth.db (encrypted)"
    } else {
        "not found"
    }
}

fn print_summary_item(label: &str, value: &str) {
    println!("  {label:<12}{value}");
}

fn count_label(count: usize, singular: &str, plural: &str) -> String {
    if count == 1 {
        return format!("1 {singular}");
    }
    format!("{count} {plural}")
}

fn format_bytes(size_bytes: u64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = KIB * 1024.0;
    let size = size_bytes as f64;
    if size >= MIB {
        return format!("{:.1} MB", size / MIB);
    }
    if size >= KIB {
        return format!("{:.1} KB", size / KIB);
    }
    format!("{size_bytes} B")
}
