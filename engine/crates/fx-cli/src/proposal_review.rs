use fx_kernel::is_tier3_path;
use fx_propose::{checked_target_path, current_file_hash, ProposalSidecar};
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub(crate) struct ReviewContext {
    pub proposals_dir: PathBuf,
    pub working_dir: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct StoredProposal {
    stem: String,
    title: String,
    target_path: PathBuf,
    proposed_content: String,
    risk: String,
    timestamp: u64,
    file_hash_at_creation: Option<String>,
    markdown_path: PathBuf,
    sidecar_path: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProposalLoadFailure {
    file_name: String,
    error: String,
}

#[derive(Debug, Default)]
struct PendingProposals {
    proposals: Vec<StoredProposal>,
    failures: Vec<ProposalLoadFailure>,
}

struct ResolvedTarget {
    absolute_path: PathBuf,
    policy_path: String,
}

#[derive(Debug)]
pub(crate) enum ProposalReviewError {
    Io(io::Error),
    Parse(String),
    NotFound(String),
    Ambiguous(String),
}

impl fmt::Display for ProposalReviewError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "{error}"),
            Self::Parse(message) | Self::NotFound(message) | Self::Ambiguous(message) => {
                write!(f, "{message}")
            }
        }
    }
}

impl std::error::Error for ProposalReviewError {}

impl From<io::Error> for ProposalReviewError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

pub(crate) fn render_pending(context: ReviewContext) -> Result<String, ProposalReviewError> {
    let pending = load_pending_proposals(&context.proposals_dir)?;
    if pending.proposals.is_empty() && pending.failures.is_empty() {
        return Ok("No pending proposals.".to_string());
    }

    let mut lines = vec!["Pending proposals:".to_string()];
    append_pending_proposals(&mut lines, &pending.proposals);
    append_parse_failures(&mut lines, &pending.failures);
    if !pending.proposals.is_empty() {
        lines.push("".to_string());
        lines.push("Use /approve <number> or /reject <number>".to_string());
    }
    Ok(lines.join("\n"))
}

#[must_use = "approval result includes the user-facing outcome message"]
pub(crate) fn approve_pending(
    context: ReviewContext,
    selector: &str,
    force: bool,
) -> Result<String, ProposalReviewError> {
    let proposal = resolve_pending_proposal(&context.proposals_dir, selector)?;
    let resolved_target = resolve_review_target(&proposal.target_path, &context.working_dir)?;
    if is_tier3_path(&resolved_target.policy_path) {
        return Ok(format!(
            "Cannot apply: {} is Tier 3 (kernel immutable)",
            proposal.target_path.display()
        ));
    }
    if is_stale(&proposal, &context.working_dir)? && !force {
        return Ok(format!(
            "⚠ Target file changed since proposal was created.\nUse /approve {selector} --force to apply anyway."
        ));
    }

    // NOTE: There is an unavoidable TOCTOU window between the staleness check
    // above and the later write because this approval flow operates on paths,
    // not open file descriptors.
    write_approved_content(&proposal, &resolved_target.absolute_path)?;
    archive_proposal(&context.proposals_dir, &proposal, "applied")?;
    Ok(format!(
        "✓ Applied proposal: {} → {}",
        proposal.title,
        proposal.target_path.display()
    ))
}

#[must_use = "rejection result includes the user-facing outcome message"]
pub(crate) fn reject_pending(
    context: ReviewContext,
    selector: &str,
) -> Result<String, ProposalReviewError> {
    let proposal = resolve_pending_proposal(&context.proposals_dir, selector)?;
    archive_proposal(&context.proposals_dir, &proposal, "rejected")?;
    Ok(format!("✗ Rejected proposal: {}", proposal.title))
}

