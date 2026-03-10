use super::diagnostics::{plural_suffix, DiagnosticLine, DiagnosticSection, DiagnosticStatus};
use super::log_files::find_log_files;
use super::runtime_layout::RuntimeLayout;
use super::skill_signatures::{scan_skill_signatures, SkillSignatureReport};
use crate::auth_store::AuthStore;
use clap::Args;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

type AuditLine = DiagnosticLine;
type AuditSection = DiagnosticSection;
type SectionStatus = DiagnosticStatus;

const LOG_PATTERNS: [&str; 4] = ["sk-ant-", "sk-proj-", "Bearer ", "Authorization: Bearer"];
const TIER3_GROUPS: [Tier3Group; 4] = [
    Tier3Group::new("fx-kernel", "engine/crates/fx-kernel/src"),
    Tier3Group::new("fx-auth", "engine/crates/fx-auth/src"),
    Tier3Group::new(".github", ".github"),
    Tier3Group::new("fawx-ripcord", "engine/crates/fawx-ripcord/src"),
];

#[derive(Debug, Clone, Args)]
pub struct SecurityAuditArgs {
    /// Rewrite the stored Tier 3 integrity baseline with current hashes
    #[arg(long)]
    pub update_baseline: bool,
}

pub async fn run(args: &SecurityAuditArgs) -> anyhow::Result<i32> {
    let layout = RuntimeLayout::detect()?;
    let report = SecurityReport::gather(&layout, args.update_baseline).await;
    report.print();
    Ok(report.exit_code())
}

#[derive(Debug, Clone)]
struct SecurityReport {
    sections: Vec<AuditSection>,
}

impl SecurityReport {
    async fn gather(layout: &RuntimeLayout, update_baseline: bool) -> Self {
        Self {
            sections: vec![
                credential_store_section(layout),
                wasm_skills_section(layout),
                tier3_paths_section(layout, update_baseline),
                config_section(layout),
                log_scan_section(layout),
                audit_log_section(layout).await,
            ],
        }
    }

    fn print(&self) {
        println!("Fawx Security Audit\n───────────────────\n");
        for section in &self.sections {
            section.print();
        }
        println!("{}", self.score_line());
    }

    fn exit_code(&self) -> i32 {
        if self.count(SectionStatus::Fail) > 0 {
            1
        } else {
            0
        }
    }

    fn score_line(&self) -> String {
        format!(
            "Score: {}/{} passed, {} warning{}, {} failed",
            self.count(SectionStatus::Pass),
            self.sections.len(),
            self.count(SectionStatus::Warning),
            plural_suffix(self.count(SectionStatus::Warning)),
            self.count(SectionStatus::Fail),
        )
    }

    fn count(&self, status: SectionStatus) -> usize {
        self.sections
            .iter()
            .filter(|section| section.status == status)
            .count()
    }
}

fn credential_store_section(layout: &RuntimeLayout) -> AuditSection {
    let decrypt_line = credential_store_integrity_line(layout);
    let permission_line = file_mode_line(&layout.auth_db_path, 0o600, "File permissions");
    AuditSection::new("Credential Store", vec![decrypt_line, permission_line])
}

fn credential_store_integrity_line(layout: &RuntimeLayout) -> AuditLine {
    if !layout.auth_db_path.exists() {
        return AuditLine::new(SectionStatus::Warning, "Store not found");
    }
    match AuthStore::open(&layout.data_dir).and_then(|store| store.load_auth_manager()) {
        Ok(_) => AuditLine::new(SectionStatus::Pass, "Store exists and decryptable"),
        Err(error) => AuditLine::new(SectionStatus::Fail, format!("Store unreadable: {error}")),
    }
}

fn file_mode_line(path: &Path, expected: u32, label: &str) -> AuditLine {
    match unix_mode(path) {
        Some(mode) if mode == expected => {
            AuditLine::new(SectionStatus::Pass, format!("{label}: {:04o}", mode))
        }
        Some(mode) => AuditLine::new(
            SectionStatus::Fail,
            format!("{label}: {:04o} (expected {:04o})", mode, expected),
        ),
        None if path.exists() => {
            AuditLine::new(SectionStatus::Warning, format!("{label}: unavailable"))
        }
        None => AuditLine::new(SectionStatus::Warning, format!("{label}: file missing")),
    }
}

fn unix_mode(path: &Path) -> Option<u32> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(path).ok()?.permissions().mode() & 0o777;
        Some(mode)
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        None
    }
}

fn wasm_skills_section(layout: &RuntimeLayout) -> AuditSection {
    match scan_skill_signatures(&layout.skills_dir, &layout.trusted_keys_dir) {
        Ok(report) => render_skill_section(report),
        Err(error) => AuditSection::new(
            "WASM Skills",
            vec![AuditLine::new(
                SectionStatus::Fail,
                format!("Scan failed: {error}"),
            )],
        ),
    }
}

