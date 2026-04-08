use serde::{Deserialize, Serialize};

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
}
