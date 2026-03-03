//! Proposal system for self-modification.
//!
//! When the agent attempts to write to a propose-tier path, a structured
//! proposal is written to disk instead. A human must review and approve
//! the proposal before the change can be applied.

use std::fmt;
use std::fs;
use std::io;
use std::path::PathBuf;

const MAX_TITLE_LENGTH: usize = 80;

/// A structured proposal for a self-modification change.
#[derive(Debug, Clone)]
pub struct Proposal {
    pub title: String,
    pub description: String,
    pub target_path: PathBuf,
    pub proposed_content: String,
    pub risk: String,
    pub timestamp: u64,
}

/// Error type for proposal operations.
#[derive(Debug)]
pub enum ProposalError {
    /// Failed to create the proposals directory.
    CreateDir(io::Error),
    /// Failed to write the proposal file.
    WriteFile(io::Error),
}

impl fmt::Display for ProposalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CreateDir(error) => write!(f, "failed to create proposals directory: {error}"),
            Self::WriteFile(error) => write!(f, "failed to write proposal: {error}"),
        }
    }
}

impl std::error::Error for ProposalError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::CreateDir(error) | Self::WriteFile(error) => Some(error),
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
        let sanitized = sanitize_title(&proposal.title);
        let filename = format!("{}-{}.md", proposal.timestamp, sanitized);
        let path = self.proposals_dir.join(filename);
        let content = format_proposal(proposal);
        fs::write(&path, content).map_err(ProposalError::WriteFile)?;
        Ok(path)
    }
}

/// Format a proposal as structured markdown per the git-self-modification spec.
#[must_use]
pub(crate) fn format_proposal(proposal: &Proposal) -> String {
    format!(
        "# Proposal: {title}\n\n\
         ## What and Why\n\
         {description}\n\n\
         ## Proposed Diff\n\
         {target_path}:\n\
         ```\n\
         {diff}\n\
         ```\n\n\
         ## Risk\n\
         {risk}\n",
        title = proposal.title,
        description = proposal.description,
        target_path = proposal.target_path.display(),
        diff = proposal.proposed_content,
        risk = proposal.risk,
    )
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
        }
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
    fn proposal_writer_creates_directory_and_file() {
        let temp = TempDir::new().expect("tempdir");
        let proposals_dir = temp.path().join("proposals");
        let proposal = sample_proposal();
        let writer = ProposalWriter::new(proposals_dir.clone());

        let file_path = writer.write(&proposal).expect("write proposal");

        assert!(proposals_dir.exists());
        assert!(file_path.exists());
        let content = fs::read_to_string(file_path).expect("read proposal");
        assert_eq!(content, format_proposal(&proposal));
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
    }
}
