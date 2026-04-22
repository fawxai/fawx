use fx_core::command_text::normalize_http_url_token;
use fx_core::tool_routing::{
    ArtifactStrategy, ResourceKind, RouteAdvisory, RouteAdvisoryOutcome, RouteAdvisorySource,
    RouteAuthMode, RouteOperation, ToolRoutingSummary,
};
use fx_llm::ToolDefinition;
use serde::Serialize;
use std::collections::{HashMap, HashSet};

use crate::FailureClass;

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
#[serde(rename_all = "snake_case")]
pub(super) enum RouteRankingBasis {
    TypedPolicyOnly,
    TypedPolicyPlusAdvisory,
    DegradedFallback,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(super) struct RouteAdvisoryInfluence {
    pub source: RouteAdvisorySource,
    pub matched_entries: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub preferred_tools: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub avoided_tools: Vec<String>,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(super) struct PlannedRoute {
    pub family: RouteFamily,
    pub tool_names: Vec<String>,
    pub reason: String,
    pub ranking_basis: RouteRankingBasis,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub advisory_influence: Option<RouteAdvisoryInfluence>,
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
    #[serde(skip)]
    pub active_route_index: usize,
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
    advisory_score: i32,
    advisory_influence: Option<RouteAdvisoryInfluence>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct RouteSortKey {
    match_rank: u8,
    auth_rank: u8,
    strategy_rank: u8,
    fallback_rank: u16,
    family_rank: u8,
}

#[derive(Debug, Clone, Copy)]
struct MatchingToolAdvisory<'a> {
    tool_name: &'a str,
    advisory: &'a RouteAdvisory,
}

pub(super) fn detect_route_resource(user_message: &str) -> Option<RouteResource> {
    let url = first_url(user_message)?;
    classify_url(&url)
}

pub(super) fn build_route_plan(
    resource: &RouteResource,
    available_tools: &[ToolDefinition],
    routing_tools: &[ToolRoutingSummary],
    advisories: &[RouteAdvisory],
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
    let ready_routes = grouped_ready_routes(&candidates, resource, advisories);
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
        active_route_index: 0,
    })
}

pub(super) fn build_degraded_public_web_fallback_plan(
    resource: &RouteResource,
    available_tools: &[ToolDefinition],
) -> Option<RoutePlan> {
    // This degraded path is only used after typed routing fails. It keeps
    // URL-like requests out of broad tool soup without pretending a typed
    // route was available.
    let tool_names = degraded_public_web_tool_names(available_tools);
    if tool_names.is_empty() {
        return None;
    }

    Some(RoutePlan {
        resource: resource.clone(),
        primary_route: PlannedRoute {
            family: RouteFamily::PublicWeb,
            tool_names,
            reason: "no ready typed route was available, so the planner constrained the first move to a public-web fallback".to_string(),
            ranking_basis: RouteRankingBasis::DegradedFallback,
            advisory_influence: None,
            authenticated: false,
            artifact_strategy: ArtifactStrategy::DirectFetch,
            // Degraded fallback is only constructed after typed routing fails;
            // keep it behind every real typed route if plans are ever compared.
            fallback_rank: u16::MAX,
        },
        fallback_routes: Vec::new(),
        requires_probe: false,
        active_route_index: 0,
    })
}

impl RoutePlan {
    pub(super) fn current_route(&self) -> &PlannedRoute {
        match self.active_route_index {
            0 => &self.primary_route,
            index => &self.fallback_routes[index - 1],
        }
    }

    pub(super) fn advance_to_reroute(
        &mut self,
        failure_class: FailureClass,
    ) -> Option<PlannedRoute> {
        let current = self.current_route().clone();
        let mut next_index = self.active_route_index.saturating_add(1);
        while let Some(route) = self.route_at_index(next_index).cloned() {
            if failure_class.prefers_distinct_route() && routes_are_equivalent(&current, &route) {
                next_index = next_index.saturating_add(1);
                continue;
            }
            self.active_route_index = next_index;
            return Some(route);
        }
        None
    }

    fn route_at_index(&self, index: usize) -> Option<&PlannedRoute> {
        match index {
            0 => Some(&self.primary_route),
            index => self.fallback_routes.get(index - 1),
        }
    }
}

