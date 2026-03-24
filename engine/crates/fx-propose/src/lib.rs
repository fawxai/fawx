//! Proposal system for self-modification.
//!
//! When the agent attempts to write to a propose-tier path, a structured
//! proposal is written to disk instead. A human must review and approve
//! the proposal before the change can be applied.

mod target_file;

pub use target_file::{checked_target_path, current_file_hash, sha256_hex};

use serde::{Deserialize, Serialize};
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const MAX_TITLE_LENGTH: usize = 80;
const SIDECAR_VERSION: u8 = 1;

/// A structured proposal for a self-modification change.
#[derive(Debug, Clone)]
pub struct Proposal {
    pub title: String,
    pub description: String,
    pub target_path: PathBuf,
    pub proposed_content: String,
    pub risk: String,
    pub timestamp: u64,
    pub file_hash: Option<String>,
}

/// Machine-readable proposal metadata stored alongside markdown proposals.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProposalSidecar {
    pub version: u8,
    pub timestamp: u64,
    pub title: String,
    pub description: String,
    pub target_path: String,
    pub proposed_content: String,
    pub risk: String,
    pub file_hash_at_creation: Option<String>,
}

impl ProposalSidecar {
    #[must_use]
    pub fn from_proposal(proposal: &Proposal) -> Self {
        Self {
            version: SIDECAR_VERSION,
            timestamp: proposal.timestamp,
            title: proposal.title.clone(),
            description: proposal.description.clone(),
            target_path: proposal.target_path.display().to_string(),
            proposed_content: proposal.proposed_content.clone(),
            risk: proposal.risk.clone(),
            file_hash_at_creation: proposal.file_hash.clone(),
        }
    }
}

/// Error type for proposal operations.
#[derive(Debug)]
pub enum ProposalError {
    /// Failed to create the proposals directory.
    CreateDir(io::Error),
    /// Failed to write the markdown proposal file.
    WriteMarkdown(io::Error),
    /// Failed to serialize the sidecar JSON.
    SerializeSidecar(serde_json::Error),
    /// Failed to write the sidecar JSON file.
    WriteSidecar(io::Error),
}

impl fmt::Display for ProposalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CreateDir(error) => write!(f, "failed to create proposals directory: {error}"),
            Self::WriteMarkdown(error) => write!(f, "failed to write proposal markdown: {error}"),
            Self::SerializeSidecar(error) => {
                write!(f, "failed to serialize proposal sidecar: {error}")
            }
            Self::WriteSidecar(error) => write!(f, "failed to write proposal sidecar: {error}"),
        }
    }
}

impl std::error::Error for ProposalError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::CreateDir(error) | Self::WriteMarkdown(error) | Self::WriteSidecar(error) => {
                Some(error)
            }
            Self::SerializeSidecar(error) => Some(error),
        }
    }
}

/// Writes proposals to a configurable directory as structured markdown.
#[derive(Debug)]
pub struct ProposalWriter {
    proposals_dir: PathBuf,
}

impl ProposalWriter {
    pub fn new(proposals_dir: PathBuf) -> Self {
        Self { proposals_dir }
    }

    /// Write a proposal to disk and return the path of the created file.
    pub fn write(&self, proposal: &Proposal) -> Result<PathBuf, ProposalError> {
        fs::create_dir_all(&self.proposals_dir).map_err(ProposalError::CreateDir)?;
        let stem = proposal_stem(proposal);
        let markdown_path = self.markdown_path(&stem);
        let sidecar_path = self.sidecar_path(&stem);
        write_markdown(&markdown_path, proposal)?;
        write_sidecar(&sidecar_path, proposal)?;
        Ok(markdown_path)
    }

    fn markdown_path(&self, stem: &str) -> PathBuf {
        self.proposals_dir.join(format!("{stem}.md"))
    }

    fn sidecar_path(&self, stem: &str) -> PathBuf {
        self.proposals_dir.join(format!("{stem}.json"))
    }
}

fn proposal_stem(proposal: &Proposal) -> String {
    let sanitized = sanitize_title(&proposal.title);
    format!("{}-{sanitized}", proposal.timestamp)
}

fn write_markdown(path: &Path, proposal: &Proposal) -> Result<(), ProposalError> {
    let content = format_proposal(proposal);
    fs::write(path, content).map_err(ProposalError::WriteMarkdown)
}

fn write_sidecar(path: &Path, proposal: &Proposal) -> Result<(), ProposalError> {
    let content = serde_json::to_string_pretty(&ProposalSidecar::from_proposal(proposal))
        .map_err(ProposalError::SerializeSidecar)?;
    fs::write(path, content).map_err(ProposalError::WriteSidecar)
}

