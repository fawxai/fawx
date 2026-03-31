use crate::kernel_blind::{
    is_kernel_blind_enforced, is_kernel_blind_path, normalize_relative_path,
    shell_targets_kernel_path,
};
use crate::permission_gate::PermissionPolicy;
use crate::proposal_gate::{ActiveProposal, ProposalGateState};
use fx_core::path::expand_tilde;
use fx_core::runtime_info::{AuthorityDecisionInfo, AuthorityRuntimeInfo, RuntimeInfo};
use fx_core::self_modify::{classify_path, classify_write_domain, PathTier, WriteDomain};
use fx_llm::ToolCall;
use serde::Serialize;
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

const CACHED_DECISION_TTL: Duration = Duration::from_secs(300);
const RECENT_DECISION_LIMIT: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthorityEffect {
    Read,
    Write,
    Delete,
    Execute,
    Network,
    None,
}

impl AuthorityEffect {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Write => "write",
            Self::Delete => "delete",
            Self::Execute => "execute",
            Self::Network => "network",
            Self::None => "none",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthorityTargetKind {
    Path,
    Command,
    Network,
    None,
}

impl AuthorityTargetKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Path => "path",
            Self::Command => "command",
            Self::Network => "network",
            Self::None => "none",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthorityDomain {
    Project,
    SelfLoadable,
    KernelSource,
    Sovereign,
    External,
    None,
}

impl AuthorityDomain {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Project => "project",
            Self::SelfLoadable => "self_loadable",
            Self::KernelSource => "kernel_source",
            Self::Sovereign => "sovereign",
            Self::External => "external",
            Self::None => "none",
        }
    }
}

