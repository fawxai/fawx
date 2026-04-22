use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

/// External resource classes used by the route planner control plane.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ResourceKind {
    GenericUrl,
    #[serde(rename = "github_repository")]
    GitHubRepository,
    #[serde(rename = "github_pull_request")]
    GitHubPullRequest,
    #[serde(rename = "github_issue")]
    GitHubIssue,
}

impl ResourceKind {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::GenericUrl => "generic_url",
            Self::GitHubRepository => "github_repository",
            Self::GitHubPullRequest => "github_pull_request",
            Self::GitHubIssue => "github_issue",
        }
    }
}

impl fmt::Display for ResourceKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseResourceKindError {
    value: String,
}

impl ParseResourceKindError {
    fn new(value: &str) -> Self {
        Self {
            value: value.to_string(),
        }
    }
}

impl fmt::Display for ParseResourceKindError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unknown resource kind '{}'", self.value)
    }
}

impl std::error::Error for ParseResourceKindError {}

impl FromStr for ResourceKind {
    type Err = ParseResourceKindError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim() {
            "generic_url" => Ok(Self::GenericUrl),
            "github_repository" => Ok(Self::GitHubRepository),
            "github_pull_request" => Ok(Self::GitHubPullRequest),
            "github_issue" => Ok(Self::GitHubIssue),
            other => Err(ParseResourceKindError::new(other)),
        }
    }
}

/// Supported operations over a routed external resource.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum RouteOperation {
    Fetch,
    List,
    Create,
    Comment,
}

/// Authentication contract for a route-capable tool.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum RouteAuthMode {
    None,
    CredentialRequired { key: String },
}

/// Artifact retrieval strategy exposed to the control plane.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactStrategy {
    DirectFetch,
    ProbeFirst,
}

/// Advisory-only memory source for route ranking signals.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum RouteAdvisorySource {
    Journal,
}

/// Advisory-only route outcome captured in memory.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum RouteAdvisoryOutcome {
    Prefer,
    Avoid,
    Neutral,
}

/// Typed advisory record that may improve route ranking without changing the
/// authoritative route contract.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RouteAdvisory {
    pub resource_kind: ResourceKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    pub outcome: RouteAdvisoryOutcome,
    pub source: RouteAdvisorySource,
    pub note: String,
    pub observed_at_ms: u64,
}

/// Static routing metadata declared by a tool manifest.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolRoutingMetadata {
    pub resource_kinds: Vec<ResourceKind>,
    pub operations: Vec<RouteOperation>,
    pub auth_mode: RouteAuthMode,
    pub artifact_strategy: ArtifactStrategy,
    /// Lower values win first when multiple tools can satisfy the same route.
    pub fallback_rank: u16,
}

/// Runtime readiness visible to the kernel without exposing secrets.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolReadinessSummary {
    pub available: bool,
    pub ready: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub readiness_reason: Option<String>,
}

/// Combined static + runtime route summary for a tool.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolRoutingSummary {
    pub tool_name: String,
    pub metadata: ToolRoutingMetadata,
    pub readiness: ToolReadinessSummary,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn github_resource_kinds_use_expected_manifest_names() {
        let repository =
            serde_json::to_string(&ResourceKind::GitHubRepository).expect("serialize repository");
        let pull_request = serde_json::to_string(&ResourceKind::GitHubPullRequest)
            .expect("serialize pull request");
        let issue = serde_json::to_string(&ResourceKind::GitHubIssue).expect("serialize issue");

        assert_eq!(repository, "\"github_repository\"");
        assert_eq!(pull_request, "\"github_pull_request\"");
        assert_eq!(issue, "\"github_issue\"");

        assert_eq!(
            serde_json::from_str::<ResourceKind>("\"github_repository\"").expect("deserialize"),
            ResourceKind::GitHubRepository
        );
    }

    #[test]
    fn route_advisory_round_trips_through_json() {
        let advisory = RouteAdvisory {
            resource_kind: ResourceKind::GitHubPullRequest,
            tool_name: Some("list_pr_files".to_string()),
            outcome: RouteAdvisoryOutcome::Prefer,
            source: RouteAdvisorySource::Journal,
            note: "probe first".to_string(),
            observed_at_ms: 1_234,
        };

        let json = serde_json::to_string(&advisory).expect("serialize advisory");
        let restored: RouteAdvisory = serde_json::from_str(&json).expect("deserialize advisory");
        assert_eq!(restored, advisory);
    }

    #[test]
    fn resource_kind_from_str_uses_manifest_names() {
        assert_eq!(
            "generic_url"
                .parse::<ResourceKind>()
                .expect("parse generic url"),
            ResourceKind::GenericUrl
        );
        assert_eq!(
            "github_repository"
                .parse::<ResourceKind>()
                .expect("parse repository"),
            ResourceKind::GitHubRepository
        );
        assert_eq!(
            "github_pull_request"
                .parse::<ResourceKind>()
                .expect("parse pull request"),
            ResourceKind::GitHubPullRequest
        );
        assert_eq!(
            "github_issue".parse::<ResourceKind>().expect("parse issue"),
            ResourceKind::GitHubIssue
        );

        let error = "unknown_kind"
            .parse::<ResourceKind>()
            .expect_err("invalid kinds should fail");
        assert_eq!(error.to_string(), "unknown resource kind 'unknown_kind'");
    }
}
