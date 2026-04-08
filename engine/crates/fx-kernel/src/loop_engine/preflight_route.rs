use fx_core::tool_routing::{
    ArtifactStrategy, ResourceKind, RouteAuthMode, RouteOperation, ToolRoutingSummary,
};
use fx_llm::ToolDefinition;
use serde::Serialize;
use std::collections::HashSet;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(super) enum RouteResource {
    #[serde(rename = "github_pull_request")]
    GitHubPullRequest {
        url: String,
        owner: String,
        repo: String,
        number: u64,
    },
    #[serde(rename = "github_issue")]
    GitHubIssue {
        url: String,
        owner: String,
        repo: String,
        number: u64,
    },
    #[serde(rename = "github_repository")]
    GitHubRepository {
        url: String,
        owner: String,
        repo: String,
    },
    GenericUrl {
        url: String,
    },
}

impl RouteResource {
    pub(super) fn kind(&self) -> ResourceKind {
        match self {
            Self::GitHubPullRequest { .. } => ResourceKind::GitHubPullRequest,
            Self::GitHubIssue { .. } => ResourceKind::GitHubIssue,
            Self::GitHubRepository { .. } => ResourceKind::GitHubRepository,
            Self::GenericUrl { .. } => ResourceKind::GenericUrl,
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub(super) enum RouteFamily {
    #[serde(rename = "github")]
    GitHub,
    #[serde(rename = "public_web")]
    PublicWeb,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(super) struct PlannedRoute {
    pub family: RouteFamily,
    pub tool_names: Vec<String>,
    pub reason: String,
    pub authenticated: bool,
    pub artifact_strategy: ArtifactStrategy,
    pub fallback_rank: u16,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(super) struct RoutePlan {
    pub resource: RouteResource,
    pub primary_route: PlannedRoute,
    pub fallback_routes: Vec<PlannedRoute>,
    pub requires_probe: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum RouteMatchKind {
    Exact,
    GenericFallback,
}

#[derive(Debug, Clone)]
struct CandidateTool {
    tool_name: String,
    family: RouteFamily,
    match_kind: RouteMatchKind,
    authenticated: bool,
    artifact_strategy: ArtifactStrategy,
    fallback_rank: u16,
    ready: bool,
}

#[derive(Debug, Clone)]
struct CandidateRoute {
    family: RouteFamily,
    match_kind: RouteMatchKind,
    authenticated: bool,
    artifact_strategy: ArtifactStrategy,
    fallback_rank: u16,
    tool_names: Vec<String>,
}

pub(super) fn detect_route_resource(user_message: &str) -> Option<RouteResource> {
    let url = first_url(user_message)?;
    classify_url(&url)
}

pub(super) fn build_route_plan(
    resource: &RouteResource,
    available_tools: &[ToolDefinition],
    routing_tools: &[ToolRoutingSummary],
) -> Option<RoutePlan> {
    let available_tool_names: HashSet<&str> = available_tools
        .iter()
        .map(|tool| tool.name.as_str())
        .collect();
    let candidates = routing_tools
        .iter()
        .filter(|summary| available_tool_names.contains(summary.tool_name.as_str()))
        .filter_map(|summary| candidate_tool_for_resource(resource, summary))
        .collect::<Vec<_>>();

    if candidates.is_empty() {
        return None;
    }

    let exact_route_present = candidates
        .iter()
        .any(|candidate| candidate.match_kind == RouteMatchKind::Exact);
    let ready_routes = grouped_ready_routes(&candidates, resource);
    if ready_routes.is_empty() {
        return None;
    }

    let mut routes = ready_routes.into_iter();
    let primary = routes.next().expect("ready routes should not be empty");
    let fallback_routes = routes
        .map(|route| planned_route_from_candidate(route, exact_route_present, false))
        .collect::<Vec<_>>();
    let primary_route = planned_route_from_candidate(primary, exact_route_present, true);

    Some(RoutePlan {
        resource: resource.clone(),
        requires_probe: matches!(
            primary_route.artifact_strategy,
            ArtifactStrategy::ProbeFirst
        ),
        primary_route,
        fallback_routes,
    })
}

fn grouped_ready_routes(
    candidates: &[CandidateTool],
    resource: &RouteResource,
) -> Vec<CandidateRoute> {
    let mut grouped = Vec::<CandidateRoute>::new();

    for candidate in candidates.iter().filter(|candidate| candidate.ready) {
        if let Some(route) = grouped.iter_mut().find(|route| {
            route.family == candidate.family
                && route.match_kind == candidate.match_kind
                && route.authenticated == candidate.authenticated
                && route.artifact_strategy == candidate.artifact_strategy
        }) {
            route.fallback_rank = route.fallback_rank.min(candidate.fallback_rank);
            route.tool_names.push(candidate.tool_name.clone());
            continue;
        }

        grouped.push(CandidateRoute {
            family: candidate.family.clone(),
            match_kind: candidate.match_kind,
            authenticated: candidate.authenticated,
            artifact_strategy: candidate.artifact_strategy.clone(),
            fallback_rank: candidate.fallback_rank,
            tool_names: vec![candidate.tool_name.clone()],
        });
    }

    for route in &mut grouped {
        route.tool_names.sort();
    }

    grouped.sort_by(|left, right| {
        route_sort_key(resource, left)
            .cmp(&route_sort_key(resource, right))
            .then_with(|| left.tool_names.cmp(&right.tool_names))
    });
    grouped
}

fn route_sort_key(resource: &RouteResource, route: &CandidateRoute) -> (u8, u8, u8, u16, u8) {
    let match_rank = match route.match_kind {
        RouteMatchKind::Exact => 0,
        RouteMatchKind::GenericFallback => 1,
    };
    let auth_rank = if route.authenticated { 0 } else { 1 };
    let strategy_rank = match route.family {
        RouteFamily::GitHub => match route.artifact_strategy {
            ArtifactStrategy::ProbeFirst => 0,
            ArtifactStrategy::DirectFetch => 1,
        },
        RouteFamily::PublicWeb => match route.artifact_strategy {
            ArtifactStrategy::DirectFetch => 0,
            ArtifactStrategy::ProbeFirst => 1,
        },
    };
    let family_rank = match route.family {
        RouteFamily::GitHub => 0,
        RouteFamily::PublicWeb => 1,
    };

    let strategy_rank = if matches!(resource, RouteResource::GenericUrl { .. }) {
        match route.artifact_strategy {
            ArtifactStrategy::DirectFetch => 0,
            ArtifactStrategy::ProbeFirst => 1,
        }
    } else {
        strategy_rank
    };

    (
        match_rank,
        auth_rank,
        strategy_rank,
        route.fallback_rank,
        family_rank,
    )
}

fn planned_route_from_candidate(
    route: CandidateRoute,
    exact_route_present: bool,
    primary: bool,
) -> PlannedRoute {
    let reason = if primary {
        route_reason(&route, exact_route_present)
    } else if route.match_kind == RouteMatchKind::GenericFallback && exact_route_present {
        "fallback public-web route if stronger exact routes are unavailable later".to_string()
    } else {
        "fallback route ordered behind the primary route".to_string()
    };

    PlannedRoute {
        family: route.family,
        tool_names: route.tool_names,
        reason,
        authenticated: route.authenticated,
        artifact_strategy: route.artifact_strategy,
        fallback_rank: route.fallback_rank,
    }
}

fn route_reason(route: &CandidateRoute, exact_route_present: bool) -> String {
    match (
        route.match_kind,
        &route.family,
        route.authenticated,
        &route.artifact_strategy,
    ) {
        (RouteMatchKind::Exact, RouteFamily::GitHub, true, ArtifactStrategy::ProbeFirst) => {
            "selected the strongest ready authenticated GitHub probe route".to_string()
        }
        (RouteMatchKind::Exact, RouteFamily::GitHub, true, ArtifactStrategy::DirectFetch) => {
            "selected the strongest ready authenticated GitHub retrieval route".to_string()
        }
        (RouteMatchKind::Exact, RouteFamily::GitHub, false, _) => {
            "selected the strongest ready GitHub route".to_string()
        }
        (RouteMatchKind::Exact, RouteFamily::PublicWeb, _, ArtifactStrategy::DirectFetch) => {
            "selected the ready public-web fetch route".to_string()
        }
        (RouteMatchKind::Exact, RouteFamily::PublicWeb, _, ArtifactStrategy::ProbeFirst) => {
            "selected the ready public-web probe route".to_string()
        }
        (RouteMatchKind::GenericFallback, RouteFamily::PublicWeb, _, _) if exact_route_present => {
            "no ready exact route was available, so the planner fell back to public web".to_string()
        }
        (RouteMatchKind::GenericFallback, RouteFamily::PublicWeb, _, _) => {
            "no typed exact route existed, so the planner used public web".to_string()
        }
        _ => "selected the strongest ready route".to_string(),
    }
}

fn candidate_tool_for_resource(
    resource: &RouteResource,
    summary: &ToolRoutingSummary,
) -> Option<CandidateTool> {
    let metadata = &summary.metadata;
    let match_kind = if metadata.resource_kinds.contains(&resource.kind()) {
        RouteMatchKind::Exact
    } else if !matches!(resource, RouteResource::GenericUrl { .. })
        && metadata.resource_kinds.contains(&ResourceKind::GenericUrl)
    {
        RouteMatchKind::GenericFallback
    } else {
        return None;
    };

    if !operation_allowed(resource, match_kind, &metadata.operations) {
        return None;
    }

    let family = match match_kind {
        RouteMatchKind::Exact => match resource {
            RouteResource::GitHubPullRequest { .. }
            | RouteResource::GitHubIssue { .. }
            | RouteResource::GitHubRepository { .. } => RouteFamily::GitHub,
            RouteResource::GenericUrl { .. } => RouteFamily::PublicWeb,
        },
        RouteMatchKind::GenericFallback => RouteFamily::PublicWeb,
    };

    Some(CandidateTool {
        tool_name: summary.tool_name.clone(),
        family,
        match_kind,
        authenticated: !matches!(metadata.auth_mode, RouteAuthMode::None),
        artifact_strategy: metadata.artifact_strategy.clone(),
        fallback_rank: metadata.fallback_rank,
        ready: summary.readiness.available && summary.readiness.ready,
    })
}

fn operation_allowed(
    resource: &RouteResource,
    match_kind: RouteMatchKind,
    operations: &[RouteOperation],
) -> bool {
    match (resource, match_kind) {
        (RouteResource::GenericUrl { .. }, _) | (_, RouteMatchKind::GenericFallback) => {
            operations.contains(&RouteOperation::Fetch)
        }
        _ => operations
            .iter()
            .any(|operation| matches!(operation, RouteOperation::Fetch | RouteOperation::List)),
    }
}

fn first_url(user_message: &str) -> Option<String> {
    user_message
        .split_whitespace()
        .map(trim_leading_url_wrapper)
        .find(|token| token.starts_with("https://") || token.starts_with("http://"))
        .map(trim_trailing_url_punctuation)
        .filter(|token| !token.is_empty())
        .map(str::to_string)
}

fn trim_leading_url_wrapper(token: &str) -> &str {
    token.trim_start_matches(|character: char| {
        matches!(character, '"' | '\'' | '`' | '(' | '[' | '{' | '<')
    })
}

fn trim_trailing_url_punctuation(token: &str) -> &str {
    token.trim_end_matches(|character: char| {
        matches!(
            character,
            '"' | '\'' | '`' | ')' | ']' | '}' | '>' | '.' | ',' | ';' | ':' | '!'
        )
    })
}

fn classify_url(url: &str) -> Option<RouteResource> {
    let without_scheme = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;
    let without_fragment = without_scheme
        .split(['?', '#'])
        .next()
        .unwrap_or(without_scheme);
    let mut parts = without_fragment.split('/');
    let host = parts.next()?.trim().to_ascii_lowercase();
    let host = host.strip_prefix("www.").unwrap_or(host.as_str());
    let path_segments = parts
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();

    if host != "github.com" {
        return Some(RouteResource::GenericUrl {
            url: url.to_string(),
        });
    }

    match path_segments.as_slice() {
        [owner, repo, "pull", number] => Some(RouteResource::GitHubPullRequest {
            url: url.to_string(),
            owner: (*owner).to_string(),
            repo: (*repo).to_string(),
            number: number.parse().ok()?,
        }),
        [owner, repo, "issues", number] => Some(RouteResource::GitHubIssue {
            url: url.to_string(),
            owner: (*owner).to_string(),
            repo: (*repo).to_string(),
            number: number.parse().ok()?,
        }),
        [owner, repo] => Some(RouteResource::GitHubRepository {
            url: url.to_string(),
            owner: (*owner).to_string(),
            repo: (*repo).to_string(),
        }),
        _ => Some(RouteResource::GenericUrl {
            url: url.to_string(),
        }),
    }
}