fn degraded_public_web_tool_names(available_tools: &[ToolDefinition]) -> Vec<String> {
    const PUBLIC_WEB_FALLBACK_TOOLS: [&str; 2] = ["web_fetch", "fetch_url"];

    PUBLIC_WEB_FALLBACK_TOOLS
        .into_iter()
        .filter(|tool_name| {
            available_tools
                .iter()
                .any(|tool| tool.name.as_str() == *tool_name)
        })
        .map(str::to_string)
        .collect()
}

fn routes_are_equivalent(left: &PlannedRoute, right: &PlannedRoute) -> bool {
    left.family == right.family && left.authenticated == right.authenticated
}

fn grouped_ready_routes(
    candidates: &[CandidateTool],
    resource: &RouteResource,
    advisories: &[RouteAdvisory],
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
            advisory_score: 0,
            advisory_influence: None,
        });
    }

    for route in &mut grouped {
        route.tool_names.sort();
        apply_advisory_influence(resource, route, advisories);
    }

    grouped.sort_by(|left, right| {
        route_sort_key(resource, left)
            .cmp(&route_sort_key(resource, right))
            .then_with(|| right.advisory_score.cmp(&left.advisory_score))
            .then_with(|| left.tool_names.cmp(&right.tool_names))
    });
    grouped
}

fn route_sort_key(resource: &RouteResource, route: &CandidateRoute) -> RouteSortKey {
    RouteSortKey {
        match_rank: match route.match_kind {
            RouteMatchKind::Exact => 0,
            RouteMatchKind::GenericFallback => 1,
        },
        auth_rank: if route.authenticated { 0 } else { 1 },
        strategy_rank: route_strategy_rank(resource, route),
        fallback_rank: route.fallback_rank,
        family_rank: match route.family {
            RouteFamily::GitHub => 0,
            RouteFamily::PublicWeb => 1,
        },
    }
}

fn route_strategy_rank(resource: &RouteResource, route: &CandidateRoute) -> u8 {
    let prefers_probe = matches!(
        (resource, &route.family),
        (
            RouteResource::GitHubPullRequest { .. }
                | RouteResource::GitHubIssue { .. }
                | RouteResource::GitHubRepository { .. },
            RouteFamily::GitHub
        )
    );

    match (prefers_probe, &route.artifact_strategy) {
        (true, ArtifactStrategy::ProbeFirst) => 0,
        (true, ArtifactStrategy::DirectFetch) => 1,
        (false, ArtifactStrategy::DirectFetch) => 0,
        (false, ArtifactStrategy::ProbeFirst) => 1,
    }
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
        ranking_basis: if route.advisory_influence.is_some() {
            RouteRankingBasis::TypedPolicyPlusAdvisory
        } else {
            RouteRankingBasis::TypedPolicyOnly
        },
        advisory_influence: route.advisory_influence,
        authenticated: route.authenticated,
        artifact_strategy: route.artifact_strategy,
        fallback_rank: route.fallback_rank,
    }
}

fn apply_advisory_influence(
    resource: &RouteResource,
    route: &mut CandidateRoute,
    advisories: &[RouteAdvisory],
) {
    let matching = matching_tool_advisories(resource, route, advisories);
    if matching.is_empty() {
        return;
    }

    let mut tool_scores = HashMap::<String, i32>::new();
    for advisory in &matching {
        let delta = match advisory.advisory.outcome {
            RouteAdvisoryOutcome::Prefer => 1,
            RouteAdvisoryOutcome::Avoid => -1,
            RouteAdvisoryOutcome::Neutral => 0,
        };
        *tool_scores
            .entry(advisory.tool_name.to_string())
            .or_default() += delta;
    }

    route.advisory_score = tool_scores.values().copied().sum();
    route.tool_names.sort_by(|left, right| {
        tool_scores
            .get(right.as_str())
            .copied()
            .unwrap_or_default()
            .cmp(&tool_scores.get(left.as_str()).copied().unwrap_or_default())
            .then_with(|| left.cmp(right))
    });

    let preferred_tools = tool_names_with_score(&tool_scores, true);
    let avoided_tools = tool_names_with_score(&tool_scores, false);
    let source = matching
        .first()
        .map(|advisory| advisory.advisory.source)
        .unwrap_or(RouteAdvisorySource::Journal);
    route.advisory_influence = Some(RouteAdvisoryInfluence {
        source,
        matched_entries: matching.len(),
        preferred_tools: preferred_tools.clone(),
        avoided_tools: avoided_tools.clone(),
        summary: advisory_summary(source, matching.len(), &preferred_tools, &avoided_tools),
    });
}