fn render_skill_section(report: SkillSignatureReport) -> AuditSection {
    let mut lines = Vec::new();
    lines.extend(named_lines(
        &report.verified,
        SectionStatus::Pass,
        "signature valid",
    ));
    lines.extend(named_lines(
        &report.unsigned,
        SectionStatus::Warning,
        "no signature file",
    ));
    lines.extend(named_lines(
        &report.unverified,
        SectionStatus::Warning,
        "signature present but no trusted keys configured",
    ));
    lines.extend(named_lines(
        &report.invalid,
        SectionStatus::Fail,
        "signature invalid",
    ));
    let footer = Some(format!(
        "Total: {} verified, {} unsigned, {} unverified, {} invalid",
        report.verified.len(),
        report.unsigned.len(),
        report.unverified.len(),
        report.invalid.len(),
    ));
    AuditSection::with_footer("WASM Skills", lines, footer)
}

fn named_lines(names: &[String], status: SectionStatus, suffix: &str) -> Vec<AuditLine> {
    names
        .iter()
        .map(|name| AuditLine::new(status, format!("{name}: {suffix}")))
        .collect()
}

fn tier3_paths_section(layout: &RuntimeLayout, update_baseline: bool) -> AuditSection {
    match baseline_section_data(layout, update_baseline) {
        Ok(section) => section,
        Err(error) => AuditSection::new(
            "Tier 3 Paths",
            vec![AuditLine::new(
                SectionStatus::Fail,
                format!("Baseline check failed: {error}"),
            )],
        ),
    }
}

fn baseline_section_data(
    layout: &RuntimeLayout,
    update_baseline: bool,
) -> anyhow::Result<AuditSection> {
    let current = current_baseline(&layout.repo_root)?;
    if update_baseline {
        write_baseline(&layout.security_baseline_path, &current)?;
        return Ok(created_baseline_section(
            "Baseline updated",
            &layout.security_baseline_path,
        ));
    }
    let Some(previous) = read_baseline(&layout.security_baseline_path)? else {
        write_baseline(&layout.security_baseline_path, &current)?;
        return Ok(created_baseline_section(
            "No baseline found",
            &layout.security_baseline_path,
        ));
    };
    Ok(compare_baseline(previous, current))
}

fn created_baseline_section(message: &str, path: &Path) -> AuditSection {
    AuditSection::new(
        "Tier 3 Paths",
        vec![AuditLine::new(
            SectionStatus::Warning,
            format!("{message} — created at {}", path.display()),
        )],
    )
}

fn compare_baseline(previous: SecurityBaseline, current: SecurityBaseline) -> AuditSection {
    let mut lines = Vec::new();
    for group in TIER3_GROUPS {
        lines.push(compare_group(group, &previous.files, &current.files));
    }
    AuditSection::new("Tier 3 Paths", lines)
}

fn compare_group(
    group: Tier3Group,
    previous: &BTreeMap<String, String>,
    current: &BTreeMap<String, String>,
) -> AuditLine {
    let current_files = group_files(current, group.relative_dir);
    let previous_files = group_files(previous, group.relative_dir);
    if current_files == previous_files {
        return AuditLine::new(
            SectionStatus::Pass,
            format!(
                "{}: {} files, hashes match baseline",
                group.label,
                current_files.len()
            ),
        );
    }
    AuditLine::new(
        SectionStatus::Fail,
        format!(
            "{}: {} files differ from baseline",
            group.label,
            diff_count(&previous_files, &current_files)
        ),
    )
}

fn group_files(files: &BTreeMap<String, String>, prefix: &str) -> BTreeMap<String, String> {
    files
        .iter()
        .filter(|(path, _)| path.starts_with(prefix))
        .map(|(path, hash)| (path.clone(), hash.clone()))
        .collect()
}

fn diff_count(previous: &BTreeMap<String, String>, current: &BTreeMap<String, String>) -> usize {
    let removed = previous
        .keys()
        .filter(|path| !current.contains_key(*path))
        .count();
    let added = current
        .keys()
        .filter(|path| !previous.contains_key(*path))
        .count();
    let changed = current
        .iter()
        .filter(|(path, hash)| previous.get(*path).is_some_and(|prior| prior != *hash))
        .count();
    removed + added + changed
}

#[derive(Debug, Clone, Copy)]
struct Tier3Group {
    label: &'static str,
    relative_dir: &'static str,
}