fn load_pending_proposals(proposals_dir: &Path) -> Result<PendingProposals, ProposalReviewError> {
    if !proposals_dir.exists() {
        return Ok(PendingProposals::default());
    }

    let mut pending = PendingProposals::default();
    for entry in fs::read_dir(proposals_dir)? {
        let path = entry?.path();
        if !is_markdown_proposal(&path) {
            continue;
        }
        match load_single_proposal(&path) {
            Ok(proposal) => pending.proposals.push(proposal),
            Err(error) => pending.failures.push(proposal_load_failure(&path, error)),
        }
    }
    pending.proposals.sort_by(|left, right| {
        left.timestamp
            .cmp(&right.timestamp)
            .then_with(|| left.stem.cmp(&right.stem))
    });
    pending
        .failures
        .sort_by(|left, right| left.file_name.cmp(&right.file_name));
    Ok(pending)
}

fn append_pending_proposals(lines: &mut Vec<String>, proposals: &[StoredProposal]) {
    for (index, proposal) in proposals.iter().enumerate() {
        lines.push(format!(
            "  [{}] {} — {} (risk: {})",
            index + 1,
            proposal.stem,
            proposal.title,
            proposal.risk
        ));
    }
}

fn append_parse_failures(lines: &mut Vec<String>, failures: &[ProposalLoadFailure]) {
    if failures.is_empty() {
        return;
    }
    if lines.len() > 1 {
        lines.push(String::new());
    }
    for failure in failures {
        lines.push(format!(
            "  ⚠ {} — could not parse: {}",
            failure.file_name, failure.error
        ));
    }
}

fn proposal_load_failure(path: &Path, error: ProposalReviewError) -> ProposalLoadFailure {
    ProposalLoadFailure {
        file_name: path
            .file_name()
            .and_then(|name| name.to_str())
            .map(ToString::to_string)
            .unwrap_or_else(|| path.display().to_string()),
        error: error.to_string(),
    }
}

fn is_markdown_proposal(path: &Path) -> bool {
    path.is_file() && path.extension().and_then(|ext| ext.to_str()) == Some("md")
}

fn load_single_proposal(markdown_path: &Path) -> Result<StoredProposal, ProposalReviewError> {
    let stem = proposal_stem(markdown_path)?;
    let sidecar_path = markdown_path.with_extension("json");
    if sidecar_path.exists() {
        return load_sidecar_proposal(markdown_path, &sidecar_path, &stem);
    }
    load_legacy_proposal(markdown_path, &stem)
}

fn proposal_stem(markdown_path: &Path) -> Result<String, ProposalReviewError> {
    markdown_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .map(ToString::to_string)
        .ok_or_else(|| {
            ProposalReviewError::Parse(format!(
                "invalid proposal filename: {}",
                markdown_path.display()
            ))
        })
}

fn load_sidecar_proposal(
    markdown_path: &Path,
    sidecar_path: &Path,
    stem: &str,
) -> Result<StoredProposal, ProposalReviewError> {
    let content = fs::read_to_string(sidecar_path)?;
    let sidecar: ProposalSidecar = serde_json::from_str(&content).map_err(|error| {
        ProposalReviewError::Parse(format!(
            "invalid proposal sidecar {}: {error}",
            sidecar_path.display()
        ))
    })?;
    Ok(StoredProposal {
        stem: stem.to_string(),
        title: sidecar.title,
        target_path: PathBuf::from(sidecar.target_path),
        proposed_content: sidecar.proposed_content,
        risk: sidecar.risk,
        timestamp: sidecar.timestamp,
        file_hash_at_creation: sidecar.file_hash_at_creation,
        markdown_path: markdown_path.to_path_buf(),
        sidecar_path: Some(sidecar_path.to_path_buf()),
    })
}

fn load_legacy_proposal(
    markdown_path: &Path,
    stem: &str,
) -> Result<StoredProposal, ProposalReviewError> {
    let content = fs::read_to_string(markdown_path)?;
    let lines: Vec<&str> = content.lines().collect();
    let (target_path, proposed_content) = parse_legacy_diff(&lines)?;
    Ok(StoredProposal {
        stem: stem.to_string(),
        title: parse_legacy_title(&lines)?,
        target_path,
        proposed_content,
        risk: parse_legacy_risk(&lines),
        timestamp: parse_timestamp(stem),
        file_hash_at_creation: None,
        markdown_path: markdown_path.to_path_buf(),
        sidecar_path: None,
    })
}