impl From<WriteDomain> for AuthorityDomain {
    fn from(value: WriteDomain) -> Self {
        match value {
            WriteDomain::Project => Self::Project,
            WriteDomain::SelfLoadable => Self::SelfLoadable,
            WriteDomain::KernelSource => Self::KernelSource,
            WriteDomain::Sovereign => Self::Sovereign,
            WriteDomain::External => Self::External,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthorityInvariant {
    KernelBlindPath,
    KernelBlindCommand,
    SovereignWriteBoundary,
}

impl AuthorityInvariant {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::KernelBlindPath => "kernel_blind_path",
            Self::KernelBlindCommand => "kernel_blind_command",
            Self::SovereignWriteBoundary => "sovereign_write_boundary",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthorityVerdict {
    Allow,
    Prompt,
    Propose,
    Deny,
}

impl AuthorityVerdict {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Allow => "allow",
            Self::Prompt => "prompt",
            Self::Propose => "propose",
            Self::Deny => "deny",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct ApprovalScope {
    pub tool_name: String,
    pub capability: String,
    pub effect: AuthorityEffect,
    pub target_kind: AuthorityTargetKind,
    pub domain: AuthorityDomain,
    pub target_identity: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AuthorityRequest {
    pub tool_name: String,
    pub capability: String,
    pub effect: AuthorityEffect,
    pub target_kind: AuthorityTargetKind,
    pub domain: AuthorityDomain,
    pub target_summary: String,
    pub target_identity: String,
    pub paths: Vec<String>,
    pub command: Option<String>,
    pub invariant: Option<AuthorityInvariant>,
}

impl AuthorityRequest {
    #[must_use]
    pub fn approval_scope(&self) -> ApprovalScope {
        ApprovalScope {
            tool_name: self.tool_name.clone(),
            capability: self.capability.clone(),
            effect: self.effect,
            target_kind: self.target_kind,
            domain: self.domain,
            target_identity: self.target_identity.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AuthorityDecision {
    pub request: AuthorityRequest,
    pub verdict: AuthorityVerdict,
    pub reason: String,
}

#[derive(Debug, Clone)]
pub struct CachedAuthorityDecision {
    pub decision: AuthorityDecision,
    pub prompt_satisfied: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AuthorityDecisionSnapshot {
    pub tool_name: String,
    pub capability: String,
    pub effect: String,
    pub target_kind: String,
    pub domain: String,
    pub target_summary: String,
    pub verdict: String,
    pub reason: String,
}

impl AuthorityDecision {
    #[must_use]
    pub fn snapshot(&self) -> AuthorityDecisionSnapshot {
        AuthorityDecisionSnapshot {
            tool_name: self.request.tool_name.clone(),
            capability: self.request.capability.clone(),
            effect: self.request.effect.as_str().to_string(),
            target_kind: self.request.target_kind.as_str().to_string(),
            domain: self.request.domain.as_str().to_string(),
            target_summary: self.request.target_summary.clone(),
            verdict: self.verdict.as_str().to_string(),
            reason: self.reason.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AuthorityStatusSnapshot {
    pub resolver: String,
    pub approval_scope: String,
    pub path_policy_source: String,
    pub capability_mode_mutates_path_policy: bool,
    pub kernel_blind_enabled: bool,
    pub sovereign_boundary_enforced: bool,
    pub active_session_approvals: usize,
    pub active_proposal_override: Option<String>,
    pub recent_decisions: Vec<AuthorityDecisionSnapshot>,
}

#[derive(Debug)]
pub struct AuthorityCoordinator {
    permissions: PermissionPolicy,
    state: std::sync::Mutex<ProposalGateState>,
    cache: std::sync::Mutex<HashMap<String, CachedEntry>>,
    recent: std::sync::Mutex<VecDeque<AuthorityDecisionSnapshot>>,
    active_session_approvals: AtomicUsize,
    runtime_info: std::sync::Mutex<Option<Arc<RwLock<RuntimeInfo>>>>,
}

#[derive(Debug, Clone)]
struct CachedEntry {
    decision: CachedAuthorityDecision,
    created_at: Instant,
}

impl AuthorityCoordinator {
    #[must_use]
    pub fn new(permissions: PermissionPolicy, state: ProposalGateState) -> Self {
        Self {
            permissions,
            state: std::sync::Mutex::new(state),
            cache: std::sync::Mutex::new(HashMap::new()),
            recent: std::sync::Mutex::new(VecDeque::new()),
            active_session_approvals: AtomicUsize::new(0),
            runtime_info: std::sync::Mutex::new(None),
        }
    }

    pub fn attach_runtime_info(&self, runtime_info: Arc<RwLock<RuntimeInfo>>) {
        let mut slot = self
            .runtime_info
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        *slot = Some(runtime_info);
        drop(slot);
        self.publish_runtime_info();
    }

    pub fn set_active_session_approvals(&self, count: usize) {
        self.active_session_approvals
            .store(count, Ordering::Relaxed);
    }

    #[must_use]
    pub fn classify_call(
        &self,
        call: &ToolCall,
        fallback_capability: &str,
        surface: ToolAuthoritySurface,
    ) -> AuthorityRequest {
        let state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        classify_call(call, fallback_capability, state.working_dir(), surface)
    }

    #[must_use]
    pub fn resolve_request(
        &self,
        request: AuthorityRequest,
        session_approved: bool,
    ) -> AuthorityDecision {
        let state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        let decision = resolve_request(
            request,
            &self.permissions,
            &state,
            session_approved,
            is_kernel_blind_enforced(),
        );
        drop(state);
        self.record_decision(&decision);
        decision
    }

    pub fn cache_decision(
        &self,
        call_id: &str,
        decision: AuthorityDecision,
        prompt_satisfied: bool,
    ) {
        let mut cache = self.cache.lock().unwrap_or_else(|error| error.into_inner());
        clean_cache(&mut cache);
        cache.insert(
            call_id.to_string(),
            CachedEntry {
                decision: CachedAuthorityDecision {
                    decision,
                    prompt_satisfied,
                },
                created_at: Instant::now(),
            },
        );
    }

    pub fn consume_decision(&self, call_id: &str) -> Option<CachedAuthorityDecision> {
        let mut cache = self.cache.lock().unwrap_or_else(|error| error.into_inner());
        clean_cache(&mut cache);
        cache.remove(call_id).map(|entry| entry.decision)
    }

    pub fn set_active_proposal(&self, proposal: ActiveProposal) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state.set_active_proposal(proposal);
    }

    pub fn clear_active_proposal(&self) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state.clear_active_proposal();
    }

    #[must_use]
    pub fn working_dir(&self) -> PathBuf {
        self.state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .working_dir()
            .to_path_buf()
    }

    #[must_use]
    pub fn proposals_dir(&self) -> PathBuf {
        self.state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .proposals_dir()
            .to_path_buf()
    }

    #[must_use]
    pub fn status_snapshot(&self) -> AuthorityStatusSnapshot {
        let state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        let recent = self
            .recent
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        AuthorityStatusSnapshot {
            resolver: "unified".to_string(),
            approval_scope: "classified_request_identity".to_string(),
            path_policy_source: "self_modify_config".to_string(),
            capability_mode_mutates_path_policy: false,
            kernel_blind_enabled: is_kernel_blind_enforced(),
            sovereign_boundary_enforced: true,
            active_session_approvals: self.active_session_approvals(),
            active_proposal_override: state.active_proposal().map(|proposal| proposal.id.clone()),
            recent_decisions: recent.iter().cloned().collect(),
        }
    }

    pub fn publish_runtime_info(&self) {
        let snapshot = self.status_snapshot();
        let authority_info = AuthorityRuntimeInfo {
            resolver: snapshot.resolver,
            approval_scope: snapshot.approval_scope,
            path_policy_source: snapshot.path_policy_source,
            capability_mode_mutates_path_policy: snapshot.capability_mode_mutates_path_policy,
            kernel_blind_enabled: snapshot.kernel_blind_enabled,
            sovereign_boundary_enforced: snapshot.sovereign_boundary_enforced,
            active_session_approvals: snapshot.active_session_approvals,
            active_proposal_override: snapshot.active_proposal_override,
            recent_decisions: snapshot
                .recent_decisions
                .into_iter()
                .map(|decision| AuthorityDecisionInfo {
                    tool_name: decision.tool_name,
                    capability: decision.capability,
                    effect: decision.effect,
                    target_kind: decision.target_kind,
                    domain: decision.domain,
                    target_summary: decision.target_summary,
                    verdict: decision.verdict,
                    reason: decision.reason,
                })
                .collect(),
        };
        let runtime_info = self
            .runtime_info
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone();
        let Some(runtime_info) = runtime_info else {
            return;
        };
        if let Ok(mut info) = runtime_info.write() {
            info.authority = Some(authority_info);
        };
    }

    fn record_decision(&self, decision: &AuthorityDecision) {
        let mut recent = self
            .recent
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        recent.push_front(decision.snapshot());
        while recent.len() > RECENT_DECISION_LIMIT {
            recent.pop_back();
        }
        drop(recent);
        self.publish_runtime_info();
    }

    fn active_session_approvals(&self) -> usize {
        self.active_session_approvals.load(Ordering::Relaxed)
    }
}

fn clean_cache(cache: &mut HashMap<String, CachedEntry>) {
    cache.retain(|_, entry| entry.created_at.elapsed() <= CACHED_DECISION_TTL);
}

fn classify_call(
    call: &ToolCall,
    fallback_capability: &str,
    working_dir: &Path,
    surface: ToolAuthoritySurface,
) -> AuthorityRequest {
    let capability = capability_for_call(call, fallback_capability, working_dir, surface);
    match surface {
        ToolAuthoritySurface::PathRead
        | ToolAuthoritySurface::PathWrite
        | ToolAuthoritySurface::PathDelete => {
            classify_path_request(call, capability, surface.effect(), working_dir)
        }
        ToolAuthoritySurface::GitCheckpoint => {
            classify_git_checkpoint_request(call, capability, working_dir)
        }
        ToolAuthoritySurface::Command => classify_command_request(call, capability),
        ToolAuthoritySurface::Network => {
            classify_network_request(call, capability, surface.effect())
        }
        ToolAuthoritySurface::Other => classify_none_request(call, capability, surface.effect()),
    }
}

fn capability_for_call(
    call: &ToolCall,
    fallback_capability: &str,
    working_dir: &Path,
    surface: ToolAuthoritySurface,
) -> String {
    if matches!(
        surface,
        ToolAuthoritySurface::PathWrite | ToolAuthoritySurface::PathDelete
    ) {
        return path_capability(call, working_dir);
    }
    if matches!(surface, ToolAuthoritySurface::PathRead)
        && read_targets_outside_workspace(call, working_dir)
    {
        return "outside_workspace".to_string();
    }
    if matches!(surface, ToolAuthoritySurface::GitCheckpoint) {
        return "git".to_string();
    }
    fallback_capability.to_string()
}

fn path_capability(call: &ToolCall, working_dir: &Path) -> String {
    extract_path(call)
        .map(|path| classify_write_domain(&expand_tilde(path), working_dir).permission_category())
        .unwrap_or("file_write")
        .to_string()
}

fn read_targets_outside_workspace(call: &ToolCall, working_dir: &Path) -> bool {
    extract_path(call)
        .map(|path| {
            classify_write_domain(&expand_tilde(path), working_dir) == WriteDomain::External
        })
        .unwrap_or(false)
}

fn classify_path_request(
    call: &ToolCall,
    capability: String,
    effect: AuthorityEffect,
    working_dir: &Path,
) -> AuthorityRequest {
    let Some(path) = extract_path(call) else {
        return classify_none_request(call, capability, effect);
    };
    let expanded = expand_tilde(path);
    let domain = AuthorityDomain::from(classify_write_domain(&expanded, working_dir));
    let relative = normalize_relative_to_base(&expanded, working_dir);
    let invariant = classify_path_invariant(effect, &relative, domain);
    AuthorityRequest {
        tool_name: call.name.clone(),
        capability,
        effect,
        target_kind: AuthorityTargetKind::Path,
        domain,
        target_summary: relative.clone(),
        target_identity: relative.clone(),
        paths: vec![relative],
        command: None,
        invariant,
    }
}

fn classify_path_invariant(
    effect: AuthorityEffect,
    relative: &str,
    domain: AuthorityDomain,
) -> Option<AuthorityInvariant> {
    if matches!(effect, AuthorityEffect::Write | AuthorityEffect::Delete)
        && domain == AuthorityDomain::Sovereign
    {
        return Some(AuthorityInvariant::SovereignWriteBoundary);
    }
    if is_kernel_blind_path(relative) {
        return Some(AuthorityInvariant::KernelBlindPath);
    }
    None
}

fn classify_git_checkpoint_request(
    call: &ToolCall,
    capability: String,
    working_dir: &Path,
) -> AuthorityRequest {
    let paths = extract_path(call)
        .map(|path| vec![normalize_relative_to_base(&expand_tilde(path), working_dir)])
        .unwrap_or_else(|| git_checkpoint_paths(working_dir));
    let domain = strongest_domain_for_paths(&paths, working_dir);
    let invariant = if domain == AuthorityDomain::Sovereign {
        Some(AuthorityInvariant::SovereignWriteBoundary)
    } else if paths.iter().any(|path| is_kernel_blind_path(path)) {
        Some(AuthorityInvariant::KernelBlindPath)
    } else {
        None
    };
    let summary = git_checkpoint_summary(&paths);
    AuthorityRequest {
        tool_name: call.name.clone(),
        capability,
        effect: AuthorityEffect::Write,
        target_kind: AuthorityTargetKind::Path,
        domain,
        target_summary: summary.clone(),
        target_identity: summary,
        paths,
        command: None,
        invariant,
    }
}

fn classify_command_request(call: &ToolCall, capability: String) -> AuthorityRequest {
    let command = call
        .arguments
        .get("command")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
        .to_string();
    let invariant = command_invariant(&command);
    let domain = command_domain(invariant);
    AuthorityRequest {
        tool_name: call.name.clone(),
        capability,
        effect: AuthorityEffect::Execute,
        target_kind: AuthorityTargetKind::Command,
        domain,
        target_summary: command.clone(),
        target_identity: command.clone(),
        paths: Vec::new(),
        command: Some(command),
        invariant,
    }
}

fn command_invariant(command: &str) -> Option<AuthorityInvariant> {
    if shell_targets_kernel_path(command) {
        return Some(AuthorityInvariant::KernelBlindCommand);
    }
    None
}

fn command_domain(invariant: Option<AuthorityInvariant>) -> AuthorityDomain {
    match invariant {
        Some(AuthorityInvariant::KernelBlindCommand) => AuthorityDomain::KernelSource,
        _ => AuthorityDomain::None,
    }
}

fn classify_network_request(
    call: &ToolCall,
    capability: String,
    effect: AuthorityEffect,
) -> AuthorityRequest {
    let target = call
        .arguments
        .get("url")
        .and_then(serde_json::Value::as_str)
        .or_else(|| {
            call.arguments
                .get("query")
                .and_then(serde_json::Value::as_str)
        })
        .unwrap_or(call.name.as_str())
        .to_string();
    AuthorityRequest {
        tool_name: call.name.clone(),
        capability,
        effect,
        target_kind: AuthorityTargetKind::Network,
        domain: AuthorityDomain::External,
        target_summary: target.clone(),
        target_identity: target,
        paths: Vec::new(),
        command: None,
        invariant: None,
    }
}

fn classify_none_request(
    call: &ToolCall,
    capability: String,
    effect: AuthorityEffect,
) -> AuthorityRequest {
    AuthorityRequest {
        tool_name: call.name.clone(),
        capability,
        effect,
        target_kind: AuthorityTargetKind::None,
        domain: AuthorityDomain::None,
        target_summary: call.name.clone(),
        target_identity: call.name.clone(),
        paths: Vec::new(),
        command: None,
        invariant: None,
    }
}

fn resolve_request(
    request: AuthorityRequest,
    permissions: &PermissionPolicy,
    state: &ProposalGateState,
    session_approved: bool,
    kernel_blind_enabled: bool,
) -> AuthorityDecision {
    if let Some(decision) = resolve_invariant(&request, kernel_blind_enabled) {
        return decision;
    }
    if has_active_proposal_override(&request, state.active_proposal()) {
        return decision(request, AuthorityVerdict::Allow, "active proposal override");
    }
    if let Some(decision) = resolve_path_policy(&request, state) {
        return decision;
    }
    if session_approved {
        return decision(request, AuthorityVerdict::Allow, "session approval scope");
    }
    resolve_permission_policy(request, permissions)
}

fn resolve_invariant(
    request: &AuthorityRequest,
    kernel_blind_enabled: bool,
) -> Option<AuthorityDecision> {
    match request.invariant {
        Some(AuthorityInvariant::SovereignWriteBoundary) => Some(decision(
            request.clone(),
            AuthorityVerdict::Deny,
            "sovereign write boundary",
        )),
        Some(AuthorityInvariant::KernelBlindPath | AuthorityInvariant::KernelBlindCommand)
            if kernel_blind_enabled =>
        {
            Some(decision(
                request.clone(),
                AuthorityVerdict::Deny,
                "kernel blind invariant",
            ))
        }
        _ => None,
    }
}

fn has_active_proposal_override(
    request: &AuthorityRequest,
    active: Option<&ActiveProposal>,
) -> bool {
    let Some(proposal) = active else {
        return false;
    };
    proposal_covers_request(proposal, &request.paths)
}

fn proposal_covers_request(proposal: &ActiveProposal, paths: &[String]) -> bool {
    if let Some(expires_at) = proposal.expires_at {
        if current_epoch_seconds() > expires_at {
            return false;
        }
    }
    if paths.is_empty() {
        return false;
    }
    paths
        .iter()
        .all(|path| proposal_covers_path(proposal, path))
}

fn proposal_covers_path(proposal: &ActiveProposal, path: &str) -> bool {
    proposal.allowed_paths.iter().any(|allowed| {
        normalize_relative_path(&allowed.to_string_lossy()) == normalize_relative_path(path)
    })
}

fn resolve_path_policy(
    request: &AuthorityRequest,
    state: &ProposalGateState,
) -> Option<AuthorityDecision> {
    if !matches!(
        request.effect,
        AuthorityEffect::Write | AuthorityEffect::Delete
    ) {
        return None;
    }
    if request.paths.is_empty() {
        return None;
    }
    match strongest_path_tier(&request.paths, state.working_dir(), state.config()) {
        PathTier::Allow => None,
        PathTier::Propose => Some(decision(
            request.clone(),
            AuthorityVerdict::Propose,
            "path policy requires proposal",
        )),
        PathTier::Deny => Some(decision(
            request.clone(),
            AuthorityVerdict::Deny,
            "path policy denied request",
        )),
    }
}

fn strongest_path_tier(
    paths: &[String],
    working_dir: &Path,
    config: &fx_core::self_modify::SelfModifyConfig,
) -> PathTier {
    paths
        .iter()
        .map(|path| classify_path(Path::new(path), working_dir, config))
        .max_by_key(path_tier_rank)
        .unwrap_or(PathTier::Allow)
}

fn path_tier_rank(tier: &PathTier) -> u8 {
    match tier {
        PathTier::Allow => 0,
        PathTier::Propose => 1,
        PathTier::Deny => 2,
    }
}

fn resolve_permission_policy(
    request: AuthorityRequest,
    permissions: &PermissionPolicy,
) -> AuthorityDecision {
    if !permissions.requires_asking(&request.capability) {
        return decision(request, AuthorityVerdict::Allow, "unrestricted capability");
    }
    let verdict = match permissions.mode {
        fx_config::CapabilityMode::Capability => AuthorityVerdict::Deny,
        fx_config::CapabilityMode::Prompt => AuthorityVerdict::Prompt,
    };
    let reason = match verdict {
        AuthorityVerdict::Prompt => "approval required by permission policy",
        AuthorityVerdict::Deny => "capability mode denied restricted request",
        _ => "unrestricted capability",
    };
    decision(request, verdict, reason)
}

fn decision(
    request: AuthorityRequest,
    verdict: AuthorityVerdict,
    reason: &str,
) -> AuthorityDecision {
    AuthorityDecision {
        request,
        verdict,
        reason: reason.to_string(),
    }
}

fn extract_path(call: &ToolCall) -> Option<&str> {
    call.arguments
        .get("path")
        .and_then(serde_json::Value::as_str)
}

fn git_checkpoint_paths(working_dir: &Path) -> Vec<String> {
    git_status_paths(working_dir)
}

fn git_status_paths(working_dir: &Path) -> Vec<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(working_dir)
        .args(["status", "--porcelain", "--untracked-files=all"])
        .output();
    let Ok(output) = output else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(parse_porcelain_path)
        .collect()
}

fn parse_porcelain_path(line: &str) -> Option<String> {
    if line.len() < 4 {
        return None;
    }
    let candidate = line.get(3..)?.trim();
    if candidate.is_empty() {
        return None;
    }
    Some(
        candidate
            .rsplit(" -> ")
            .next()
            .map(normalize_relative_path)
            .unwrap_or_else(|| normalize_relative_path(candidate)),
    )
}

fn strongest_domain_for_paths(paths: &[String], working_dir: &Path) -> AuthorityDomain {
    paths
        .iter()
        .map(|path| classify_write_domain(Path::new(path), working_dir))
        .map(AuthorityDomain::from)
        .max_by_key(domain_rank)
        .unwrap_or(AuthorityDomain::Project)
}

fn domain_rank(domain: &AuthorityDomain) -> u8 {
    match domain {
        AuthorityDomain::Project => 0,
        AuthorityDomain::SelfLoadable => 1,
        AuthorityDomain::KernelSource => 2,
        AuthorityDomain::Sovereign => 3,
        AuthorityDomain::External => 4,
        AuthorityDomain::None => 0,
    }
}

fn git_checkpoint_summary(paths: &[String]) -> String {
    if paths.is_empty() {
        return "git checkpoint (clean working tree)".to_string();
    }
    format!("git checkpoint [{}]", paths.join(","))
}

fn normalize_relative_to_base(path: &Path, base_dir: &Path) -> String {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        base_dir.join(path)
    };
    absolute
        .strip_prefix(base_dir)
        .map(|relative| normalize_relative_path(&relative.to_string_lossy()))
        .unwrap_or_else(|_| normalize_relative_path(&absolute.to_string_lossy()))
}

fn current_epoch_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ToolAuthoritySurface {
    PathRead,
    PathWrite,
    PathDelete,
    GitCheckpoint,
    Command,
    Network,
    Other,
}

impl ToolAuthoritySurface {
    const fn effect(self) -> AuthorityEffect {
        match self {
            Self::PathRead => AuthorityEffect::Read,
            Self::PathWrite | Self::GitCheckpoint => AuthorityEffect::Write,
            Self::PathDelete => AuthorityEffect::Delete,
            Self::Command => AuthorityEffect::Execute,
            Self::Network => AuthorityEffect::Network,
            Self::Other => AuthorityEffect::None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fx_config::CapabilityMode;
    use fx_core::runtime_info::ConfigSummary;
    use fx_core::self_modify::SelfModifyConfig;
    use std::collections::HashSet;

    fn policy(mode: CapabilityMode) -> PermissionPolicy {
        PermissionPolicy {
            unrestricted: HashSet::from([
                "read_any".to_string(),
                "web_search".to_string(),
                "web_fetch".to_string(),
            ]),
            ask_required: HashSet::from([
                "file_write".to_string(),
                "git".to_string(),
                "shell".to_string(),
                "code_execute".to_string(),
                "self_modify".to_string(),
                "kernel_modify".to_string(),
                "outside_workspace".to_string(),
            ]),
            default_ask: matches!(mode, CapabilityMode::Prompt),
            mode,
        }
    }

    fn state() -> ProposalGateState {
        let config = SelfModifyConfig {
            enabled: true,
            allow_paths: vec!["README.md".to_string(), "docs/**".to_string()],
            propose_paths: vec![
                ".fawx/**".to_string(),
                "engine/**".to_string(),
                "config.toml".to_string(),
            ],
            deny_paths: vec![".git/**".to_string()],
            ..SelfModifyConfig::default()
        };
        ProposalGateState::new(
            config,
            PathBuf::from("/repo"),
            PathBuf::from("/tmp/proposals"),
        )
    }

    fn call(name: &str, arguments: serde_json::Value) -> ToolCall {
        ToolCall {
            id: format!("call-{name}"),
            name: name.to_string(),
            arguments,
        }
    }

    #[test]
    fn classifies_write_request_with_domain_and_scope() {
        let coordinator = AuthorityCoordinator::new(policy(CapabilityMode::Prompt), state());
        let request = coordinator.classify_call(
            &call(
                "write_file",
                serde_json::json!({"path":"README.md","content":"x"}),
            ),
            "file_write",
            ToolAuthoritySurface::PathWrite,
        );

        assert_eq!(request.capability, "file_write");
        assert_eq!(request.effect, AuthorityEffect::Write);
        assert_eq!(request.target_kind, AuthorityTargetKind::Path);
        assert_eq!(request.domain, AuthorityDomain::Project);
        assert_eq!(request.target_identity, "README.md");
    }

    #[test]
    fn resolves_project_write_to_prompt_when_permission_requires_it() {
        let coordinator = AuthorityCoordinator::new(policy(CapabilityMode::Prompt), state());
        let request = coordinator.classify_call(
            &call(
                "write_file",
                serde_json::json!({"path":"README.md","content":"x"}),
            ),
            "file_write",
            ToolAuthoritySurface::PathWrite,
        );

        let decision = coordinator.resolve_request(request, false);

        assert_eq!(decision.verdict, AuthorityVerdict::Prompt);
        assert_eq!(decision.reason, "approval required by permission policy");
    }

    #[test]
    fn resolves_self_modify_write_to_propose_before_prompt() {
        let coordinator = AuthorityCoordinator::new(policy(CapabilityMode::Prompt), state());
        let request = coordinator.classify_call(
            &call(
                "write_file",
                serde_json::json!({"path":"engine/crates/fx-kernel/src/lib.rs","content":"x"}),
            ),
            "file_write",
            ToolAuthoritySurface::PathWrite,
        );

        let decision = coordinator.resolve_request(request, false);

        assert_eq!(decision.request.capability, "kernel_modify");
        assert_eq!(decision.verdict, AuthorityVerdict::Propose);
    }

    #[test]
    fn resolves_kernel_blind_read_to_deny_even_if_read_is_unrestricted() {
        let permissions = policy(CapabilityMode::Capability);
        let proposal_state = state();
        let coordinator = AuthorityCoordinator::new(policy(CapabilityMode::Capability), state());
        let request = coordinator.classify_call(
            &call(
                "read_file",
                serde_json::json!({"path":"engine/crates/fx-kernel/src/lib.rs"}),
            ),
            "read_any",
            ToolAuthoritySurface::PathRead,
        );

        let decision = resolve_request(request, &permissions, &proposal_state, false, true);

        assert_eq!(decision.verdict, AuthorityVerdict::Deny);
        assert_eq!(decision.reason, "kernel blind invariant");
    }

    #[test]
    fn resolves_session_scoped_approval_on_exact_request_identity() {
        let coordinator = AuthorityCoordinator::new(policy(CapabilityMode::Prompt), state());
        let request = coordinator.classify_call(
            &call(
                "write_file",
                serde_json::json!({"path":"README.md","content":"x"}),
            ),
            "file_write",
            ToolAuthoritySurface::PathWrite,
        );

        let decision = coordinator.resolve_request(request.clone(), true);

        assert_eq!(decision.verdict, AuthorityVerdict::Allow);
        assert_eq!(
            request.approval_scope().target_identity,
            "README.md".to_string()
        );
    }

    #[test]
    fn request_identity_changes_across_surfaces_for_same_tool_name() {
        let coordinator = AuthorityCoordinator::new(policy(CapabilityMode::Prompt), state());
        let project = coordinator.classify_call(
            &call(
                "write_file",
                serde_json::json!({"path":"README.md","content":"x"}),
            ),
            "file_write",
            ToolAuthoritySurface::PathWrite,
        );
        let kernel = coordinator.classify_call(
            &call(
                "write_file",
                serde_json::json!({"path":"engine/crates/fx-kernel/src/lib.rs","content":"x"}),
            ),
            "file_write",
            ToolAuthoritySurface::PathWrite,
        );

        assert_ne!(project.approval_scope(), kernel.approval_scope());
        assert_eq!(project.capability, "file_write");
        assert_eq!(kernel.capability, "kernel_modify");
    }

    #[test]
    fn classifies_path_write_from_declared_surface_not_tool_name() {
        let coordinator = AuthorityCoordinator::new(policy(CapabilityMode::Prompt), state());
        let request = coordinator.classify_call(
            &call(
                "custom_writer",
                serde_json::json!({"path":"README.md","content":"x"}),
            ),
            "file_write",
            ToolAuthoritySurface::PathWrite,
        );

        assert_eq!(request.effect, AuthorityEffect::Write);
        assert_eq!(request.target_kind, AuthorityTargetKind::Path);
        assert_eq!(request.target_identity, "README.md");
    }

    #[test]
    fn ignores_matching_tool_name_when_declared_surface_is_other() {
        let coordinator = AuthorityCoordinator::new(policy(CapabilityMode::Prompt), state());
        let request = coordinator.classify_call(
            &call(
                "write_file",
                serde_json::json!({"path":"README.md","content":"x"}),
            ),
            "file_write",
            ToolAuthoritySurface::Other,
        );

        assert_eq!(request.effect, AuthorityEffect::None);
        assert_eq!(request.target_kind, AuthorityTargetKind::None);
        assert!(request.paths.is_empty());
    }

    #[test]
    fn capability_mode_does_not_mutate_path_policy_source() {
        let coordinator = AuthorityCoordinator::new(policy(CapabilityMode::Capability), state());
        let snapshot = coordinator.status_snapshot();

        assert_eq!(snapshot.path_policy_source, "self_modify_config");
        assert!(!snapshot.capability_mode_mutates_path_policy);
    }

    #[test]
    fn shell_command_detects_kernel_blind_command_invariant() {
        let coordinator = AuthorityCoordinator::new(policy(CapabilityMode::Prompt), state());
        let request = coordinator.classify_call(
            &call(
                "run_command",
                serde_json::json!({"command":"rg TODO engine/crates/fx-kernel/src"}),
            ),
            "code_execute",
            ToolAuthoritySurface::Command,
        );

        assert_eq!(
            request.invariant,
            Some(AuthorityInvariant::KernelBlindCommand)
        );
        assert_eq!(request.domain, AuthorityDomain::KernelSource);
    }

    #[test]
    fn status_snapshot_tracks_recent_decisions() {
        let coordinator = AuthorityCoordinator::new(policy(CapabilityMode::Prompt), state());
        let request = coordinator.classify_call(
            &call(
                "write_file",
                serde_json::json!({"path":"README.md","content":"x"}),
            ),
            "file_write",
            ToolAuthoritySurface::PathWrite,
        );
        let _ = coordinator.resolve_request(request, false);

        coordinator.set_active_session_approvals(1);
        let snapshot = coordinator.status_snapshot();

        assert_eq!(snapshot.resolver, "unified");
        assert_eq!(snapshot.active_session_approvals, 1);
        assert_eq!(snapshot.recent_decisions.len(), 1);
        assert_eq!(snapshot.recent_decisions[0].verdict, "prompt");
    }

    #[test]
    fn runtime_info_reports_active_session_approvals_after_recording_decision() {
        let coordinator = AuthorityCoordinator::new(policy(CapabilityMode::Prompt), state());
        let runtime_info = Arc::new(RwLock::new(RuntimeInfo {
            active_model: String::new(),
            provider: String::new(),
            skills: Vec::new(),
            config_summary: ConfigSummary {
                max_iterations: 10,
                max_history: 20,
                memory_enabled: true,
            },
            authority: None,
            version: "test".to_string(),
        }));
        coordinator.set_active_session_approvals(2);
        coordinator.attach_runtime_info(Arc::clone(&runtime_info));

        let request = coordinator.classify_call(
            &call(
                "write_file",
                serde_json::json!({"path":"README.md","content":"x"}),
            ),
            "file_write",
            ToolAuthoritySurface::PathWrite,
        );
        let _ = coordinator.resolve_request(request, false);

        let snapshot = runtime_info
            .read()
            .expect("runtime info lock")
            .authority
            .clone()
            .expect("authority runtime info");
        assert_eq!(snapshot.active_session_approvals, 2);
        assert_eq!(snapshot.recent_decisions.len(), 1);
        assert_eq!(snapshot.recent_decisions[0].verdict, "prompt");
    }
}