/// Format a proposal as structured markdown per the git-self-modification spec.
#[must_use]
pub(crate) fn format_proposal(proposal: &Proposal) -> String {
    let fence = proposal_fence(&proposal.proposed_content);
    format!(
        "# Proposal: {title}\n\n\
         ## What and Why\n\
         {description}\n\n\
         ## Proposed Diff\n\
         {target_path}:\n\
         {fence}\n\
         {diff}\n\
         {fence}\n\n\
         ## Risk\n\
         {risk}\n",
        title = proposal.title,
        description = proposal.description,
        target_path = proposal.target_path.display(),
        fence = fence,
        diff = proposal.proposed_content,
        risk = proposal.risk,
    )
}

const ORIGINAL_HEADER_PREFIX: &str = "--- original (";
const PROPOSED_HEADER_PREFIX: &str = "--- proposed (";
const HEADER_SUFFIX: &str = " bytes) ---";

#[must_use]
pub fn build_proposal_content(original: Option<&str>, proposed: &str) -> String {
    match original {
        Some(content) => format!(
            "{ORIGINAL_HEADER_PREFIX}{}{}\n{}\n{PROPOSED_HEADER_PREFIX}{}{}\n{}",
            content.len(),
            HEADER_SUFFIX,
            content,
            proposed.len(),
            HEADER_SUFFIX,
            proposed
        ),
        None => proposed.to_string(),
    }
}

#[must_use]
pub fn extract_proposed_content(content: &str) -> String {
    split_proposal_content(content)
        .map(|(_, proposed)| proposed)
        .unwrap_or_else(|| content.to_string())
}

#[must_use]
pub fn split_proposal_content(content: &str) -> Option<(String, String)> {
    let (original_len, rest) = parse_content_header(content, ORIGINAL_HEADER_PREFIX)?;
    let rest = rest.strip_prefix('\n')?;
    let original = rest.get(..original_len)?;
    let rest = rest.get(original_len..)?.strip_prefix('\n')?;
    let (proposed_len, rest) = parse_content_header(rest, PROPOSED_HEADER_PREFIX)?;
    let rest = rest.strip_prefix('\n')?;
    if rest.len() != proposed_len {
        return None;
    }
    Some((original.to_string(), rest.to_string()))
}

fn parse_content_header<'a>(content: &'a str, prefix: &str) -> Option<(usize, &'a str)> {
    let newline = content.find('\n')?;
    let header = content.get(..newline)?;
    let length = header
        .strip_prefix(prefix)?
        .strip_suffix(HEADER_SUFFIX)?
        .parse::<usize>()
        .ok()?;
    Some((length, content.get(newline..)?))
}

fn proposal_fence(content: &str) -> String {
    let longest_run = content.lines().map(longest_backtick_run).max().unwrap_or(0);
    "`".repeat((longest_run + 1).max(3))
}

fn longest_backtick_run(line: &str) -> usize {
    let mut longest = 0;
    let mut current = 0;

    for ch in line.chars() {
        if ch == '`' {
            current += 1;
            longest = longest.max(current);
        } else {
            current = 0;
        }
    }

    longest
}

/// Sanitize a title for use as a filename component.
///
/// Replaces non-alphanumeric characters (except `-` and `_`) with dashes,
/// lowercases, collapses consecutive dashes, strips leading/trailing dashes,
/// and truncates to [`MAX_TITLE_LENGTH`]. Returns `"untitled"` for empty input.
#[must_use]
pub(crate) fn sanitize_title(title: &str) -> String {
    let replaced: String = title
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let collapsed = collapse_dashes(&replaced);
    let trimmed = collapsed.trim_matches('-');
    if trimmed.is_empty() {
        return "untitled".to_string();
    }
    truncate_to_boundary(trimmed, MAX_TITLE_LENGTH).to_string()
}

fn collapse_dashes(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut prev_dash = false;
    for c in input.chars() {
        if c == '-' {
            if !prev_dash {
                result.push(c);
            }
            prev_dash = true;
        } else {
            result.push(c);
            prev_dash = false;
        }
    }
    result
}

