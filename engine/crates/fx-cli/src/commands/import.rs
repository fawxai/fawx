use crate::commands::slash::display_path_for_user;
use crate::startup::fawx_data_dir;
use anyhow::{anyhow, Context};
use clap::{Args, ValueEnum};
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

const MEMORY_DIR: &str = "memory";
const CONTEXT_DIR: &str = "context";
const ARCHIVE_DIR: &str = "archive";
const ROOT_MEMORY_FILE: &str = "MEMORY.md";
const LABEL_WIDTH: usize = 18;

#[derive(Debug, Clone, Args)]
pub struct ImportArgs {
    /// Source workspace type to import from
    #[arg(long, value_enum)]
    pub from: ImportSourceKind,

    /// Path to the OpenClaw workspace
    #[arg(value_name = "SOURCE_DIR", default_value_os_t = default_openclaw_workspace())]
    pub source_dir: PathBuf,

    /// Show what would be copied without copying
    #[arg(long)]
    pub dry_run: bool,

    /// Overwrite existing files in ~/.fawx/
    #[arg(long)]
    pub force: bool,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, ValueEnum)]
pub enum ImportSourceKind {
    Openclaw,
}

#[derive(Debug, Clone)]
pub(crate) struct ImportOptions {
    pub(crate) source_dir: PathBuf,
    pub(crate) data_dir: PathBuf,
    pub(crate) dry_run: bool,
    pub(crate) force: bool,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum ImportSection {
    Memory,
    Context,
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct ImportJob {
    section: ImportSection,
    label: String,
    source: PathBuf,
    destination: PathBuf,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct ImportOutcome {
    section: ImportSection,
    pub(crate) label: String,
    pub(crate) destination: PathBuf,
    pub(crate) status: ImportStatus,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) enum ImportStatus {
    Planned,
    Copied,
    SkippedExisting,
    Error(String),
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct ImportReport {
    pub(crate) dry_run: bool,
    pub(crate) outcomes: Vec<ImportOutcome>,
}

pub fn run(args: &ImportArgs) -> anyhow::Result<i32> {
    let report = execute_import(&ImportOptions::from_args(args))?;
    print_import_report(&report);
    Ok(i32::from(report.error_count() > 0))
}

impl ImportOptions {
    fn from_args(args: &ImportArgs) -> Self {
        let _source = args.from;
        Self {
            source_dir: args.source_dir.clone(),
            data_dir: fawx_data_dir(),
            dry_run: args.dry_run,
            force: args.force,
        }
    }
}

impl ImportReport {
    pub(crate) fn copied_count(&self) -> usize {
        self.outcomes
            .iter()
            .filter(|outcome| matches!(outcome.status, ImportStatus::Copied))
            .count()
    }

    pub(crate) fn planned_count(&self) -> usize {
        self.outcomes
            .iter()
            .filter(|outcome| matches!(outcome.status, ImportStatus::Planned))
            .count()
    }

    pub(crate) fn skipped_count(&self) -> usize {
        self.outcomes
            .iter()
            .filter(|outcome| matches!(outcome.status, ImportStatus::SkippedExisting))
            .count()
    }

    pub(crate) fn error_count(&self) -> usize {
        self.outcomes
            .iter()
            .filter(|outcome| matches!(outcome.status, ImportStatus::Error(_)))
            .count()
    }

    fn section_count(&self, section: ImportSection) -> usize {
        self.outcomes
            .iter()
            .filter(|outcome| outcome.section == section)
            .filter(|outcome| counts_toward_summary(&outcome.status, self.dry_run))
            .count()
    }
}

impl ImportSection {
    fn title(self) -> &'static str {
        match self {
            Self::Memory => "Memory",
            Self::Context => "Context",
        }
    }
}

fn counts_toward_summary(status: &ImportStatus, dry_run: bool) -> bool {
    match status {
        ImportStatus::Planned => dry_run,
        ImportStatus::Copied => !dry_run,
        ImportStatus::SkippedExisting | ImportStatus::Error(_) => false,
    }
}

fn default_openclaw_workspace() -> PathBuf {
    dirs::home_dir()
        .map(|home| home.join(".openclaw").join("workspace"))
        .unwrap_or_else(|| PathBuf::from(".openclaw/workspace"))
}

pub(crate) fn execute_import(options: &ImportOptions) -> anyhow::Result<ImportReport> {
    validate_source_dir(&options.source_dir)?;
    let jobs = discover_import_jobs(options)?;
    let outcomes = jobs
        .iter()
        .map(|job| process_import_job(job, options))
        .collect();
    Ok(ImportReport {
        dry_run: options.dry_run,
        outcomes,
    })
}

fn validate_source_dir(source_dir: &Path) -> anyhow::Result<()> {
    if source_dir.is_dir() {
        return Ok(());
    }
    Err(anyhow!(
        "Directory not found: {}",
        display_path_for_user(source_dir)
    ))
}

fn discover_import_jobs(options: &ImportOptions) -> anyhow::Result<Vec<ImportJob>> {
    let mut jobs = discover_memory_jobs(options)?;
    jobs.extend(discover_context_jobs(options)?);
    if jobs.is_empty() {
        return Err(anyhow!(
            "No markdown files found in {}. Is this an OpenClaw workspace?",
            display_path_for_user(&options.source_dir)
        ));
    }
    Ok(jobs)
}

fn discover_memory_jobs(options: &ImportOptions) -> anyhow::Result<Vec<ImportJob>> {
    let mut jobs = Vec::new();
    let has_root_memory = root_memory_job(options)
        .map(|job| {
            jobs.push(job);
            true
        })
        .unwrap_or(false);
    jobs.extend(discover_daily_memory_jobs(options, has_root_memory)?);
    jobs.extend(discover_archive_jobs(options)?);
    Ok(jobs)
}

fn root_memory_job(options: &ImportOptions) -> Option<ImportJob> {
    let source = options.source_dir.join(ROOT_MEMORY_FILE);
    source.is_file().then(|| ImportJob {
        section: ImportSection::Memory,
        label: ROOT_MEMORY_FILE.to_string(),
        source,
        destination: options.data_dir.join(MEMORY_DIR).join(ROOT_MEMORY_FILE),
    })
}

fn discover_daily_memory_jobs(
    options: &ImportOptions,
    skip_nested_memory: bool,
) -> anyhow::Result<Vec<ImportJob>> {
    let memory_dir = options.source_dir.join(MEMORY_DIR);
    if !memory_dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut jobs = Vec::new();
    for source in read_markdown_files(&memory_dir)? {
        if skip_nested_memory && source.file_name() == Some(OsStr::new(ROOT_MEMORY_FILE)) {
            continue;
        }
        jobs.push(daily_memory_job(source, &options.data_dir));
    }
    Ok(jobs)
}

fn daily_memory_job(source: PathBuf, data_dir: &Path) -> ImportJob {
    let file_name = source
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    ImportJob {
        section: ImportSection::Memory,
        label: format!("memory/{file_name}"),
        source,
        destination: data_dir.join(MEMORY_DIR).join(&file_name),
    }
}

fn discover_archive_jobs(options: &ImportOptions) -> anyhow::Result<Vec<ImportJob>> {
    let archive_dir = options.source_dir.join(MEMORY_DIR).join(ARCHIVE_DIR);
    if !archive_dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut jobs = Vec::new();
    for source in read_markdown_files_recursive(&archive_dir)? {
        jobs.push(archive_job(source, options)?);
    }
    Ok(jobs)
}

fn archive_job(source: PathBuf, options: &ImportOptions) -> anyhow::Result<ImportJob> {
    let memory_root = options.source_dir.join(MEMORY_DIR);
    let relative = source
        .strip_prefix(&memory_root)
        .with_context(|| format!("failed to relativize {}", source.display()))?
        .to_path_buf();
    Ok(ImportJob {
        section: ImportSection::Memory,
        label: normalize_path(&relative),
        source,
        destination: options.data_dir.join(MEMORY_DIR).join(&relative),
    })
}

fn discover_context_jobs(options: &ImportOptions) -> anyhow::Result<Vec<ImportJob>> {
    let mut jobs = Vec::new();
    for source in read_markdown_files(&options.source_dir)? {
        if source.file_name() == Some(OsStr::new(ROOT_MEMORY_FILE)) {
            continue;
        }
        jobs.push(context_job(source, &options.data_dir));
    }
    Ok(jobs)
}

fn context_job(source: PathBuf, data_dir: &Path) -> ImportJob {
    let file_name = source
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    ImportJob {
        section: ImportSection::Context,
        label: file_name.clone(),
        source,
        destination: data_dir.join(CONTEXT_DIR).join(file_name),
    }
}

fn read_markdown_files(dir: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    let entries = fs::read_dir(dir).with_context(|| format!("failed to read {}", dir.display()))?;
    for entry in entries {
        let path = entry?.path();
        if path.is_file() && is_markdown_file(&path) {
            files.push(path);
        }
    }
    files.sort_by_key(|path| normalize_path(path));
    Ok(files)
}

fn read_markdown_files_recursive(dir: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    let entries = fs::read_dir(dir).with_context(|| format!("failed to read {}", dir.display()))?;
    for entry in entries {
        let path = entry?.path();
        if path.is_dir() {
            files.extend(read_markdown_files_recursive(&path)?);
            continue;
        }
        if path.is_file() && is_markdown_file(&path) {
            files.push(path);
        }
    }
    files.sort_by_key(|path| normalize_path(path));
    Ok(files)
}

fn is_markdown_file(path: &Path) -> bool {
    path.extension().is_some_and(|extension| extension == "md")
}

fn normalize_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn process_import_job(job: &ImportJob, options: &ImportOptions) -> ImportOutcome {
    if options.dry_run {
        return outcome(job, ImportStatus::Planned);
    }
    if should_skip_existing(job, options) {
        return outcome(job, ImportStatus::SkippedExisting);
    }
    match copy_job(job) {
        Ok(()) => outcome(job, ImportStatus::Copied),
        Err(error) => outcome(job, ImportStatus::Error(error.to_string())),
    }
}

fn should_skip_existing(job: &ImportJob, options: &ImportOptions) -> bool {
    job.destination.exists() && !options.force
}

fn copy_job(job: &ImportJob) -> anyhow::Result<()> {
    ensure_destination_parent(&job.destination)?;
    fs::copy(&job.source, &job.destination).with_context(|| {
        format!(
            "failed to copy {} to {}",
            job.source.display(),
            job.destination.display()
        )
    })?;
    Ok(())
}

fn ensure_destination_parent(destination: &Path) -> anyhow::Result<()> {
    let parent = destination.parent().context("destination parent missing")?;
    fs::create_dir_all(parent).with_context(|| format!("failed to create {}", parent.display()))?;
    Ok(())
}

fn outcome(job: &ImportJob, status: ImportStatus) -> ImportOutcome {
    ImportOutcome {
        section: job.section,
        label: job.label.clone(),
        destination: job.destination.clone(),
        status,
    }
}

fn print_import_report(report: &ImportReport) {
    print!("{}", render_import_report(report));
}

pub(crate) fn render_import_report(report: &ImportReport) -> String {
    let mut lines = vec![report_heading(report).to_string(), String::new()];
    lines.extend(render_section(report, ImportSection::Memory));
    lines.push(String::new());
    lines.extend(render_section(report, ImportSection::Context));
    lines.push(String::new());
    lines.push(format!("  {}", summary_line(report)));
    if !report.dry_run {
        lines.push("  Fawx loads all .md files from ~/.fawx/context/ automatically.".to_string());
    }
    lines.push(String::new());
    lines.join("\n")
}

fn report_heading(report: &ImportReport) -> &'static str {
    if report.dry_run {
        "🦊 Import preview (dry run)"
    } else {
        "🦊 Importing from OpenClaw"
    }
}

fn render_section(report: &ImportReport, section: ImportSection) -> Vec<String> {
    let width = label_width(report);
    let mut lines = vec![format!("  {}:", section.title())];
    let outcomes = report
        .outcomes
        .iter()
        .filter(|outcome| outcome.section == section)
        .map(|outcome| format_outcome(outcome, width));
    lines.extend(outcomes);
    lines
}

fn label_width(report: &ImportReport) -> usize {
    report
        .outcomes
        .iter()
        .map(|outcome| outcome.label.len())
        .max()
        .unwrap_or(LABEL_WIDTH)
        .max(LABEL_WIDTH)
}

fn format_outcome(outcome: &ImportOutcome, width: usize) -> String {
    let label = format!("{:<width$}", outcome.label, width = width);
    let destination = display_path_for_user(&outcome.destination);
    match &outcome.status {
        ImportStatus::Planned => format!("    {label} → {destination}"),
        ImportStatus::Copied => format!("    ✓ {label} → {destination}"),
        ImportStatus::SkippedExisting => {
            format!("    ⊘ {label} — already exists (use --force to overwrite)")
        }
        ImportStatus::Error(message) => format!("    ✗ {label} — {message}"),
    }
}

fn summary_line(report: &ImportReport) -> String {
    if report.dry_run {
        return format!(
            "Would import {} files. Run without --dry-run to proceed.",
            report.planned_count()
        );
    }

    let imported = report.copied_count();
    let memory = report.section_count(ImportSection::Memory);
    let context = report.section_count(ImportSection::Context);
    let mut parts = vec![format!(
        "Imported {imported} files ({memory} memory, {context} context)"
    )];
    append_optional_count(&mut parts, report.skipped_count(), "skipped");
    append_optional_count(&mut parts, report.error_count(), "errors");
    format!("{}. Your memory and context are ready.", parts.join(", "))
}

fn append_optional_count(parts: &mut Vec<String>, count: usize, label: &str) {
    if count > 0 {
        parts.push(format!("{count} {label}"));
    }
}