fn parse_legacy_title(lines: &[&str]) -> Result<String, ProposalReviewError> {
    lines
        .iter()
        .find_map(|line| line.strip_prefix("# Proposal: "))
        .map(ToString::to_string)
        .ok_or_else(|| ProposalReviewError::Parse("legacy proposal missing title".to_string()))
}

fn parse_legacy_diff(lines: &[&str]) -> Result<(PathBuf, String), ProposalReviewError> {
    let section = section_index(lines, "## Proposed Diff")?;
    let section_end = section_end_index(lines, section + 1);
    let target = legacy_diff_target(lines, section)?;
    let (start, fence) = opening_diff_fence(lines, section, section_end)?;
    let end = closing_fence_index(lines, start + 1, section_end, &fence)?;
    Ok((PathBuf::from(target), lines[start + 1..end].join("\n")))
}

fn legacy_diff_target<'a>(
    lines: &'a [&str],
    section: usize,
) -> Result<&'a str, ProposalReviewError> {
    next_non_empty_line(lines, section + 1)?
        .trim()
        .strip_suffix(':')
        .ok_or_else(|| {
            ProposalReviewError::Parse("legacy proposal missing target path".to_string())
        })
}

fn opening_diff_fence(
    lines: &[&str],
    section: usize,
    section_end: usize,
) -> Result<(usize, String), ProposalReviewError> {
    let start = next_non_empty_index(lines, section + 2)?;
    if start >= section_end {
        return Err(ProposalReviewError::Parse(
            "legacy proposal diff fence missing".to_string(),
        ));
    }
    let fence = lines[start].trim();
    if !fence.starts_with("```") {
        return Err(ProposalReviewError::Parse(
            "legacy proposal diff fence missing".to_string(),
        ));
    }
    Ok((start, fence.to_string()))
}

fn parse_legacy_risk(lines: &[&str]) -> String {
    section_index(lines, "## Risk")
        .ok()
        .and_then(|index| next_non_empty_line(lines, index + 1).ok())
        .map(ToString::to_string)
        .unwrap_or_else(|| "unknown".to_string())
}

fn section_index(lines: &[&str], section: &str) -> Result<usize, ProposalReviewError> {
    lines
        .iter()
        .position(|line| line.trim() == section)
        .ok_or_else(|| ProposalReviewError::Parse(format!("proposal missing section {section}")))
}

fn next_non_empty_index(lines: &[&str], start: usize) -> Result<usize, ProposalReviewError> {
    lines
        .iter()
        .enumerate()
        .skip(start)
        .find(|(_, line)| !line.trim().is_empty())
        .map(|(index, _)| index)
        .ok_or_else(|| ProposalReviewError::Parse("proposal missing expected content".to_string()))
}

fn next_non_empty_line<'a>(
    lines: &'a [&str],
    start: usize,
) -> Result<&'a str, ProposalReviewError> {
    let index = next_non_empty_index(lines, start)?;
    Ok(lines[index])
}

fn section_end_index(lines: &[&str], start: usize) -> usize {
    lines
        .iter()
        .enumerate()
        .skip(start)
        .find(|(_, line)| line.trim_start().starts_with("## "))
        .map(|(index, _)| index)
        .unwrap_or(lines.len())
}

fn closing_fence_index(
    lines: &[&str],
    start: usize,
    section_end: usize,
    fence: &str,
) -> Result<usize, ProposalReviewError> {
    lines[start..section_end]
        .iter()
        .enumerate()
        .rev()
        .find(|(_, line)| line.trim() == fence)
        .map(|(index, _)| start + index)
        .ok_or_else(|| ProposalReviewError::Parse("proposal diff fence never closed".to_string()))
}