fn matching_tool_advisories<'a>(
    resource: &RouteResource,
    route: &CandidateRoute,
    advisories: &'a [RouteAdvisory],
) -> Vec<MatchingToolAdvisory<'a>> {
    advisories
        .iter()
        .filter(|advisory| advisory.resource_kind == resource.kind())
        .filter_map(|advisory| {
            let tool_name = advisory.tool_name.as_deref()?;
            route
                .tool_names
                .iter()
                .any(|candidate| candidate == tool_name)
                .then_some(MatchingToolAdvisory {
                    tool_name,
                    advisory,
                })
        })
        .collect()
}

fn tool_names_with_score(tool_scores: &HashMap<String, i32>, preferred: bool) -> Vec<String> {
    let mut tools = tool_scores
        .iter()
        .filter_map(|(tool_name, score)| {
            if preferred {
                (*score > 0).then_some((tool_name, *score))
            } else {
                (*score < 0).then_some((tool_name, *score))
            }
        })
        .collect::<Vec<_>>();
    // Preferred tools sort toward the highest positive score while avoided
    // tools sort toward the lowest negative score so the strongest signal in
    // either direction stays first in traces and reroute diagnostics.
    tools.sort_by(|(left_name, left_score), (right_name, right_score)| {
        if preferred {
            right_score
                .cmp(left_score)
                .then_with(|| left_name.cmp(right_name))
        } else {
            left_score
                .cmp(right_score)
                .then_with(|| left_name.cmp(right_name))
        }
    });
    tools
        .into_iter()
        .map(|(tool_name, _)| tool_name.clone())
        .collect()
}

fn advisory_summary(
    source: RouteAdvisorySource,
    matched_entries: usize,
    preferred_tools: &[String],
    avoided_tools: &[String],
) -> String {
    let source = advisory_source_label(source);
    let match_count = advisory_match_count_label(matched_entries);
    match (preferred_tools.is_empty(), avoided_tools.is_empty()) {
        (false, false) => format!(
            "{source} advisories ({match_count}) preferred {} and deprioritized {}",
            preferred_tools.join(", "),
            avoided_tools.join(", ")
        ),
        (false, true) => format!(
            "{source} advisories ({match_count}) preferred {}",
            preferred_tools.join(", ")
        ),
        (true, false) => format!(
            "{source} advisories ({match_count}) deprioritized {}",
            avoided_tools.join(", ")
        ),
        (true, true) => format!(
            "{source} matched {matched_entries} advisory memories without changing tool rank"
        ),
    }
}

fn advisory_match_count_label(matched_entries: usize) -> String {
    match matched_entries {
        1 => "1 match".to_string(),
        count => format!("{count} matches"),
    }
}

fn advisory_source_label(source: RouteAdvisorySource) -> &'static str {
    match source {
        RouteAdvisorySource::Journal => "journal",
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
        .find_map(normalize_http_url_token)
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
        [owner, repo, "pull", number, ..] => Some(RouteResource::GitHubPullRequest {
            url: url.to_string(),
            owner: (*owner).to_string(),
            repo: (*repo).to_string(),
            number: number.parse().ok()?,
        }),
        [owner, repo, "issues", number, ..] => Some(RouteResource::GitHubIssue {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_url_handles_pull_request_subpaths() {
        for url in [
            "https://github.com/fawxai/fawx/pull/1753/files",
            "https://github.com/fawxai/fawx/pull/1753/commits",
        ] {
            assert_eq!(
                classify_url(url),
                Some(RouteResource::GitHubPullRequest {
                    url: url.to_string(),
                    owner: "fawxai".to_string(),
                    repo: "fawx".to_string(),
                    number: 1753,
                })
            );
        }
    }

    #[test]
    fn classify_url_handles_common_github_url_variants() {
        let cases = [
            "https://github.com/fawxai/fawx/pull/1753?diff=split",
            "https://github.com/fawxai/fawx/pull/1753#discussion_r1",
            "https://github.com/fawxai/fawx/pull/1753/",
            "https://www.github.com/fawxai/fawx/pull/1753",
            "http://github.com/fawxai/fawx/pull/1753",
        ];

        for url in cases {
            assert_eq!(
                classify_url(url),
                Some(RouteResource::GitHubPullRequest {
                    url: url.to_string(),
                    owner: "fawxai".to_string(),
                    repo: "fawx".to_string(),
                    number: 1753,
                })
            );
        }
    }
}
