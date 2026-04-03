use chrono::{TimeZone, Utc};
use fx_kernel::is_tier3_path;
use fx_propose::{
    checked_target_path, current_file_hash, extract_proposed_content, sha256_hex,
    split_proposal_content, ProposalSidecar,
};
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone)]
pub(crate) struct ReviewContext {
    pub proposals_dir: PathBuf,
    pub working_dir: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProposalInfo {
    pub id: String,
    pub filename: String,
    pub target_path: PathBuf,
    pub summary: String,
    pub created_at: u64,
    pub lines_added: usize,
    pub lines_removed: usize,
    pub diff_preview: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct StoredProposal {
    stem: String,
    info: ProposalInfo,
    apply_content: String,
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

struct ProposalContentAnalysis {
    apply_content: String,
    diff_preview: String,
    lines_added: usize,
    lines_removed: usize,
}

#[derive(Default)]
struct DiffHeuristic {
    added: usize,
    removed: usize,
    non_empty: usize,
    has_file_markers: bool,
    has_hunk_header: bool,
}

const MIN_DIFF_SIGNAL_LINES: usize = 3;
const MAX_LCS_LINES_PER_SIDE: usize = 10_000;

enum DiffLine {
    Context(String),
    Added(String),
    Removed(String),
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

pub(crate) fn render_pending(
    context: ReviewContext,
    selector: Option<&str>,
) -> Result<String, ProposalReviewError> {
    let pending = load_pending_proposals(&context.proposals_dir)?;
    match selector {
        Some(value) => render_proposal_detail(&context, &pending.proposals, value),
        None => Ok(render_pending_list(&context, pending)),
    }
}

#[must_use = "approval result includes the user-facing outcome message"]
pub(crate) fn approve_pending(
    context: ReviewContext,
    selector: &str,
    force: bool,
) -> Result<String, ProposalReviewError> {
    let proposal = resolve_pending_proposal(&context.proposals_dir, selector)?;
    let resolved_target = resolve_review_target(&proposal.info.target_path, &context.working_dir)?;
    if is_tier3_path(&resolved_target.policy_path) {
        return Ok(format!(
            "Cannot apply: {} is Tier 3 (kernel immutable)",
            proposal.info.target_path.display()
        ));
    }
    if is_stale(&proposal, &context.working_dir)? && !force {
        return Ok(format!(
            "⚠ Proposal #{} is stale: target file changed since it was created.\nUse /approve {} --force to apply anyway.",
            proposal.info.id, proposal.info.id
        ));
    }

    write_approved_content(&proposal, &resolved_target.absolute_path)?;
    archive_proposal(&context.proposals_dir, &proposal, "applied")?;
    Ok(render_approval_message(&proposal, &context.working_dir))
}

#[must_use = "rejection result includes the user-facing outcome message"]
pub(crate) fn reject_pending(
    context: ReviewContext,
    selector: &str,
) -> Result<String, ProposalReviewError> {
    let proposal = resolve_pending_proposal(&context.proposals_dir, selector)?;
    archive_proposal(&context.proposals_dir, &proposal, "rejected")?;
    Ok(render_rejection_message(&proposal, &context.working_dir))
}

fn render_pending_list(context: &ReviewContext, pending: PendingProposals) -> String {
    if pending.proposals.is_empty() && pending.failures.is_empty() {
        return empty_proposals_message();
    }

    let now = epoch_seconds();
    let mut lines = vec![
        format!("📋 Pending Proposals ({})", pending.proposals.len()),
        String::new(),
    ];
    append_pending_proposals(&mut lines, &pending.proposals, now, &context.working_dir);
    append_parse_failures(&mut lines, &pending.failures);
    if !pending.proposals.is_empty() {
        lines.push(String::new());
        lines.push("Use /proposals <id> for details · /approve <id> · /reject <id>".to_string());
    }
    lines.join("\n")
}

fn render_proposal_detail(
    context: &ReviewContext,
    proposals: &[StoredProposal],
    selector: &str,
) -> Result<String, ProposalReviewError> {
    let proposal = resolve_pending_proposal_from(proposals, selector)?;
    let target_path = display_target_path(&proposal.info.target_path, &context.working_dir);
    let relative_age = format_relative_age(epoch_seconds(), proposal.info.created_at);
    let created_at = format_created_at(proposal.info.created_at);
    let diff_preview = rendered_diff_preview(&proposal.info.diff_preview);

    Ok([
        format!("📋 Proposal #{} — {}", proposal.info.id, target_path),
        String::new(),
        format!("Created: {} ({})", relative_age, created_at),
        format!("Target:  {}", target_path),
        format!("Reason:  {}", proposal.info.summary),
        String::new(),
        "─── Diff ───".to_string(),
        diff_preview,
        String::new(),
        format_line_counts(&proposal.info),
        String::new(),
        format!(
            "/approve {} · /reject {}",
            proposal.info.id, proposal.info.id
        ),
    ]
    .join("\n"))
}

fn empty_proposals_message() -> String {
    [
        "📋 Pending Proposals (0)".to_string(),
        String::new(),
        "No pending proposals.".to_string(),
        "Self-modification requests that need approval will appear here.".to_string(),
    ]
    .join("\n")
}

fn render_approval_message(proposal: &StoredProposal, working_dir: &Path) -> String {
    format!(
        "✅ Applied proposal #{}\n   {} — {}\n   Proposal file removed from pending list.",
        proposal.info.id,
        display_target_path(&proposal.info.target_path, working_dir),
        format_line_counts(&proposal.info)
    )
}

fn render_rejection_message(proposal: &StoredProposal, working_dir: &Path) -> String {
    format!(
        "❌ Rejected proposal #{}\n   {} — proposal file removed from pending list.",
        proposal.info.id,
        display_target_path(&proposal.info.target_path, working_dir)
    )
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
        left.info
            .created_at
            .cmp(&right.info.created_at)
            .then_with(|| left.info.filename.cmp(&right.info.filename))
    });
    pending
        .failures
        .sort_by(|left, right| left.file_name.cmp(&right.file_name));
    Ok(pending)
}

fn append_pending_proposals(
    lines: &mut Vec<String>,
    proposals: &[StoredProposal],
    now: u64,
    working_dir: &Path,
) {
    for (index, proposal) in proposals.iter().enumerate() {
        lines.push(render_list_header(index + 1, proposal, now, working_dir));
        lines.push(format!("     ╰─ {}", proposal.info.summary));
        lines.push(format!("     ╰─ {}", format_line_counts(&proposal.info)));
        lines.push(String::new());
    }
    if matches!(lines.last(), Some(value) if value.is_empty()) {
        lines.pop();
    }
}

fn render_list_header(
    index: usize,
    proposal: &StoredProposal,
    now: u64,
    working_dir: &Path,
) -> String {
    let target_path = display_target_path(&proposal.info.target_path, working_dir);
    let relative_age = format_relative_age(now, proposal.info.created_at);
    format!(
        " #{}  {}  {}  {}",
        index, proposal.info.id, target_path, relative_age
    )
}

fn append_parse_failures(lines: &mut Vec<String>, failures: &[ProposalLoadFailure]) {
    if failures.is_empty() {
        return;
    }
    if !matches!(lines.last(), Some(value) if value.is_empty()) {
        lines.push(String::new());
    }
    lines.push("Unreadable proposals:".to_string());
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
    let filename = proposal_filename(markdown_path)?;
    let sidecar_path = markdown_path.with_extension("json");
    if sidecar_path.exists() {
        return load_sidecar_proposal(markdown_path, &sidecar_path, &stem, &filename);
    }
    load_legacy_proposal(markdown_path, &stem, &filename)
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

fn proposal_filename(markdown_path: &Path) -> Result<String, ProposalReviewError> {
    markdown_path
        .file_name()
        .and_then(|name| name.to_str())
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
    filename: &str,
) -> Result<StoredProposal, ProposalReviewError> {
    let content = fs::read_to_string(sidecar_path)?;
    let sidecar: ProposalSidecar = serde_json::from_str(&content).map_err(|error| {
        ProposalReviewError::Parse(format!(
            "invalid proposal sidecar {}: {error}",
            sidecar_path.display()
        ))
    })?;
    build_stored_proposal(StoredProposalParts {
        stem,
        filename,
        target_path: PathBuf::from(sidecar.target_path),
        description: sidecar.description,
        payload: sidecar.proposed_content,
        created_at: sidecar.timestamp,
        file_hash_at_creation: sidecar.file_hash_at_creation,
        markdown_path,
        sidecar_path: Some(sidecar_path),
    })
}

fn load_legacy_proposal(
    markdown_path: &Path,
    stem: &str,
    filename: &str,
) -> Result<StoredProposal, ProposalReviewError> {
    let content = fs::read_to_string(markdown_path)?;
    let lines: Vec<&str> = content.lines().collect();
    let (target_path, payload) = parse_legacy_diff(&lines)?;
    build_stored_proposal(StoredProposalParts {
        stem,
        filename,
        target_path,
        description: parse_legacy_description(&lines)?,
        payload,
        created_at: parse_timestamp(stem),
        file_hash_at_creation: None,
        markdown_path,
        sidecar_path: None,
    })
}

struct StoredProposalParts<'a> {
    stem: &'a str,
    filename: &'a str,
    target_path: PathBuf,
    description: String,
    payload: String,
    created_at: u64,
    file_hash_at_creation: Option<String>,
    markdown_path: &'a Path,
    sidecar_path: Option<&'a Path>,
}

fn build_stored_proposal(
    parts: StoredProposalParts<'_>,
) -> Result<StoredProposal, ProposalReviewError> {
    let analysis = analyze_proposal_content(&parts.payload);
    Ok(StoredProposal {
        stem: parts.stem.to_string(),
        info: ProposalInfo {
            id: proposal_id(parts.filename),
            filename: parts.filename.to_string(),
            target_path: parts.target_path,
            summary: summarize_description(&parts.description),
            created_at: parts.created_at,
            lines_added: analysis.lines_added,
            lines_removed: analysis.lines_removed,
            diff_preview: analysis.diff_preview,
        },
        apply_content: analysis.apply_content,
        file_hash_at_creation: parts.file_hash_at_creation,
        markdown_path: parts.markdown_path.to_path_buf(),
        sidecar_path: parts.sidecar_path.map(Path::to_path_buf),
    })
}

fn proposal_id(filename: &str) -> String {
    sha256_hex(filename.as_bytes())
        .chars()
        .take(6)
        .collect::<String>()
}

fn summarize_description(description: &str) -> String {
    description
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("No summary provided.")
        .to_string()
}

fn parse_legacy_description(lines: &[&str]) -> Result<String, ProposalReviewError> {
    parse_section_body(lines, "## What and Why")
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

fn parse_section_body(lines: &[&str], section: &str) -> Result<String, ProposalReviewError> {
    let start = section_index(lines, section)?;
    let end = section_end_index(lines, start + 1);
    let body = lines[start + 1..end]
        .iter()
        .map(|line| line.trim_end())
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string();
    if body.is_empty() {
        return Err(ProposalReviewError::Parse(format!(
            "proposal missing content for {section}"
        )));
    }
    Ok(body)
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

fn analyze_proposal_content(payload: &str) -> ProposalContentAnalysis {
    if let Some((original, proposed)) = split_proposal_content(payload) {
        return analyze_before_after(&original, &proposed);
    }
    if looks_like_diff(payload) {
        return analyze_existing_diff(payload);
    }
    analyze_before_after("", &extract_proposed_content(payload))
}

fn analyze_before_after(original: &str, proposed: &str) -> ProposalContentAnalysis {
    let diff = diff_lines(original, proposed);
    let (lines_added, lines_removed) = count_diff_lines(&diff);
    ProposalContentAnalysis {
        apply_content: proposed.to_string(),
        diff_preview: render_diff_preview(&diff),
        lines_added,
        lines_removed,
    }
}

fn analyze_existing_diff(payload: &str) -> ProposalContentAnalysis {
    let (lines_added, lines_removed) = count_prefixed_diff_lines(payload);
    ProposalContentAnalysis {
        apply_content: payload.to_string(),
        diff_preview: payload.trim().to_string(),
        lines_added,
        lines_removed,
    }
}

// Legacy proposals can contain raw file content. Require explicit diff markers
// or dense bidirectional +/- lines so markdown and YAML lists do not get
// misclassified as diffs during the migration window.
fn looks_like_diff(payload: &str) -> bool {
    let signals = collect_diff_heuristic(payload);
    has_explicit_diff_markers(&signals) || has_dense_bidirectional_changes(&signals)
}

fn collect_diff_heuristic(payload: &str) -> DiffHeuristic {
    payload
        .lines()
        .fold(DiffHeuristic::default(), |mut signals, line| {
            if !line.trim().is_empty() {
                signals.non_empty += 1;
            }
            if line.starts_with("+++") || line.starts_with("---") {
                signals.has_file_markers = true;
            }
            if line.starts_with("@@") {
                signals.has_hunk_header = true;
            }
            if is_counted_diff_line(line) {
                if line.starts_with('+') {
                    signals.added += 1;
                } else {
                    signals.removed += 1;
                }
            }
            signals
        })
}

fn has_explicit_diff_markers(signals: &DiffHeuristic) -> bool {
    signals.has_file_markers || (signals.has_hunk_header && signals.added + signals.removed > 0)
}

fn has_dense_bidirectional_changes(signals: &DiffHeuristic) -> bool {
    let total = signals.added + signals.removed;
    signals.added > 0
        && signals.removed > 0
        && total >= MIN_DIFF_SIGNAL_LINES
        && total * 2 >= signals.non_empty
}

fn is_counted_diff_line(line: &str) -> bool {
    if line.starts_with("+++") || line.starts_with("---") {
        return false;
    }
    matches!(line.as_bytes().first(), Some(b'+') | Some(b'-'))
        && !matches!(line.as_bytes().get(1), Some(b' ') | Some(b'\t'))
}

fn count_prefixed_diff_lines(payload: &str) -> (usize, usize) {
    payload.lines().fold((0, 0), |(added, removed), line| {
        if line.starts_with('+') && !line.starts_with("+++") {
            (added + 1, removed)
        } else if line.starts_with('-') && !line.starts_with("---") {
            (added, removed + 1)
        } else {
            (added, removed)
        }
    })
}

fn diff_lines(original: &str, proposed: &str) -> Vec<DiffLine> {
    let original_lines = collect_lines(original);
    let proposed_lines = collect_lines(proposed);
    if should_skip_lcs(&original_lines, &proposed_lines) {
        return simple_diff_lines(&original_lines, &proposed_lines);
    }
    let table = lcs_table(&original_lines, &proposed_lines);
    build_diff_lines(&original_lines, &proposed_lines, &table)
}

fn should_skip_lcs(original: &[String], proposed: &[String]) -> bool {
    original.len() > MAX_LCS_LINES_PER_SIDE && proposed.len() > MAX_LCS_LINES_PER_SIDE
}

fn simple_diff_lines(original: &[String], proposed: &[String]) -> Vec<DiffLine> {
    let mut diff = Vec::with_capacity(original.len() + proposed.len());
    append_remaining_diff_lines(&mut diff, original, proposed);
    diff
}

fn collect_lines(content: &str) -> Vec<String> {
    if content.is_empty() {
        return Vec::new();
    }
    content.lines().map(ToString::to_string).collect()
}

fn lcs_table(original: &[String], proposed: &[String]) -> Vec<Vec<usize>> {
    let mut table = vec![vec![0; proposed.len() + 1]; original.len() + 1];
    for left in (0..original.len()).rev() {
        for right in (0..proposed.len()).rev() {
            table[left][right] = if original[left] == proposed[right] {
                table[left + 1][right + 1] + 1
            } else {
                table[left + 1][right].max(table[left][right + 1])
            };
        }
    }
    table
}

fn build_diff_lines(
    original: &[String],
    proposed: &[String],
    table: &[Vec<usize>],
) -> Vec<DiffLine> {
    let mut left = 0;
    let mut right = 0;
    let mut diff = Vec::new();

    while left < original.len() && right < proposed.len() {
        if original[left] == proposed[right] {
            diff.push(DiffLine::Context(original[left].clone()));
            left += 1;
            right += 1;
        } else if table[left + 1][right] >= table[left][right + 1] {
            diff.push(DiffLine::Removed(original[left].clone()));
            left += 1;
        } else {
            diff.push(DiffLine::Added(proposed[right].clone()));
            right += 1;
        }
    }

    append_remaining_diff_lines(&mut diff, &original[left..], &proposed[right..]);
    diff
}

fn append_remaining_diff_lines(diff: &mut Vec<DiffLine>, original: &[String], proposed: &[String]) {
    diff.extend(original.iter().cloned().map(DiffLine::Removed));
    diff.extend(proposed.iter().cloned().map(DiffLine::Added));
}

fn count_diff_lines(diff: &[DiffLine]) -> (usize, usize) {
    diff.iter()
        .fold((0, 0), |(added, removed), line| match line {
            DiffLine::Added(_) => (added + 1, removed),
            DiffLine::Removed(_) => (added, removed + 1),
            DiffLine::Context(_) => (added, removed),
        })
}

fn render_diff_preview(diff: &[DiffLine]) -> String {
    diff.iter()
        .map(|line| match line {
            DiffLine::Context(content) => format!(" {content}"),
            DiffLine::Added(content) => format!("+{content}"),
            DiffLine::Removed(content) => format!("-{content}"),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn resolve_pending_proposal(
    proposals_dir: &Path,
    selector: &str,
) -> Result<StoredProposal, ProposalReviewError> {
    let proposals = load_pending_proposals(proposals_dir)?.proposals;
    resolve_pending_proposal_from(&proposals, selector)
}

fn resolve_pending_proposal_from(
    proposals: &[StoredProposal],
    selector: &str,
) -> Result<StoredProposal, ProposalReviewError> {
    if proposals.is_empty() {
        return Err(ProposalReviewError::NotFound(
            "No pending proposals.".to_string(),
        ));
    }
    let matches = proposals
        .iter()
        .filter(|proposal| proposal_matches_selector(proposal, selector))
        .cloned()
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [] => Err(ProposalReviewError::NotFound(format!(
            "No pending proposal matches ID '{selector}'. Run /proposals to see valid IDs."
        ))),
        [proposal] => Ok(proposal.clone()),
        _ => Err(ProposalReviewError::Ambiguous(format!(
            "Multiple proposals match ID '{selector}'. Run /proposals to inspect them."
        ))),
    }
}

fn proposal_matches_selector(proposal: &StoredProposal, selector: &str) -> bool {
    proposal.info.id.starts_with(selector)
        || selector == proposal.stem
        || selector == proposal.info.filename
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
    let current_hash = current_file_hash(working_dir, &proposal.info.target_path)?;
    Ok(current_hash.as_deref() != Some(expected_hash.as_str()))
}

fn write_approved_content(
    proposal: &StoredProposal,
    target_path: &Path,
) -> Result<(), ProposalReviewError> {
    if let Some(parent) = target_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(target_path, &proposal.apply_content)?;
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

fn epoch_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn format_relative_age(now: u64, created_at: u64) -> String {
    let age = now.saturating_sub(created_at);
    if age < 60 {
        return "just now".to_string();
    }
    if age < 3_600 {
        return format!("{}m ago", age / 60);
    }
    if age < 86_400 {
        return format!("{}h ago", age / 3_600);
    }
    format!("{}d ago", age / 86_400)
}

fn format_created_at(created_at: u64) -> String {
    let Some(timestamp) = Utc.timestamp_opt(created_at as i64, 0).single() else {
        return "unknown time".to_string();
    };
    timestamp.format("%Y-%m-%d %H:%M UTC").to_string()
}

fn format_line_counts(info: &ProposalInfo) -> String {
    format!("+{} / -{} lines", info.lines_added, info.lines_removed)
}

fn display_target_path(target_path: &Path, working_dir: &Path) -> String {
    if target_path.is_absolute() {
        if let Ok(relative) = target_path.strip_prefix(working_dir) {
            return relative.display().to_string();
        }
    }
    target_path.display().to_string()
}

fn rendered_diff_preview(diff_preview: &str) -> String {
    if diff_preview.trim().is_empty() {
        return "(no diff preview available)".to_string();
    }
    diff_preview.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use fx_propose::build_proposal_content;
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
        description: &str,
    ) {
        let markdown = format!(
            "# Proposal: Update {}\n\n## What and Why\n{}\n\n## Proposed Diff\n{}:\n```\n{}\n```\n\n## Risk\nlow\n",
            target_path.display(),
            description,
            target_path.display(),
            content
        );
        fs::write(proposals_dir.join(format!("{stem}.md")), markdown).expect("write markdown");
        let sidecar = ProposalSidecar {
            version: 1,
            action: "write_file".to_string(),
            timestamp: parse_timestamp(stem),
            title: format!("Update {}", target_path.display()),
            description: description.to_string(),
            target_path: target_path.display().to_string(),
            proposed_content: content.to_string(),
            risk: "low".to_string(),
            file_hash_at_creation: file_hash,
        };
        let sidecar_json = serde_json::to_string_pretty(&sidecar).expect("serialize sidecar");
        fs::write(proposals_dir.join(format!("{stem}.json")), sidecar_json).expect("write sidecar");
    }

    fn proposal_id_for(stem: &str) -> String {
        proposal_id(&format!("{stem}.md"))
    }

    fn proposal_with_filename(filename: &str, target_path: &Path) -> StoredProposal {
        StoredProposal {
            stem: filename.trim_end_matches(".md").to_string(),
            info: ProposalInfo {
                id: proposal_id(filename),
                filename: filename.to_string(),
                target_path: target_path.to_path_buf(),
                summary: "test proposal".to_string(),
                created_at: 0,
                lines_added: 1,
                lines_removed: 0,
                diff_preview: "+content".to_string(),
            },
            apply_content: "content".to_string(),
            file_hash_at_creation: None,
            markdown_path: PathBuf::from(filename),
            sidecar_path: None,
        }
    }

    fn filenames_with_shared_id_prefix() -> (String, String, String) {
        let mut seen = std::collections::HashMap::new();
        for index in 0..1_000 {
            let filename = format!("proposal-{index}.md");
            let id = proposal_id(&filename);
            let prefix = id[..2].to_string();
            if let Some(previous) = seen.insert(prefix.clone(), filename.clone()) {
                return (previous, filename, prefix);
            }
        }
        panic!("failed to find shared id prefix");
    }

    fn large_content(prefix: &str, shared_index: Option<usize>) -> String {
        (0..=10_500)
            .map(|index| match shared_index {
                Some(value) if index == value => "shared-line".to_string(),
                _ => format!("{prefix}-{index}"),
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn render_pending_lists_ids_ages_and_line_counts() {
        let temp = TempDir::new().expect("tempdir");
        let proposals_dir = temp.path().join("proposals");
        fs::create_dir_all(&proposals_dir).expect("create proposals dir");
        let now = epoch_seconds();
        let first_stem = format!("{}-first", now.saturating_sub(7_200));
        let second_stem = format!("{}-second", now.saturating_sub(300));
        let first_content = build_proposal_content(
            Some("name = \"before\"\nmode = \"old\"\n"),
            "name = \"after\"\nmode = \"new\"\nextra = true\n",
        );
        let second_content = build_proposal_content(None, "b = 2\n");
        write_sidecar_proposal(
            &proposals_dir,
            &first_stem,
            Path::new("config/a.toml"),
            &first_content,
            None,
            "modify error handling",
        );
        write_sidecar_proposal(
            &proposals_dir,
            &second_stem,
            Path::new("config/b.toml"),
            &second_content,
            None,
            "add new field",
        );

        let output = render_pending(
            ReviewContext {
                proposals_dir: proposals_dir.clone(),
                working_dir: temp.path().to_path_buf(),
            },
            None,
        )
        .expect("render pending");

        assert!(output.contains("📋 Pending Proposals (2)"));
        assert!(output.contains(&proposal_id_for(&first_stem)));
        assert!(output.contains("config/a.toml"));
        assert!(output.contains("2h ago"));
        assert!(output.contains("modify error handling"));
        assert!(output.contains("+3 / -2 lines"));
        assert!(output.contains("5m ago"));
        assert!(output.contains("Use /proposals <id> for details · /approve <id> · /reject <id>"));
    }

    #[test]
    fn render_pending_shows_detail_view_for_id() {
        let temp = TempDir::new().expect("tempdir");
        let proposals_dir = temp.path().join("proposals");
        fs::create_dir_all(&proposals_dir).expect("create proposals dir");
        let stem = format!("{}-detail", epoch_seconds().saturating_sub(120));
        let proposal_content = build_proposal_content(Some("a = 1\n"), "a = 2\nb = 3\n");
        write_sidecar_proposal(
            &proposals_dir,
            &stem,
            Path::new("config/settings.toml"),
            &proposal_content,
            None,
            "modify error handling for config load failure",
        );
        let id = proposal_id_for(&stem);

        let output = render_pending(
            ReviewContext {
                proposals_dir: proposals_dir.clone(),
                working_dir: temp.path().to_path_buf(),
            },
            Some(&id),
        )
        .expect("render detail");

        assert!(output.contains(&format!("📋 Proposal #{}", id)));
        assert!(output.contains("Target:  config/settings.toml"));
        assert!(output.contains("Reason:  modify error handling for config load failure"));
        assert!(output.contains("-a = 1"));
        assert!(output.contains("+a = 2"));
        assert!(output.contains("+b = 3"));
        assert!(output.contains("/approve"));
    }

    #[test]
    fn render_pending_keeps_listing_when_legacy_proposal_is_malformed() {
        let temp = TempDir::new().expect("tempdir");
        let proposals_dir = temp.path().join("proposals");
        fs::create_dir_all(&proposals_dir).expect("create proposals dir");
        let valid_stem = format!("{}-valid", epoch_seconds());
        write_sidecar_proposal(
            &proposals_dir,
            &valid_stem,
            Path::new("config/a.toml"),
            "a = 1",
            None,
            "valid proposal",
        );

        let malformed = concat!(
            "# Proposal: Legacy change\n\n",
            "## What and Why\nTest\n\n",
            "## Proposed Diff\n",
            "config/legacy.toml:\n",
            "legacy = true\n\n",
            "## Risk\nlow\n"
        );
        fs::write(proposals_dir.join("1710000001-malformed.md"), malformed)
            .expect("write malformed proposal");

        let output = render_pending(
            ReviewContext {
                proposals_dir: proposals_dir.clone(),
                working_dir: temp.path().to_path_buf(),
            },
            None,
        )
        .expect("render pending");

        assert!(output.contains(&proposal_id_for(&valid_stem)));
        assert!(output.contains(
            "⚠ 1710000001-malformed.md — could not parse: legacy proposal diff fence missing"
        ));
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

        let stem = format!("{}-stale", epoch_seconds());
        write_sidecar_proposal(
            &proposals_dir,
            &stem,
            Path::new("config/settings.toml"),
            "approved = true\n",
            Some(original_hash),
            "stale proposal",
        );
        let id = proposal_id_for(&stem);

        let output = approve_pending(
            ReviewContext {
                proposals_dir: proposals_dir.clone(),
                working_dir: working_dir.clone(),
            },
            &id,
            false,
        )
        .expect("approve result");

        assert!(output.contains("is stale"));
        assert!(output.contains(&format!("/approve {} --force", id)));
        let written = fs::read_to_string(&target).expect("read unchanged target");
        assert_eq!(written, "new = false\n");
    }

    #[test]
    fn approve_uses_stable_id_and_extracts_proposed_content() {
        let temp = TempDir::new().expect("tempdir");
        let working_dir = temp.path().join("repo");
        let proposals_dir = temp.path().join("proposals");
        fs::create_dir_all(&working_dir).expect("create working dir");
        fs::create_dir_all(&proposals_dir).expect("create proposals dir");

        let target = working_dir.join("config/settings.toml");
        fs::create_dir_all(target.parent().expect("target parent")).expect("create target parent");
        fs::write(&target, "old = true\n").expect("write original target");
        let content = build_proposal_content(Some("old = true\n"), "approved = true\n");
        let stem = format!("{}-apply", epoch_seconds());
        write_sidecar_proposal(
            &proposals_dir,
            &stem,
            Path::new("config/settings.toml"),
            &content,
            None,
            "apply update",
        );
        let id = proposal_id_for(&stem);

        let output = approve_pending(
            ReviewContext {
                proposals_dir: proposals_dir.clone(),
                working_dir: working_dir.clone(),
            },
            &id,
            true,
        )
        .expect("approve result");

        assert!(output.contains(&format!("✅ Applied proposal #{}", id)));
        let written = fs::read_to_string(&target).expect("read updated target");
        assert_eq!(written, "approved = true\n");
        assert!(proposals_dir.join(format!("applied/{stem}.md")).exists());
        assert!(proposals_dir.join(format!("applied/{stem}.json")).exists());
    }

    #[test]
    fn approve_rejects_sovereign_targets() {
        let temp = TempDir::new().expect("tempdir");
        let proposals_dir = temp.path().join("proposals");
        fs::create_dir_all(&proposals_dir).expect("create proposals dir");
        let stem = format!("{}-ripcord", epoch_seconds());
        write_sidecar_proposal(
            &proposals_dir,
            &stem,
            Path::new("engine/crates/fx-ripcord/src/lib.rs"),
            "bad",
            None,
            "sovereign write",
        );
        let id = proposal_id_for(&stem);

        let output = approve_pending(
            ReviewContext {
                proposals_dir: proposals_dir.clone(),
                working_dir: temp.path().to_path_buf(),
            },
            &id,
            false,
        )
        .expect("approve result");

        assert!(output.contains("Tier 3"));
        assert!(proposals_dir.join(format!("{stem}.md")).exists());
    }

    #[test]
    fn approve_rejects_absolute_sovereign_targets() {
        let temp = TempDir::new().expect("tempdir");
        let working_dir = temp.path().join("repo");
        let proposals_dir = temp.path().join("proposals");
        let target = working_dir.join("engine/crates/fx-ripcord/src/lib.rs");
        fs::create_dir_all(target.parent().expect("parent")).expect("mkdir");
        fs::create_dir_all(&proposals_dir).expect("create proposals dir");
        let stem = format!("{}-ripcord-absolute", epoch_seconds());
        write_sidecar_proposal(
            &proposals_dir,
            &stem,
            &target,
            "bad",
            None,
            "sovereign write",
        );
        let id = proposal_id_for(&stem);

        let output = approve_pending(
            ReviewContext {
                proposals_dir: proposals_dir.clone(),
                working_dir,
            },
            &id,
            false,
        )
        .expect("approve result");

        assert!(output.contains("Tier 3"));
        assert!(proposals_dir.join(format!("{stem}.md")).exists());
    }

    #[test]
    fn approve_rejects_target_paths_that_escape_working_dir() {
        let temp = TempDir::new().expect("tempdir");
        let working_dir = temp.path().join("repo");
        let proposals_dir = temp.path().join("proposals");
        fs::create_dir_all(&working_dir).expect("create working dir");
        fs::create_dir_all(&proposals_dir).expect("create proposals dir");
        let stem = format!("{}-escape", epoch_seconds());
        write_sidecar_proposal(
            &proposals_dir,
            &stem,
            Path::new("../../outside.txt"),
            "oops",
            None,
            "escape attempt",
        );
        let id = proposal_id_for(&stem);

        let error = approve_pending(
            ReviewContext {
                proposals_dir: proposals_dir.clone(),
                working_dir,
            },
            &id,
            false,
        )
        .expect_err("escape should fail");

        assert!(error.to_string().contains("escapes working directory"));
        assert!(proposals_dir.join(format!("{stem}.md")).exists());
    }

    #[test]
    fn empty_list_shows_helpful_message() {
        let temp = TempDir::new().expect("tempdir");
        let output = render_pending(
            ReviewContext {
                proposals_dir: temp.path().join("proposals"),
                working_dir: temp.path().to_path_buf(),
            },
            None,
        )
        .expect("render empty list");

        assert!(output.contains("Pending Proposals (0)"));
        assert!(output.contains("Self-modification requests that need approval will appear here."));
    }

    #[test]
    fn format_relative_age_covers_expected_ranges() {
        assert_eq!(format_relative_age(100, 95), "just now");
        assert_eq!(format_relative_age(600, 300), "5m ago");
        assert_eq!(format_relative_age(10_800, 3_600), "2h ago");
        assert_eq!(format_relative_age(345_600, 86_400), "3d ago");
    }

    #[test]
    fn line_count_calculation_uses_structured_diff_content() {
        let analysis =
            analyze_proposal_content(&build_proposal_content(Some("a\nb\nc\n"), "a\nx\nc\nd\n"));

        assert_eq!(analysis.lines_added, 2);
        assert_eq!(analysis.lines_removed, 1);
        assert!(analysis.diff_preview.contains("-b"));
        assert!(analysis.diff_preview.contains("+x"));
        assert!(analysis.diff_preview.contains("+d"));
    }

    #[test]
    fn legacy_parser_keeps_nested_code_fences_in_diff_content() {
        let markdown = concat!(
            "# Proposal: Legacy fenced change\n\n",
            "## What and Why\nTest\n\n",
            "## Proposed Diff\n",
            "config/legacy.toml:\n",
            "````\n",
            "before\n",
            "```rust\n",
            "fn demo() {}\n",
            "```\n",
            "after\n",
            "````\n\n",
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
    fn resolve_pending_proposal_rejects_ambiguous_id_prefixes() {
        let (first, second, prefix) = filenames_with_shared_id_prefix();
        let proposals = vec![
            proposal_with_filename(&first, Path::new("config/a.toml")),
            proposal_with_filename(&second, Path::new("config/b.toml")),
        ];

        let error = resolve_pending_proposal_from(&proposals, &prefix)
            .expect_err("id prefix should be ambiguous");

        assert!(matches!(error, ProposalReviewError::Ambiguous(_)));
    }

    #[test]
    fn looks_like_diff_ignores_markdown_lists() {
        let payload = "- first item\n- second item\n+ third item\n";

        assert!(!looks_like_diff(payload));
    }

    #[test]
    fn large_before_after_content_uses_simple_diff_fallback() {
        let original = large_content("original", Some(5_000));
        let proposed = large_content("proposed", Some(5_000));
        let diff = diff_lines(&original, &proposed);

        assert_eq!(diff.len(), 21_002);
        assert!(matches!(diff.first(), Some(DiffLine::Removed(line)) if line == "original-0"));
        assert!(!diff
            .iter()
            .any(|line| matches!(line, DiffLine::Context(content) if content == "shared-line")));
        assert!(matches!(diff.last(), Some(DiffLine::Added(line)) if line == "proposed-10500"));
    }

    #[test]
    fn reject_removes_file_from_pending_by_stable_id() {
        let temp = TempDir::new().expect("tempdir");
        let proposals_dir = temp.path().join("proposals");
        fs::create_dir_all(&proposals_dir).expect("create proposals dir");
        let stem = format!("{}-legacy", unique_timestamp());
        let markdown = "# Proposal: Legacy change\n\n## What and Why\nTest\n\n## Proposed Diff\nconfig/legacy.toml:\n```\nlegacy = true\n```\n\n## Risk\nmedium\n";
        fs::write(proposals_dir.join(format!("{stem}.md")), markdown)
            .expect("write legacy proposal");
        let id = proposal_id_for(&stem);

        let output = reject_pending(
            ReviewContext {
                proposals_dir: proposals_dir.clone(),
                working_dir: temp.path().to_path_buf(),
            },
            &id,
        )
        .expect("reject result");

        assert!(output.contains(&format!("❌ Rejected proposal #{}", id)));
        assert!(proposals_dir.join(format!("rejected/{stem}.md")).exists());
    }

    #[test]
    fn invalid_id_returns_clear_error() {
        let temp = TempDir::new().expect("tempdir");
        let proposals_dir = temp.path().join("proposals");
        fs::create_dir_all(&proposals_dir).expect("create proposals dir");
        let stem = format!("{}-alpha", epoch_seconds());
        write_sidecar_proposal(
            &proposals_dir,
            &stem,
            Path::new("config/a.toml"),
            "a = 1\n",
            None,
            "alpha",
        );

        let error = approve_pending(
            ReviewContext {
                proposals_dir: proposals_dir.clone(),
                working_dir: temp.path().to_path_buf(),
            },
            "missing1",
            false,
        )
        .expect_err("missing id");

        assert!(error
            .to_string()
            .contains("No pending proposal matches ID 'missing1'"));
    }
}