fn truncate_to_boundary(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].trim_end_matches('-')
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn sample_proposal() -> Proposal {
        Proposal {
            title: "Modify kernel/loop.rs".to_string(),
            description: "Refine loop behavior".to_string(),
            target_path: PathBuf::from("kernel/loop.rs"),
            proposed_content: "fn tick() {}".to_string(),
            risk: "low".to_string(),
            timestamp: 1_710_000_000,
            file_hash: Some("sha256:abcdef1234".to_string()),
        }
    }

    #[test]
    fn proposal_content_round_trips_original_and_proposed_sections() {
        let content = build_proposal_content(Some("old = true\n"), "new = true\n");
        let (original, proposed) = split_proposal_content(&content).expect("split content");

        assert_eq!(original, "old = true\n");
        assert_eq!(proposed, "new = true\n");
        assert_eq!(extract_proposed_content(&content), "new = true\n");
    }

    #[test]
    fn extract_proposed_content_leaves_raw_content_unchanged() {
        assert_eq!(extract_proposed_content("plain text"), "plain text");
    }

    #[test]
    fn sanitize_title_replaces_special_characters() {
        assert_eq!(
            sanitize_title("Modify kernel/loop.rs"),
            "modify-kernel-loop-rs"
        );
    }

    #[test]
    fn sanitize_title_collapses_consecutive_dashes() {
        assert_eq!(sanitize_title("a---b"), "a-b");
    }

    #[test]
    fn sanitize_title_strips_leading_trailing_dashes() {
        assert_eq!(sanitize_title("---hello---"), "hello");
    }

    #[test]
    fn sanitize_title_returns_untitled_for_empty() {
        assert_eq!(sanitize_title(""), "untitled");
    }

    #[test]
    fn sanitize_title_returns_untitled_for_all_special() {
        assert_eq!(sanitize_title("///..."), "untitled");
    }

    #[test]
    fn sanitize_title_truncates_long_titles() {
        let long = "a".repeat(200);
        let sanitized = sanitize_title(&long);
        assert_eq!(sanitized.len(), MAX_TITLE_LENGTH);
    }

    #[test]
    fn sanitize_title_lowercases() {
        assert_eq!(sanitize_title("HELLO_World"), "hello_world");
    }

    #[test]
    fn format_proposal_includes_all_sections() {
        let proposal = sample_proposal();
        let output = format_proposal(&proposal);
        assert!(output.contains("# Proposal: Modify kernel/loop.rs"));
        assert!(output.contains("## What and Why"));
        assert!(output.contains("Refine loop behavior"));
        assert!(output.contains("## Proposed Diff"));
        assert!(output.contains("kernel/loop.rs"));
        assert!(output.contains("fn tick() {}"));
        assert!(output.contains("## Risk"));
        assert!(output.contains("low"));
    }

    #[test]
    fn proposal_fence_uses_standard_triple_backticks_when_content_has_no_backticks() {
        assert_eq!(proposal_fence("fn tick() {}"), "```");
    }

    #[test]
    fn format_proposal_uses_longer_fence_when_content_contains_triple_backticks() {
        let mut proposal = sample_proposal();
        proposal.proposed_content = "before\n```rust\nfn demo() {}\n```\nafter".to_string();

        let output = format_proposal(&proposal);

        assert!(output.contains("````"));
        assert!(output.contains("```rust"));
    }

    #[test]
    fn proposal_writer_creates_directory_markdown_and_sidecar() {
        let temp = TempDir::new().expect("tempdir");
        let proposals_dir = temp.path().join("proposals");
        let proposal = sample_proposal();
        let writer = ProposalWriter::new(proposals_dir.clone());

        let markdown_path = writer.write(&proposal).expect("write proposal");
        let sidecar_path = markdown_path.with_extension("json");

        assert!(proposals_dir.exists());
        assert!(markdown_path.exists());
        assert!(sidecar_path.exists());

        let markdown = fs::read_to_string(markdown_path).expect("read markdown");
        assert_eq!(markdown, format_proposal(&proposal));

        let sidecar = fs::read_to_string(sidecar_path).expect("read sidecar");
        let parsed: ProposalSidecar = serde_json::from_str(&sidecar).expect("parse sidecar");
        assert_eq!(parsed, ProposalSidecar::from_proposal(&proposal));
    }

    #[test]
    fn proposal_writer_filename_format() {
        let temp = TempDir::new().expect("tempdir");
        let proposals_dir = temp.path().join("proposals");
        let proposal = sample_proposal();
        let writer = ProposalWriter::new(proposals_dir);

        let path = writer.write(&proposal).expect("write proposal");
        let filename = path
            .file_name()
            .and_then(|name| name.to_str())
            .expect("proposal filename should be utf-8");

        assert_eq!(filename, "1710000000-modify-kernel-loop-rs.md");
        assert!(path.with_extension("json").exists());
    }

    #[test]
    fn proposal_writer_handles_missing_parent_dirs() {
        let temp = TempDir::new().expect("tempdir");
        let proposals_dir = temp.path().join("a").join("b").join("c").join("proposals");
        let proposal = sample_proposal();
        let writer = ProposalWriter::new(proposals_dir.clone());

        let path = writer.write(&proposal).expect("write proposal");

        assert!(proposals_dir.exists());
        assert!(path.exists());
        assert!(path.with_extension("json").exists());
    }

    #[test]
    fn proposal_sidecar_serializes_null_hash_when_missing() {
        let mut proposal = sample_proposal();
        proposal.file_hash = None;

        let sidecar = ProposalSidecar::from_proposal(&proposal);

        assert_eq!(sidecar.file_hash_at_creation, None);
    }
}