fn parse_timestamp(stem: &str) -> u64 {
    let Some(value) = stem.split('-').next() else {
        tracing::warn!(stem, "proposal stem missing timestamp prefix");
        return 0;
    };
    match value.parse::<u64>() {
        Ok(timestamp) => timestamp,
        Err(error) => {
            tracing::warn!(stem, %error, "failed to parse proposal timestamp prefix");
            0
        }
    }
}

fn resolve_pending_proposal(
    proposals_dir: &Path,
    selector: &str,
) -> Result<StoredProposal, ProposalReviewError> {
    let proposals = load_pending_proposals(proposals_dir)?.proposals;
    if proposals.is_empty() {
        return Err(ProposalReviewError::NotFound(
            "No pending proposals.".to_string(),
        ));
    }
    if let Some(proposal) = select_by_index(&proposals, selector) {
        return Ok(proposal.clone());
    }
    select_by_prefix(&proposals, selector)
}

fn select_by_index<'a>(
    proposals: &'a [StoredProposal],
    selector: &str,
) -> Option<&'a StoredProposal> {
    let index = selector.parse::<usize>().ok()?;
    proposals.get(index.checked_sub(1)?)
}

fn select_by_prefix(
    proposals: &[StoredProposal],
    selector: &str,
) -> Result<StoredProposal, ProposalReviewError> {
    let matches = proposals
        .iter()
        .filter(|proposal| proposal.stem.starts_with(selector))
        .cloned()
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [] => Err(ProposalReviewError::NotFound(format!(
            "No pending proposal matches '{selector}'."
        ))),
        [proposal] => Ok(proposal.clone()),
        _ => Err(ProposalReviewError::Ambiguous(format!(
            "Multiple proposals match '{selector}'. Use the list number instead."
        ))),
    }
}

fn resolve_review_target(
    target_path: &Path,
    working_dir: &Path,
) -> Result<ResolvedTarget, ProposalReviewError> {
    let absolute_path = checked_target_path(working_dir, target_path)?;
    let policy_path = policy_path(&absolute_path, working_dir)?;
    Ok(ResolvedTarget {
        absolute_path,
        policy_path,
    })
}

fn policy_path(target_path: &Path, working_dir: &Path) -> Result<String, ProposalReviewError> {
    let canonical_working_dir = fs::canonicalize(working_dir)?;
    let relative = target_path
        .strip_prefix(&canonical_working_dir)
        .map_err(|_| {
            ProposalReviewError::Parse(format!(
                "target path escapes working directory: {}",
                target_path.display()
            ))
        })?;
    Ok(relative.display().to_string())
}

fn is_stale(proposal: &StoredProposal, working_dir: &Path) -> Result<bool, ProposalReviewError> {
    let Some(expected_hash) = proposal.file_hash_at_creation.as_ref() else {
        return Ok(false);
    };
    let current_hash = current_file_hash(working_dir, &proposal.target_path)?;
    Ok(current_hash.as_deref() != Some(expected_hash.as_str()))
}

fn write_approved_content(
    proposal: &StoredProposal,
    target_path: &Path,
) -> Result<(), ProposalReviewError> {
    if let Some(parent) = target_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(target_path, &proposal.proposed_content)?;
    Ok(())
}

fn archive_proposal(
    proposals_dir: &Path,
    proposal: &StoredProposal,
    destination: &str,
) -> Result<(), ProposalReviewError> {
    let archive_dir = proposals_dir.join(destination);
    fs::create_dir_all(&archive_dir)?;
    move_file(&proposal.markdown_path, &archive_dir)?;
    if let Some(sidecar_path) = &proposal.sidecar_path {
        move_file(sidecar_path, &archive_dir)?;
    }
    Ok(())
}