impl Tier3Group {
    const fn new(label: &'static str, relative_dir: &'static str) -> Self {
        Self {
            label,
            relative_dir,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct SecurityBaseline {
    created: String,
    files: BTreeMap<String, String>,
}

fn current_baseline(repo_root: &Path) -> anyhow::Result<SecurityBaseline> {
    let mut files = BTreeMap::new();
    for group in TIER3_GROUPS {
        hash_group(repo_root, group.relative_dir, &mut files)?;
    }
    Ok(SecurityBaseline {
        created: chrono::Utc::now().to_rfc3339(),
        files,
    })
}

fn hash_group(
    repo_root: &Path,
    relative_dir: &str,
    files: &mut BTreeMap<String, String>,
) -> anyhow::Result<()> {
    let root = repo_root.join(relative_dir);
    if !root.exists() {
        anyhow::bail!("tier 3 path missing: {}", root.display());
    }
    for file in collect_files(&root)? {
        let relative = file
            .strip_prefix(repo_root)
            .map_err(|error| anyhow::anyhow!(error))?
            .to_string_lossy()
            .to_string();
        files.insert(relative, hash_file(&file)?);
    }
    Ok(())
}

fn collect_files(root: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    collect_files_into(root, root, &mut files)?;
    files.sort();
    Ok(files)
}

fn collect_files_into(
    scan_root: &Path,
    current_dir: &Path,
    files: &mut Vec<PathBuf>,
) -> anyhow::Result<()> {
    for entry in fs::read_dir(current_dir)? {
        let path = entry?.path();
        if should_skip_entry(scan_root, &path) {
            continue;
        }
        if path.is_dir() {
            collect_files_into(scan_root, &path, files)?;
        } else if path.is_file() {
            files.push(path);
        }
    }
    Ok(())
}

fn should_skip_entry(scan_root: &Path, path: &Path) -> bool {
    if path == scan_root {
        return false;
    }
    let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
        return false;
    };
    path.is_dir() && is_hidden_or_build_dir(name)
}

fn is_hidden_or_build_dir(name: &str) -> bool {
    matches!(name, "target" | "node_modules") || name.starts_with('.')
}

fn hash_file(path: &Path) -> anyhow::Result<String> {
    let bytes = fs::read(path)?;
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    Ok(format!("sha256:{:x}", hasher.finalize()))
}

fn read_baseline(path: &Path) -> anyhow::Result<Option<SecurityBaseline>> {
    if !path.exists() {
        return Ok(None);
    }
    let content = fs::read_to_string(path)?;
    let baseline = serde_json::from_str(&content)?;
    Ok(Some(baseline))
}

fn write_baseline(path: &Path, baseline: &SecurityBaseline) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(baseline)?;
    fs::write(path, json)?;
    Ok(())
}

fn config_section(layout: &RuntimeLayout) -> AuditSection {
    let line = config_permissions_line(&layout.config_path);
    AuditSection::new("Config", vec![line])
}

fn config_permissions_line(path: &Path) -> AuditLine {
    match unix_mode(path) {
        Some(mode) if mode & 0o004 == 0 => AuditLine::new(
            SectionStatus::Pass,
            format!("config.toml permissions: {:04o}", mode),
        ),
        Some(mode) => AuditLine::new(
            SectionStatus::Warning,
            format!("config.toml permissions: {:04o} (world-readable)", mode),
        ),
        None if path.exists() => AuditLine::new(
            SectionStatus::Warning,
            "config.toml permissions unavailable",
        ),
        None => AuditLine::new(SectionStatus::Warning, "config.toml not found"),
    }
}

fn log_scan_section(layout: &RuntimeLayout) -> AuditSection {
    let matches = scan_logs_for_patterns(&layout.logs_dir, &LOG_PATTERNS).unwrap_or_default();
    let line = if matches.is_empty() {
        AuditLine::new(SectionStatus::Pass, "No credential patterns found in logs")
    } else {
        AuditLine::new(
            SectionStatus::Fail,
            format!(
                "Potential credential leaks found in {} log files",
                matches.len()
            ),
        )
    };
    AuditSection::new("Log Scan", vec![line])
}

fn scan_logs_for_patterns(logs_dir: &Path, patterns: &[&str]) -> anyhow::Result<Vec<PathBuf>> {
    let mut flagged = Vec::new();
    for file in find_log_files(logs_dir)? {
        if file_contains_patterns(&file, patterns)? {
            flagged.push(file);
        }
    }
    Ok(flagged)
}

fn file_contains_patterns(path: &Path, patterns: &[&str]) -> anyhow::Result<bool> {
    let content = fs::read_to_string(path)?;
    Ok(patterns.iter().any(|pattern| content.contains(pattern)))
}

async fn audit_log_section(layout: &RuntimeLayout) -> AuditSection {
    let line = if !layout.audit_log_path.exists() {
        AuditLine::new(SectionStatus::Warning, "Audit log not found")
    } else {
        match fx_security::AuditLog::open(&layout.audit_log_path).await {
            Ok(log) if log.verify_integrity().unwrap_or(false) => AuditLine::new(
                SectionStatus::Pass,
                format!("{} entries, HMAC chain valid", log.count()),
            ),
            Ok(log) => AuditLine::new(
                SectionStatus::Fail,
                format!("{} entries, HMAC chain invalid", log.count()),
            ),
            Err(error) => AuditLine::new(
                SectionStatus::Fail,
                format!("Audit log unreadable: {error}"),
            ),
        }
    };
    AuditSection::new("Audit Log", vec![line])
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn baseline_creation_roundtrip() {
        let temp = TempDir::new().expect("tempdir");
        let baseline = SecurityBaseline {
            created: "2026-03-10T02:00:00Z".to_string(),
            files: BTreeMap::from([("a.txt".to_string(), "sha256:abc".to_string())]),
        };
        let path = temp.path().join("baseline.json");
        write_baseline(&path, &baseline).expect("write baseline");
        let loaded = read_baseline(&path)
            .expect("read baseline")
            .expect("baseline");
        assert_eq!(loaded, baseline);
    }

    #[test]
    fn baseline_comparison_detects_match_and_mismatch() {
        let previous = SecurityBaseline {
            created: "1".to_string(),
            files: BTreeMap::from([(
                "engine/crates/fx-kernel/src/lib.rs".to_string(),
                "sha256:a".to_string(),
            )]),
        };
        let current = previous.clone();
        let matched = compare_baseline(previous.clone(), current);
        assert_eq!(matched.status, SectionStatus::Pass);

        let changed = SecurityBaseline {
            created: "2".to_string(),
            files: BTreeMap::from([(
                "engine/crates/fx-kernel/src/lib.rs".to_string(),
                "sha256:b".to_string(),
            )]),
        };
        let mismatched = compare_baseline(previous, changed);
        assert_eq!(mismatched.status, SectionStatus::Fail);
    }

    #[test]
    fn credential_pattern_scanning_detects_secret_like_text() {
        let temp = TempDir::new().expect("tempdir");
        let log = temp.path().join("fawx.log");
        fs::write(&log, "Authorization: Bearer secret").expect("write log");
        let matches = scan_logs_for_patterns(temp.path(), &LOG_PATTERNS).expect("scan logs");
        assert_eq!(matches, vec![log]);
    }

    #[test]
    fn credential_pattern_scanning_ignores_clean_logs() {
        let temp = TempDir::new().expect("tempdir");
        fs::write(temp.path().join("fawx.log"), "all clear").expect("write log");
        let matches = scan_logs_for_patterns(temp.path(), &LOG_PATTERNS).expect("scan logs");
        assert!(matches.is_empty());
    }

    #[test]
    fn collect_files_skips_hidden_and_build_directories() {
        let temp = TempDir::new().expect("tempdir");
        let root = temp.path().join("engine/crates/fx-kernel/src");
        fs::create_dir_all(root.join("nested")).expect("nested dir");
        fs::create_dir_all(root.join(".git")).expect("git dir");
        fs::create_dir_all(root.join("target")).expect("target dir");
        fs::write(root.join("nested/lib.rs"), "pub fn ok() {}\n").expect("source file");
        fs::write(root.join(".git/config"), "ignored\n").expect("git file");
        fs::write(root.join("target/build.rs"), "ignored\n").expect("target file");

        let files = collect_files(&root).expect("collect files");

        assert_eq!(files, vec![root.join("nested/lib.rs")]);
    }

    #[test]
    fn current_baseline_fails_when_a_tier3_path_is_missing() {
        let temp = TempDir::new().expect("tempdir");
        let error = current_baseline(temp.path()).expect_err("missing tier3 path");
        assert!(error.to_string().contains("tier 3 path missing"));
    }

    #[cfg(unix)]
    #[test]
    fn permission_check_reports_exact_mode() {
        use std::os::unix::fs::PermissionsExt;

        let temp = TempDir::new().expect("tempdir");
        let path = temp.path().join("auth.db");
        fs::write(&path, "db").expect("write file");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).expect("chmod");
        let line = file_mode_line(&path, 0o600, "File permissions");
        assert_eq!(line.status, SectionStatus::Pass);
    }

    #[test]
    fn output_format_score_line_includes_warning_count() {
        let report = SecurityReport {
            sections: vec![
                AuditSection {
                    title: "A",
                    status: SectionStatus::Pass,
                    lines: Vec::new(),
                    footer: None,
                },
                AuditSection {
                    title: "B",
                    status: SectionStatus::Warning,
                    lines: Vec::new(),
                    footer: None,
                },
            ],
        };
        assert_eq!(
            report.score_line(),
            "Score: 1/2 passed, 1 warning, 0 failed"
        );
    }
}