fn move_file(source: &Path, destination_dir: &Path) -> Result<(), ProposalReviewError> {
    let file_name = source.file_name().ok_or_else(|| {
        ProposalReviewError::Parse(format!("invalid proposal path: {}", source.display()))
    })?;
    fs::rename(source, destination_dir.join(file_name))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};
    use tempfile::TempDir;

    fn unique_timestamp() -> u64 {
        match SystemTime::now().duration_since(UNIX_EPOCH) {
            Ok(duration) => duration.as_secs(),
            Err(_) => 0,
        }
    }

    fn write_sidecar_proposal(
        proposals_dir: &Path,
        stem: &str,
        target_path: &Path,
        content: &str,
        file_hash: Option<String>,
    ) {
        let markdown = format!(
            "# Proposal: Update {}\n\n## What and Why\nTest\n\n## Proposed Diff\n{}:\n```\n{}\n```\n\n## Risk\nlow\n",
            target_path.display(),
            target_path.display(),
            content
        );
        fs::write(proposals_dir.join(format!("{stem}.md")), markdown).expect("write markdown");
        let sidecar = ProposalSidecar {
            version: 1,
            timestamp: parse_timestamp(stem),
            title: format!("Update {}", target_path.display()),
            description: "Test".to_string(),
            target_path: target_path.display().to_string(),
            proposed_content: content.to_string(),
            risk: "low".to_string(),
            file_hash_at_creation: file_hash,
        };
        let sidecar_json = serde_json::to_string_pretty(&sidecar).expect("serialize sidecar");
        fs::write(proposals_dir.join(format!("{stem}.json")), sidecar_json).expect("write sidecar");
    }

    #[test]
    fn render_pending_lists_numbered_proposals() {
        let temp = TempDir::new().expect("tempdir");
        let proposals_dir = temp.path().join("proposals");
        fs::create_dir_all(&proposals_dir).expect("create proposals dir");
        write_sidecar_proposal(
            &proposals_dir,
            "1710000000-first",
            Path::new("config/a.toml"),
            "a = 1",
            None,
        );
        write_sidecar_proposal(
            &proposals_dir,
            "1710000100-second",
            Path::new("config/b.toml"),
            "b = 2",
            None,
        );

        let output = render_pending(ReviewContext {
            proposals_dir: proposals_dir.clone(),
            working_dir: temp.path().to_path_buf(),
        })
        .expect("render pending");

        assert!(output.contains("[1] 1710000000-first — Update config/a.toml (risk: low)"));
        assert!(output.contains("[2] 1710000100-second — Update config/b.toml (risk: low)"));
    }

    #[test]
    fn render_pending_keeps_listing_when_legacy_proposal_is_malformed() {
        let temp = TempDir::new().expect("tempdir");
        let proposals_dir = temp.path().join("proposals");
        fs::create_dir_all(&proposals_dir).expect("create proposals dir");
        write_sidecar_proposal(
            &proposals_dir,
            "1710000000-valid",
            Path::new("config/a.toml"),
            "a = 1",
            None,
        );

        let malformed = concat!(
            "# Proposal: Legacy change

",
            "## What and Why
Test

",
            "## Proposed Diff
",
            "config/legacy.toml:
",
            "legacy = true

",
            "## Risk
low
"
        );
        fs::write(proposals_dir.join("1710000001-malformed.md"), malformed)
            .expect("write malformed proposal");

        let output = render_pending(ReviewContext {
            proposals_dir: proposals_dir.clone(),
            working_dir: temp.path().to_path_buf(),
        })
        .expect("render pending");

        assert!(output.contains("[1] 1710000000-valid — Update config/a.toml (risk: low)"));
        assert!(output.contains(
            "⚠ 1710000001-malformed.md — could not parse: legacy proposal diff fence missing"
        ));
        assert!(output.contains("Use /approve <number> or /reject <number>"));
    }

    #[test]
    fn approve_requires_force_when_target_hash_changed() {
        let temp = TempDir::new().expect("tempdir");
        let working_dir = temp.path().join("repo");
        let proposals_dir = temp.path().join("proposals");
        fs::create_dir_all(&working_dir).expect("create working dir");
        fs::create_dir_all(&proposals_dir).expect("create proposals dir");

        let target = working_dir.join("config/settings.toml");
        fs::create_dir_all(target.parent().expect("target parent")).expect("create target parent");
        fs::write(&target, "old = true\n").expect("write original target");
        let original_hash = current_file_hash(&working_dir, Path::new("config/settings.toml"))
            .expect("hash original")
            .expect("hash exists");
        fs::write(&target, "new = false\n").expect("mutate target");

        write_sidecar_proposal(
            &proposals_dir,
            "1710000200-stale",
            Path::new("config/settings.toml"),
            "approved = true\n",
            Some(original_hash),
        );

        let output = approve_pending(
            ReviewContext {
                proposals_dir: proposals_dir.clone(),
                working_dir: working_dir.clone(),
            },
            "1",
            false,
        )
        .expect("approve result");

        assert!(output.contains("Target file changed since proposal was created"));
        let written = fs::read_to_string(&target).expect("read unchanged target");
        assert_eq!(written, "new = false\n");
    }

    #[test]
    fn approve_force_applies_and_archives_sidecar() {
        let temp = TempDir::new().expect("tempdir");
        let working_dir = temp.path().join("repo");
        let proposals_dir = temp.path().join("proposals");
        fs::create_dir_all(&working_dir).expect("create working dir");
        fs::create_dir_all(&proposals_dir).expect("create proposals dir");

        let target = working_dir.join("config/settings.toml");
        fs::create_dir_all(target.parent().expect("target parent")).expect("create target parent");
        fs::write(&target, "old = true\n").expect("write original target");
        let original_hash = current_file_hash(&working_dir, Path::new("config/settings.toml"))
            .expect("hash original")
            .expect("hash exists");
        fs::write(&target, "changed = true\n").expect("mutate target");

        write_sidecar_proposal(
            &proposals_dir,
            "1710000300-apply",
            Path::new("config/settings.toml"),
            "approved = true\n",
            Some(original_hash),
        );

        let output = approve_pending(
            ReviewContext {
                proposals_dir: proposals_dir.clone(),
                working_dir: working_dir.clone(),
            },
            "1",
            true,
        )
        .expect("approve result");

        assert!(output.contains("✓ Applied proposal"));
        let written = fs::read_to_string(&target).expect("read updated target");
        assert_eq!(written, "approved = true\n");
        assert!(proposals_dir.join("applied/1710000300-apply.md").exists());
        assert!(proposals_dir.join("applied/1710000300-apply.json").exists());
    }

    #[test]
    fn approve_rejects_tier3_targets() {
        let temp = TempDir::new().expect("tempdir");
        let proposals_dir = temp.path().join("proposals");
        fs::create_dir_all(&proposals_dir).expect("create proposals dir");
        write_sidecar_proposal(
            &proposals_dir,
            "1710000400-kernel",
            Path::new("engine/crates/fx-kernel/src/lib.rs"),
            "bad",
            None,
        );

        let output = approve_pending(
            ReviewContext {
                proposals_dir: proposals_dir.clone(),
                working_dir: temp.path().to_path_buf(),
            },
            "1",
            false,
        )
        .expect("approve result");

        assert!(output.contains("Tier 3"));
        assert!(proposals_dir.join("1710000400-kernel.md").exists());
    }

    #[test]
    fn approve_rejects_absolute_tier3_targets() {
        let temp = TempDir::new().expect("tempdir");
        let working_dir = temp.path().join("repo");
        let proposals_dir = temp.path().join("proposals");
        let target = working_dir.join("engine/crates/fx-kernel/src/lib.rs");
        fs::create_dir_all(target.parent().expect("parent")).expect("mkdir");
        fs::create_dir_all(&proposals_dir).expect("create proposals dir");
        write_sidecar_proposal(
            &proposals_dir,
            "1710000450-kernel-absolute",
            &target,
            "bad",
            None,
        );

        let output = approve_pending(
            ReviewContext {
                proposals_dir: proposals_dir.clone(),
                working_dir,
            },
            "1",
            false,
        )
        .expect("approve result");

        assert!(output.contains("Tier 3"));
        assert!(proposals_dir.join("1710000450-kernel-absolute.md").exists());
    }

    #[test]
    fn approve_rejects_target_paths_that_escape_working_dir() {
        let temp = TempDir::new().expect("tempdir");
        let working_dir = temp.path().join("repo");
        let proposals_dir = temp.path().join("proposals");
        fs::create_dir_all(&working_dir).expect("create working dir");
        fs::create_dir_all(&proposals_dir).expect("create proposals dir");
        write_sidecar_proposal(
            &proposals_dir,
            "1710000500-escape",
            Path::new("../../outside.txt"),
            "oops",
            None,
        );

        let error = approve_pending(
            ReviewContext {
                proposals_dir: proposals_dir.clone(),
                working_dir,
            },
            "1",
            false,
        )
        .expect_err("escape should fail");

        assert!(error.to_string().contains("escapes working directory"));
        assert!(proposals_dir.join("1710000500-escape.md").exists());
    }

    #[test]
    fn legacy_parser_keeps_nested_code_fences_in_diff_content() {
        let markdown = concat!(
            "# Proposal: Legacy fenced change\n\n",
            "## What and Why\nTest\n\n",
            "## Proposed Diff\n",
            "config/legacy.toml:\n",
            "```\n",
            "before\n",
            "```rust\n",
            "fn demo() {}\n",
            "```\n",
            "after\n",
            "```\n\n",
            "## Risk\nlow\n"
        );
        let lines: Vec<&str> = markdown.lines().collect();

        let (target, content) = parse_legacy_diff(&lines).expect("parse legacy diff");

        assert_eq!(target, PathBuf::from("config/legacy.toml"));
        assert!(content.contains("```rust"));
        assert!(content.contains("fn demo() {}"));
        assert!(content.contains("after"));
    }

    #[test]
    fn resolve_pending_proposal_rejects_ambiguous_prefixes() {
        let temp = TempDir::new().expect("tempdir");
        let proposals_dir = temp.path().join("proposals");
        fs::create_dir_all(&proposals_dir).expect("create proposals dir");
        write_sidecar_proposal(
            &proposals_dir,
            "1710000600-alpha",
            Path::new("config/a.toml"),
            "a = 1",
            None,
        );
        write_sidecar_proposal(
            &proposals_dir,
            "1710000601-alpine",
            Path::new("config/b.toml"),
            "b = 2",
            None,
        );

        let error = resolve_pending_proposal(&proposals_dir, "17100006")
            .expect_err("prefix should be ambiguous");

        assert!(matches!(error, ProposalReviewError::Ambiguous(_)));
    }

    #[test]
    fn reject_archives_legacy_markdown_proposals() {
        let temp = TempDir::new().expect("tempdir");
        let proposals_dir = temp.path().join("proposals");
        fs::create_dir_all(&proposals_dir).expect("create proposals dir");
        let stem = format!("{}-legacy", unique_timestamp());
        let markdown = "# Proposal: Legacy change\n\n## What and Why\nTest\n\n## Proposed Diff\nconfig/legacy.toml:\n```\nlegacy = true\n```\n\n## Risk\nmedium\n";
        fs::write(proposals_dir.join(format!("{stem}.md")), markdown)
            .expect("write legacy proposal");

        let output = reject_pending(
            ReviewContext {
                proposals_dir: proposals_dir.clone(),
                working_dir: temp.path().to_path_buf(),
            },
            &stem,
        )
        .expect("reject result");

        assert!(output.contains("✗ Rejected proposal: Legacy change"));
        assert!(proposals_dir.join(format!("rejected/{stem}.md")).exists());
    }
}
