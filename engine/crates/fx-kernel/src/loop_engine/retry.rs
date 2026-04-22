use crate::act::{FailureClass, ToolCallClassification, ToolExecutor, ToolResult};
use crate::budget::RetryPolicyConfig;
use fx_core::command_text::{normalize_http_url_token, tokenize_non_shell_command};
use fx_llm::ToolCall;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, PartialEq, Eq)]
struct NoProgressState {
    last_result_hash: u64,
    consecutive_same: u16,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct RetryTracker {
    signature_failures: HashMap<ToolCallKey, FailureState>,
    cycle_total_failures: u16,
    no_progress: HashMap<ToolCallKey, NoProgressState>,
    completed_successes: HashMap<ToolCallKey, CompletedSuccessState>,
    mutation_generation: u64,
    mutation_families: HashMap<MutationFamilyKey, MutationFamilyState>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct FailureState {
    consecutive_failures: u16,
    last_failure_class: Option<FailureClass>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct MutationFamilyState {
    attempts: u16,
    last_failure_class: Option<FailureClass>,
    completed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CompletedSuccessState {
    classification: ToolCallClassification,
    mutation_generation: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct RetryAttempt {
    pub(super) prior_failures: u16,
    pub(super) attempt: u16,
    pub(super) failure_class: Option<FailureClass>,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub(super) struct ToolCallKey {
    tool_name: String,
    args_hash: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum RetryVerdict {
    Allow,
    Block {
        kind: RetryBlockKind,
        reason: String,
        failure_class: Option<FailureClass>,
        guidance: Option<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RetryBlockKind {
    PermanentFailure,
    CycleFailureLimit,
    SameCallFailureLimit,
    NoProgress,
    DuplicateSuccess,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum MutationGuardBlockKind {
    DuplicateVariant,
    CompletedIntent,
    FamilyFailure,
    RetryLimit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BlockedToolSource {
    Policy,
    Retry(RetryBlockKind),
    MutationGuard(MutationGuardBlockKind),
}

impl RetryBlockKind {
    pub(super) const fn as_str(self) -> &'static str {
        match self {
            Self::PermanentFailure => "permanent_failure",
            Self::CycleFailureLimit => "cycle_failure_limit",
            Self::SameCallFailureLimit => "same_call_failure_limit",
            Self::NoProgress => "no_progress",
            Self::DuplicateSuccess => "duplicate_success",
        }
    }
}

impl MutationGuardBlockKind {
    pub(super) const fn as_str(self) -> &'static str {
        match self {
            Self::DuplicateVariant => "duplicate_variant",
            Self::CompletedIntent => "completed_intent",
            Self::FamilyFailure => "family_failure",
            Self::RetryLimit => "retry_limit",
        }
    }
}

impl BlockedToolSource {
    pub(super) const fn as_str(self) -> &'static str {
        match self {
            Self::Policy => "policy",
            Self::Retry(_) => "retry_policy",
            Self::MutationGuard(_) => "mutation_guard",
        }
    }

    pub(super) const fn block_kind(self) -> Option<&'static str> {
        match self {
            Self::Policy => None,
            Self::Retry(kind) => Some(kind.as_str()),
            Self::MutationGuard(kind) => Some(kind.as_str()),
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct BlockedToolCall {
    pub(super) call: ToolCall,
    pub(super) source: BlockedToolSource,
    pub(super) reason: String,
    pub(super) failure_class: Option<FailureClass>,
    pub(super) guidance: Option<String>,
}

impl BlockedToolCall {
    pub(super) fn retry(
        call: ToolCall,
        kind: RetryBlockKind,
        reason: String,
        failure_class: Option<FailureClass>,
        guidance: Option<String>,
    ) -> Self {
        Self {
            call,
            source: BlockedToolSource::Retry(kind),
            reason,
            failure_class,
            guidance,
        }
    }

    pub(super) fn policy(call: ToolCall, reason: impl Into<String>) -> Self {
        Self::policy_with_details(call, reason, Some(FailureClass::Permanent), None)
    }

    pub(super) fn policy_with_details(
        call: ToolCall,
        reason: impl Into<String>,
        failure_class: Option<FailureClass>,
        guidance: Option<String>,
    ) -> Self {
        Self {
            call,
            source: BlockedToolSource::Policy,
            reason: reason.into(),
            failure_class,
            guidance,
        }
    }

    pub(super) fn mutation_guard(
        call: ToolCall,
        kind: MutationGuardBlockKind,
        reason: String,
        failure_class: Option<FailureClass>,
        guidance: Option<String>,
    ) -> Self {
        Self {
            call,
            source: BlockedToolSource::MutationGuard(kind),
            reason,
            failure_class,
            guidance,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct MutationGuardTerminal {
    pub(super) family: String,
    pub(super) reason: String,
    pub(super) failure_class: Option<FailureClass>,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
enum MutationFamilyKey {
    BrowserOpenApplication(String),
    BrowserOpenUrl(String),
}

impl RetryTracker {
    fn should_allow(&self, call: &ToolCall, config: &RetryPolicyConfig) -> RetryVerdict {
        if self
            .last_failure_class_for(call)
            .is_some_and(FailureClass::is_permanent)
        {
            return RetryVerdict::Block {
                kind: RetryBlockKind::PermanentFailure,
                reason: permanent_failure_reason(),
                failure_class: self.last_failure_class_for(call),
                guidance: blocked_run_command_guidance(
                    call.name.as_str(),
                    self.last_failure_class_for(call),
                ),
            };
        }

        if self.cycle_total_failures >= config.max_cycle_failures {
            return RetryVerdict::Block {
                kind: RetryBlockKind::CycleFailureLimit,
                reason: cycle_failure_limit_reason(),
                failure_class: None,
                guidance: blocked_run_command_guidance(call.name.as_str(), None),
            };
        }

        let signature = ToolCallKey::from_call(call);
        if let Some(state) = self.completed_successes.get(&signature) {
            let duplicate_success = match state.classification {
                ToolCallClassification::Mutation | ToolCallClassification::Orchestration => true,
                ToolCallClassification::Observation => {
                    state.mutation_generation == self.mutation_generation
                }
            };
            if duplicate_success {
                return RetryVerdict::Block {
                    kind: RetryBlockKind::DuplicateSuccess,
                    reason: duplicate_success_reason(&call.name, state.classification),
                    failure_class: None,
                    guidance: duplicate_success_guidance(call.name.as_str(), state.classification),
                };
            }
        }

        let failures = self.consecutive_failures_for(call);
        if failures >= config.max_consecutive_failures {
            return RetryVerdict::Block {
                kind: RetryBlockKind::SameCallFailureLimit,
                reason: same_call_failure_reason(failures),
                failure_class: self.last_failure_class_for(call),
                guidance: blocked_run_command_guidance(
                    call.name.as_str(),
                    self.last_failure_class_for(call),
                ),
            };
        }

        if let Some(state) = self.no_progress.get(&signature) {
            if state.consecutive_same >= config.max_no_progress {
                return RetryVerdict::Block {
                    kind: RetryBlockKind::NoProgress,
                    reason: no_progress_reason(&call.name, state.consecutive_same),
                    failure_class: None,
                    guidance: blocked_run_command_guidance(call.name.as_str(), None),
                };
            }
        }

        RetryVerdict::Allow
    }

    pub(super) fn record_results(
        &mut self,
        calls: &[ToolCall],
        results: &[ToolResult],
        executor: &dyn ToolExecutor,
    ) {
        let result_map: HashMap<&str, &ToolResult> = results
            .iter()
            .map(|result| (result.tool_call_id.as_str(), result))
            .collect();
        for call in calls {
            if let Some(result) = result_map.get(call.id.as_str()) {
                self.record_result_with_class(
                    call,
                    result.success,
                    result.failure_classification(),
                );
                if result.success {
                    self.record_success(call, executor.classify_call(call));
                    self.record_progress(call, &result.output);
                }
            }
        }
    }

    fn record_success(&mut self, call: &ToolCall, classification: ToolCallClassification) {
        if classification == ToolCallClassification::Mutation {
            self.mutation_generation = self.mutation_generation.saturating_add(1);
        }
        self.completed_successes.insert(
            ToolCallKey::from_call(call),
            CompletedSuccessState {
                classification,
                mutation_generation: self.mutation_generation,
            },
        );
    }

    pub(super) fn record_mutation_results(
        &mut self,
        calls: &[ToolCall],
        results: &[ToolResult],
        executor: &dyn ToolExecutor,
    ) {
        let result_map: HashMap<&str, &ToolResult> = results
            .iter()
            .map(|result| (result.tool_call_id.as_str(), result))
            .collect();
        for call in calls {
            if executor.classify_call(call) != ToolCallClassification::Mutation {
                continue;
            }
            let Some(family) = mutation_family_for_call(call) else {
                continue;
            };
            let Some(result) = result_map.get(call.id.as_str()) else {
                continue;
            };
            self.record_mutation_family_result(&family, result);
        }
    }

    fn record_progress(&mut self, call: &ToolCall, output: &str) {
        let signature = ToolCallKey::from_call(call);
        let result_hash = hash_string(output);
        let entry = self
            .no_progress
            .entry(signature)
            .or_insert(NoProgressState {
                last_result_hash: result_hash,
                consecutive_same: 0,
            });
        if entry.last_result_hash == result_hash {
            entry.consecutive_same = entry.consecutive_same.saturating_add(1);
        } else {
            entry.last_result_hash = result_hash;
            entry.consecutive_same = 1;
        }
    }

    #[cfg(test)]
    pub(super) fn record_result(&mut self, call: &ToolCall, success: bool) {
        self.record_result_with_class(call, success, (!success).then_some(FailureClass::Unknown));
    }

    pub(super) fn record_result_with_class(
        &mut self,
        call: &ToolCall,
        success: bool,
        failure_class: Option<FailureClass>,
    ) {
        let signature = ToolCallKey::from_call(call);
        let entry = self.signature_failures.entry(signature).or_default();
        if success {
            entry.consecutive_failures = 0;
            entry.last_failure_class = None;
            return;
        }

        entry.consecutive_failures = entry.consecutive_failures.saturating_add(1);
        entry.last_failure_class = Some(failure_class.unwrap_or(FailureClass::Unknown));
        self.cycle_total_failures = self.cycle_total_failures.saturating_add(1);
    }

    pub(super) fn consecutive_failures_for(&self, call: &ToolCall) -> u16 {
        self.signature_failures
            .get(&ToolCallKey::from_call(call))
            .map(|state| state.consecutive_failures)
            .unwrap_or(0)
    }

    pub(super) fn last_failure_class_for(&self, call: &ToolCall) -> Option<FailureClass> {
        self.signature_failures
            .get(&ToolCallKey::from_call(call))
            .and_then(|state| state.last_failure_class)
    }

    pub(super) fn cycle_total_failures(&self) -> u16 {
        self.cycle_total_failures
    }

    pub(super) fn clear(&mut self) {
        self.signature_failures.clear();
        self.cycle_total_failures = 0;
        self.no_progress.clear();
        self.completed_successes.clear();
        self.mutation_generation = self.mutation_generation.saturating_add(1);
        self.mutation_families.clear();
    }

    pub(super) fn retry_attempt_for(&self, call: &ToolCall) -> Option<RetryAttempt> {
        let prior_failures = self.consecutive_failures_for(call);
        if prior_failures == 0 {
            return None;
        }

        Some(RetryAttempt {
            prior_failures,
            attempt: prior_failures.saturating_add(1),
            failure_class: self.last_failure_class_for(call),
        })
    }

    fn mutation_family_attempts(&self, family: &MutationFamilyKey) -> u16 {
        self.mutation_families
            .get(family)
            .map(|state| state.attempts)
            .unwrap_or(0)
    }

    fn mutation_family_block(
        &self,
        family: &MutationFamilyKey,
    ) -> Option<(
        MutationGuardBlockKind,
        String,
        Option<FailureClass>,
        Option<String>,
    )> {
        let state = self.mutation_families.get(family)?;
        if state.completed {
            return Some((
                MutationGuardBlockKind::CompletedIntent,
                completed_mutation_family_reason(family),
                Some(FailureClass::Permanent),
                Some(blocked_mutation_guidance(
                    family,
                    Some(FailureClass::Permanent),
                )),
            ));
        }

        let failure_class = state.last_failure_class?;
        if failure_class.is_permanent() || matches!(failure_class, FailureClass::Unknown) {
            return Some((
                MutationGuardBlockKind::FamilyFailure,
                deterministic_mutation_failure_reason(family, failure_class),
                Some(failure_class),
                Some(blocked_mutation_guidance(family, Some(failure_class))),
            ));
        }

        if state.attempts >= 2 {
            return Some((
                MutationGuardBlockKind::RetryLimit,
                deterministic_mutation_retry_limit_reason(family, failure_class),
                Some(failure_class),
                Some(blocked_mutation_guidance(family, Some(failure_class))),
            ));
        }

        None
    }

    fn record_mutation_family_result(&mut self, family: &MutationFamilyKey, result: &ToolResult) {
        let entry = self.mutation_families.entry(family.clone()).or_default();
        entry.attempts = entry.attempts.saturating_add(1);
        if result.success {
            entry.completed = true;
            entry.last_failure_class = None;
        } else {
            entry.last_failure_class = result.failure_classification();
        }
    }
}

impl ToolCallKey {
    pub(super) fn from_call(call: &ToolCall) -> Self {
        Self {
            tool_name: call.name.clone(),
            args_hash: hash_tool_arguments(&call.arguments),
        }
    }
}

impl MutationFamilyKey {
    fn label(&self) -> String {
        match self {
            Self::BrowserOpenApplication(browser) => {
                format!("browser application open for {browser}")
            }
            Self::BrowserOpenUrl(url) => format!("browser URL open for {url}"),
        }
    }
}

pub(super) fn partition_by_same_batch_mutation_guard_policy(
    calls: &[ToolCall],
    executor: &dyn ToolExecutor,
) -> (Vec<ToolCall>, Vec<BlockedToolCall>) {
    let mut allowed = Vec::new();
    let mut blocked = Vec::new();
    let mut seen_mutation_signatures = HashSet::new();
    let mut seen_mutation_families = HashSet::new();

    for call in calls {
        if executor.classify_call(call) != ToolCallClassification::Mutation {
            allowed.push(call.clone());
            continue;
        }

        let signature = super::tool_execution::tool_execution_signature(call);
        if !seen_mutation_signatures.insert(signature) {
            blocked.push(BlockedToolCall::mutation_guard(
                call.clone(),
                MutationGuardBlockKind::DuplicateVariant,
                duplicate_mutation_signature_reason(call),
                Some(FailureClass::Permanent),
                Some(blocked_mutation_guidance_for_call(call, None)),
            ));
            continue;
        }

        if let Some(family) = mutation_family_for_call(call) {
            if !seen_mutation_families.insert(family.clone()) {
                blocked.push(BlockedToolCall::mutation_guard(
                    call.clone(),
                    MutationGuardBlockKind::DuplicateVariant,
                    duplicate_mutation_family_reason(&family),
                    Some(FailureClass::Permanent),
                    Some(blocked_mutation_guidance(&family, None)),
                ));
                continue;
            }
        }

        allowed.push(call.clone());
    }

    (allowed, blocked)
}

pub(super) fn partition_by_mutation_guard_policy(
    calls: &[ToolCall],
    tracker: &RetryTracker,
    executor: &dyn ToolExecutor,
) -> (Vec<ToolCall>, Vec<BlockedToolCall>) {
    let (same_batch_allowed, mut blocked) =
        partition_by_same_batch_mutation_guard_policy(calls, executor);
    let mut allowed = Vec::new();

    for call in same_batch_allowed {
        if executor.classify_call(&call) != ToolCallClassification::Mutation {
            allowed.push(call);
            continue;
        }

        let Some(family) = mutation_family_for_call(&call) else {
            allowed.push(call);
            continue;
        };

        if let Some((kind, reason, failure_class, guidance)) =
            tracker.mutation_family_block(&family)
        {
            blocked.push(BlockedToolCall::mutation_guard(
                call,
                kind,
                reason,
                failure_class,
                guidance,
            ));
        } else {
            allowed.push(call);
        }
    }

    (allowed, blocked)
}

pub(super) fn mutation_guard_terminal_for_round(
    tracker: &RetryTracker,
    calls: &[ToolCall],
    results: &[ToolResult],
    blocked: &[BlockedToolCall],
    executor: &dyn ToolExecutor,
) -> Option<MutationGuardTerminal> {
    // Phase 1: if same-turn history already blocked a mutation family, honor that
    // terminal guard immediately rather than requesting another continuation round.
    for blocked_call in blocked {
        let BlockedToolSource::MutationGuard(kind) = blocked_call.source else {
            continue;
        };
        if !matches!(
            kind,
            MutationGuardBlockKind::FamilyFailure | MutationGuardBlockKind::RetryLimit
        ) {
            continue;
        }
        if executor.classify_call(&blocked_call.call) != ToolCallClassification::Mutation {
            continue;
        }
        let family = mutation_family_for_call(&blocked_call.call)
            .map(|family| family.label())
            .unwrap_or_else(|| blocked_call.call.name.clone());
        return Some(MutationGuardTerminal {
            family,
            reason: blocked_call.reason.clone(),
            failure_class: blocked_call.failure_class,
        });
    }

    // Phase 2: stop on fresh deterministic mutation failures from this round even
    // before a later round has the chance to hit the family-level block path.
    let result_map: HashMap<&str, &ToolResult> = results
        .iter()
        .map(|result| (result.tool_call_id.as_str(), result))
        .collect();
    for call in calls {
        if executor.classify_call(call) != ToolCallClassification::Mutation {
            continue;
        }
        let Some(family) = mutation_family_for_call(call) else {
            continue;
        };
        let Some(result) = result_map.get(call.id.as_str()) else {
            continue;
        };
        if result.success {
            continue;
        }
        let failure_class = result
            .failure_classification()
            .unwrap_or(FailureClass::Unknown);
        if failure_class.is_permanent() || matches!(failure_class, FailureClass::Unknown) {
            return Some(MutationGuardTerminal {
                family: family.label(),
                reason: deterministic_mutation_failure_reason(&family, failure_class),
                failure_class: Some(failure_class),
            });
        }

        if tracker.mutation_family_attempts(&family) >= 2 {
            return Some(MutationGuardTerminal {
                family: family.label(),
                reason: deterministic_mutation_retry_limit_reason(&family, failure_class),
                failure_class: Some(failure_class),
            });
        }
    }

    None
}

pub(super) fn partition_by_retry_policy(
    calls: &[ToolCall],
    tracker: &RetryTracker,
    config: &RetryPolicyConfig,
) -> (Vec<ToolCall>, Vec<BlockedToolCall>) {
    let mut allowed = Vec::new();
    let mut blocked = Vec::new();
    for call in calls {
        match tracker.should_allow(call, config) {
            RetryVerdict::Allow => allowed.push(call.clone()),
            RetryVerdict::Block {
                kind,
                reason,
                failure_class,
                guidance,
            } => blocked.push(BlockedToolCall::retry(
                call.clone(),
                kind,
                reason,
                failure_class,
                guidance,
            )),
        }
    }
    (allowed, blocked)
}

pub(super) fn same_call_failure_reason(failures: u16) -> String {
    format!("same call failed {failures} times consecutively")
}

fn cycle_failure_limit_reason() -> String {
    "too many total failures this cycle".to_string()
}

fn permanent_failure_reason() -> String {
    "previous identical call failed permanently".to_string()
}

fn no_progress_reason(tool_name: &str, count: u16) -> String {
    format!(
        "tool '{}' returned the same result {} times with identical arguments \
         — no progress detected",
        tool_name, count
    )
}

fn duplicate_success_reason(tool_name: &str, classification: ToolCallClassification) -> String {
    let action = match classification {
        ToolCallClassification::Observation => "already gathered the same evidence",
        ToolCallClassification::Orchestration => "already started the same orchestration task",
        ToolCallClassification::Mutation => "already completed the same side-effecting action",
    };
    format!("tool '{tool_name}' {action} this turn")
}

fn duplicate_success_guidance(
    tool_name: &str,
    classification: ToolCallClassification,
) -> Option<String> {
    if tool_name == "run_command" {
        return Some(
            "Use the existing command result, run a materially different command, or proceed with the current findings."
                .to_string(),
        );
    }

    Some(match classification {
        ToolCallClassification::Observation => {
            "Use the existing observed result or request materially different evidence.".to_string()
        }
        ToolCallClassification::Orchestration => {
            "Use the existing orchestration result or request materially different delegated work."
                .to_string()
        }
        ToolCallClassification::Mutation => {
            "Do not repeat the same side effect; inspect the result or explain the blocker."
                .to_string()
        }
    })
}

fn duplicate_mutation_signature_reason(call: &ToolCall) -> String {
    format!(
        "same-turn duplicate mutation call for '{}' was rejected before execution",
        call.name
    )
}

fn duplicate_mutation_family_reason(family: &MutationFamilyKey) -> String {
    format!(
        "same-turn duplicate mutation variant for {} was rejected before execution",
        family.label()
    )
}

fn completed_mutation_family_reason(family: &MutationFamilyKey) -> String {
    format!("{} already completed earlier in this turn", family.label())
}

fn deterministic_mutation_failure_reason(
    family: &MutationFamilyKey,
    failure_class: FailureClass,
) -> String {
    format!(
        "{} already failed with {} earlier in this turn",
        family.label(),
        mutation_failure_label(failure_class)
    )
}

fn deterministic_mutation_retry_limit_reason(
    family: &MutationFamilyKey,
    failure_class: FailureClass,
) -> String {
    format!(
        "{} already retried after {} without converging",
        family.label(),
        mutation_failure_label(failure_class)
    )
}

fn blocked_mutation_guidance_for_call(
    call: &ToolCall,
    failure_class: Option<FailureClass>,
) -> String {
    mutation_family_for_call(call)
        .map(|family| blocked_mutation_guidance(&family, failure_class))
        .unwrap_or_else(|| {
            "Execute one deterministic mutation variant, review the result, then either stop or explain the blocker instead of trying more near-equivalent mutations in the same turn.".to_string()
        })
}

fn blocked_mutation_guidance(
    family: &MutationFamilyKey,
    failure_class: Option<FailureClass>,
) -> String {
    match failure_class {
        Some(class) if class.is_permanent() || matches!(class, FailureClass::Unknown) => format!(
            "Stop exploring more variants of {} in this turn. Surface the blocker or wait for materially new information before retrying.",
            family.label()
        ),
        Some(_) => format!(
            "Retry {} at most once after a transient failure. If it still does not converge, stop and report the blocker instead of trying more variants.",
            family.label()
        ),
        None => format!(
            "Execute one variant of {} at a time. Review the result before proposing another near-equivalent mutation.",
            family.label()
        ),
    }
}

fn mutation_failure_label(failure_class: FailureClass) -> &'static str {
    match failure_class {
        FailureClass::Timeout => "a timeout",
        FailureClass::Transient | FailureClass::TransientTransport | FailureClass::RateLimited => {
            "a transient failure"
        }
        FailureClass::Unknown => "an unknown failure",
        _ => "a permanent failure",
    }
}

fn blocked_run_command_guidance(
    tool_name: &str,
    failure_class: Option<FailureClass>,
) -> Option<String> {
    if tool_name != "run_command" {
        return None;
    }

    Some(match failure_class {
        Some(class) if class.is_permanent() => "Stop retrying this command; inspect the repo/files directly or use a different installed tool/command.".to_string(),
        _ => "Stop repeating the same command; try a different command or proceed with the current findings.".to_string(),
    })
}

fn hash_tool_arguments(arguments: &serde_json::Value) -> u64 {
    use std::hash::{Hash, Hasher};

    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    let canonical = super::tool_execution::canonicalized_tool_arguments(arguments);
    canonical.hash(&mut hasher);
    hasher.finish()
}

fn hash_string(text: &str) -> u64 {
    use std::hash::{Hash, Hasher};

    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}

fn mutation_family_for_call(call: &ToolCall) -> Option<MutationFamilyKey> {
    match call.name.as_str() {
        "open_browser_application" => call
            .arguments
            .get("browser")
            .and_then(serde_json::Value::as_str)
            .and_then(normalize_browser_name)
            .map(MutationFamilyKey::BrowserOpenApplication),
        "open_browser_url" => call
            .arguments
            .get("url")
            .and_then(serde_json::Value::as_str)
            .and_then(normalize_http_url_token)
            .map(MutationFamilyKey::BrowserOpenUrl),
        "run_command" => mutation_family_from_run_command(&call.arguments),
        _ => None,
    }
}

fn mutation_family_from_run_command(arguments: &serde_json::Value) -> Option<MutationFamilyKey> {
    let tokens = run_command_tokens(arguments)?;
    let first = tokens.first()?.to_ascii_lowercase();

    if first == "open" {
        if let Some(browser) = run_command_open_application_family(&tokens) {
            return Some(browser);
        }
        return tokens
            .iter()
            .skip(1)
            .find_map(|token| normalize_http_url_token(token))
            .map(MutationFamilyKey::BrowserOpenUrl);
    }

    if first == "xdg-open" {
        return tokens
            .get(1)
            .and_then(|token| normalize_http_url_token(token))
            .map(MutationFamilyKey::BrowserOpenUrl);
    }

    if first == "start" {
        if let Some(url) = tokens
            .iter()
            .skip(1)
            .find_map(|token| normalize_http_url_token(token))
        {
            return Some(MutationFamilyKey::BrowserOpenUrl(url));
        }
    }

    if let Some(browser) = normalize_browser_name(&first) {
        if let Some(url) = tokens
            .iter()
            .skip(1)
            .find_map(|token| normalize_http_url_token(token))
        {
            return Some(MutationFamilyKey::BrowserOpenUrl(url));
        }
        return Some(MutationFamilyKey::BrowserOpenApplication(browser));
    }

    None
}

fn run_command_open_application_family(tokens: &[String]) -> Option<MutationFamilyKey> {
    let launcher_index = tokens.iter().position(|token| token == "-a")?;
    let browser = tokens.get(launcher_index + 1)?;
    normalize_browser_name(browser).map(MutationFamilyKey::BrowserOpenApplication)
}

fn run_command_tokens(arguments: &serde_json::Value) -> Option<Vec<String>> {
    if let Some(argv) = arguments.get("argv").and_then(serde_json::Value::as_array) {
        let tokens = argv
            .iter()
            .map(serde_json::Value::as_str)
            .collect::<Option<Vec<_>>>()?;
        return Some(tokens.into_iter().map(str::to_string).collect());
    }

    let command = arguments
        .get("command")
        .and_then(serde_json::Value::as_str)?;
    let shell = arguments
        .get("shell")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    if shell && command_contains_shell_metacharacters(command) {
        return None;
    }
    tokenize_non_shell_command(command).ok()
}

fn command_contains_shell_metacharacters(command: &str) -> bool {
    command
        .chars()
        .any(|ch| matches!(ch, '|' | '&' | ';' | '<' | '>' | '$' | '`'))
}

fn normalize_browser_name(name: &str) -> Option<String> {
    super::bounded_local::BrowserApplication::from_candidate(name)
        .map(|browser| browser.argument_value().to_string())
}

#[cfg(test)]
mod tests {
    use super::super::{blocked_tool_message, CycleStream, LlmProvider, LoopEngine};
    use super::*;
    use crate::act::{
        ActionNextStep, ActionTerminal, RunCommandDiagnostics, ToolCallClassification,
        ToolExecutionDiagnostics, ToolExecutor, ToolExecutorError,
    };
    use crate::budget::{BudgetConfig, BudgetState, BudgetTracker};
    use crate::cancellation::CancellationToken;
    use crate::context_manager::ContextCompactor;
    use crate::decide::Decision;
    use async_trait::async_trait;
    use fx_core::error::LlmError as CoreLlmError;
    use fx_llm::{CompletionRequest, CompletionResponse, ContentBlock, Message, ProviderError};
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};

    fn success_result(call: &ToolCall, output: impl Into<String>) -> ToolResult {
        ToolResult::success(call.id.clone(), call.name.clone(), output)
    }

    fn failure_result(
        call: &ToolCall,
        output: impl Into<String>,
        class: FailureClass,
    ) -> ToolResult {
        ToolResult::failure(call.id.clone(), call.name.clone(), output, class)
    }

    #[derive(Debug)]
    struct AlwaysSucceedExecutor;

    #[async_trait]
    impl ToolExecutor for AlwaysSucceedExecutor {
        async fn execute_tools(
            &self,
            calls: &[ToolCall],
            _cancel: Option<&CancellationToken>,
        ) -> Result<Vec<ToolResult>, ToolExecutorError> {
            Ok(calls
                .iter()
                .map(|call| success_result(call, format!("ok: {}", call.name)))
                .collect())
        }

        fn tool_definitions(&self) -> Vec<fx_llm::ToolDefinition> {
            Vec::new()
        }

        fn clear_cache(&self) {}
    }

    #[derive(Debug)]
    struct AlwaysFailExecutor;

    #[async_trait]
    impl ToolExecutor for AlwaysFailExecutor {
        async fn execute_tools(
            &self,
            calls: &[ToolCall],
            _cancel: Option<&CancellationToken>,
        ) -> Result<Vec<ToolResult>, ToolExecutorError> {
            Ok(calls
                .iter()
                .map(|call| {
                    failure_result(call, format!("err: {}", call.name), FailureClass::Unknown)
                })
                .collect())
        }

        fn tool_definitions(&self) -> Vec<fx_llm::ToolDefinition> {
            Vec::new()
        }

        fn clear_cache(&self) {}
    }

    #[derive(Debug)]
    struct ClassifiedFailExecutor {
        class: FailureClass,
        message: &'static str,
        diagnostics: Mutex<Option<ToolExecutionDiagnostics>>,
    }

    #[async_trait]
    impl ToolExecutor for ClassifiedFailExecutor {
        async fn execute_tools(
            &self,
            calls: &[ToolCall],
            _cancel: Option<&CancellationToken>,
        ) -> Result<Vec<ToolResult>, ToolExecutorError> {
            Ok(calls
                .iter()
                .map(|call| failure_result(call, self.message, self.class))
                .collect())
        }

        fn tool_definitions(&self) -> Vec<fx_llm::ToolDefinition> {
            Vec::new()
        }

        fn take_execution_diagnostics(&self, _call_id: &str) -> Option<ToolExecutionDiagnostics> {
            self.diagnostics.lock().expect("diagnostics lock").take()
        }

        fn clear_cache(&self) {}
    }

    #[derive(Debug, Clone, Copy)]
    struct SequencedOutcome {
        success: bool,
        message: &'static str,
        failure_class: FailureClass,
    }

    impl SequencedOutcome {
        fn success(message: &'static str) -> Self {
            Self {
                success: true,
                message,
                failure_class: FailureClass::Unknown,
            }
        }

        fn failure(message: &'static str, failure_class: FailureClass) -> Self {
            Self {
                success: false,
                message,
                failure_class,
            }
        }
    }

    #[derive(Debug)]
    struct SequencedExecutor {
        outcomes: Mutex<VecDeque<SequencedOutcome>>,
    }

    impl SequencedExecutor {
        fn new(outcomes: Vec<SequencedOutcome>) -> Self {
            Self {
                outcomes: Mutex::new(VecDeque::from(outcomes)),
            }
        }
    }

    #[async_trait]
    impl ToolExecutor for SequencedExecutor {
        async fn execute_tools(
            &self,
            calls: &[ToolCall],
            _cancel: Option<&CancellationToken>,
        ) -> Result<Vec<ToolResult>, ToolExecutorError> {
            let mut outcomes = self.outcomes.lock().expect("outcomes lock");
            Ok(calls
                .iter()
                .map(|call| {
                    let outcome = outcomes
                        .pop_front()
                        .expect("missing sequenced outcome for tool call");
                    if outcome.success {
                        success_result(call, outcome.message)
                    } else {
                        failure_result(call, outcome.message, outcome.failure_class)
                    }
                })
                .collect())
        }

        fn tool_definitions(&self) -> Vec<fx_llm::ToolDefinition> {
            Vec::new()
        }

        fn clear_cache(&self) {}
    }

    #[derive(Debug)]
    struct RecordingMutationExecutor {
        outcomes: Mutex<VecDeque<SequencedOutcome>>,
        executed_calls: Mutex<Vec<ToolCall>>,
    }

    impl RecordingMutationExecutor {
        fn new(outcomes: Vec<SequencedOutcome>) -> Self {
            Self {
                outcomes: Mutex::new(VecDeque::from(outcomes)),
                executed_calls: Mutex::new(Vec::new()),
            }
        }

        fn executed_calls(&self) -> Vec<ToolCall> {
            self.executed_calls
                .lock()
                .expect("executed calls lock")
                .clone()
        }
    }

    #[async_trait]
    impl ToolExecutor for RecordingMutationExecutor {
        async fn execute_tools(
            &self,
            calls: &[ToolCall],
            _cancel: Option<&CancellationToken>,
        ) -> Result<Vec<ToolResult>, ToolExecutorError> {
            self.executed_calls
                .lock()
                .expect("executed calls lock")
                .extend(calls.iter().cloned());
            let mut outcomes = self.outcomes.lock().expect("outcomes lock");
            Ok(calls
                .iter()
                .map(|call| {
                    let outcome = outcomes
                        .pop_front()
                        .unwrap_or_else(|| SequencedOutcome::success("ok"));
                    if outcome.success {
                        success_result(call, outcome.message)
                    } else {
                        failure_result(call, outcome.message, outcome.failure_class)
                    }
                })
                .collect())
        }

        fn classify_call(&self, call: &ToolCall) -> ToolCallClassification {
            match call.name.as_str() {
                "open_browser_application" | "open_browser_url" => ToolCallClassification::Mutation,
                "run_command" if mutation_family_from_run_command(&call.arguments).is_some() => {
                    ToolCallClassification::Mutation
                }
                _ => ToolCallClassification::Observation,
            }
        }

        fn tool_definitions(&self) -> Vec<fx_llm::ToolDefinition> {
            Vec::new()
        }

        fn clear_cache(&self) {}
    }

    #[derive(Debug)]
    struct RecordingMockLlm {
        responses: Mutex<VecDeque<CompletionResponse>>,
        requests: Mutex<Vec<CompletionRequest>>,
    }

    impl RecordingMockLlm {
        fn new(responses: Vec<CompletionResponse>) -> Self {
            Self {
                responses: Mutex::new(VecDeque::from(responses)),
                requests: Mutex::new(Vec::new()),
            }
        }

        fn request_count(&self) -> usize {
            self.requests.lock().expect("requests lock").len()
        }
    }

    #[async_trait]
    impl LlmProvider for RecordingMockLlm {
        async fn generate(&self, _: &str, _: u32) -> Result<String, CoreLlmError> {
            Ok("summary".to_string())
        }

        async fn generate_streaming(
            &self,
            _: &str,
            _: u32,
            callback: Box<dyn Fn(String) + Send + 'static>,
        ) -> Result<String, CoreLlmError> {
            callback("summary".to_string());
            Ok("summary".to_string())
        }

        fn model_name(&self) -> &str {
            "retry-mock"
        }

        async fn complete(
            &self,
            request: CompletionRequest,
        ) -> Result<CompletionResponse, ProviderError> {
            self.requests.lock().expect("requests lock").push(request);
            self.responses
                .lock()
                .expect("responses lock")
                .pop_front()
                .ok_or_else(|| ProviderError::Provider("no response".to_string()))
        }
    }

    fn make_call(id: &str, name: &str) -> ToolCall {
        make_call_with_args(id, name, serde_json::json!({}))
    }

    fn make_call_with_args(id: &str, name: &str, arguments: serde_json::Value) -> ToolCall {
        ToolCall {
            id: id.to_string(),
            name: name.to_string(),
            arguments,
        }
    }

    fn open_url_run_command(id: &str, url: &str) -> ToolCall {
        make_call_with_args(
            id,
            "run_command",
            serde_json::json!({
                "command": format!("open {url}"),
                "shell": false,
            }),
        )
    }

    fn run_command_argv_call(id: &str, argv: &[&str]) -> ToolCall {
        make_call_with_args(
            id,
            "run_command",
            serde_json::json!({
                "argv": argv,
            }),
        )
    }

    fn open_browser_url_call(id: &str, url: &str) -> ToolCall {
        make_call_with_args(id, "open_browser_url", serde_json::json!({ "url": url }))
    }

    fn read_file_call(id: &str, path: &str) -> ToolCall {
        make_call_with_args(id, "read_file", serde_json::json!({ "path": path }))
    }

    fn text_response(text: &str) -> CompletionResponse {
        CompletionResponse {
            content: vec![ContentBlock::Text {
                text: text.to_string(),
            }],
            tool_calls: Vec::new(),
            usage: None,
            stop_reason: Some("end_turn".to_string()),
        }
    }

    fn retry_config(max_tool_retries: u8) -> BudgetConfig {
        let max_consecutive_failures = u16::from(max_tool_retries).saturating_add(1);
        BudgetConfig {
            max_consecutive_failures,
            max_tool_retries,
            ..BudgetConfig::default()
        }
    }

    fn retry_engine_with_executor(
        config: BudgetConfig,
        executor: Arc<dyn ToolExecutor>,
    ) -> LoopEngine {
        LoopEngine::builder()
            .budget(BudgetTracker::new(config, 0, 0))
            .context(ContextCompactor::new(2048, 256))
            .max_iterations(5)
            .tool_executor(executor)
            .synthesis_instruction("Summarize".to_string())
            .build()
            .expect("build")
    }

    fn retry_engine(max_tool_retries: u8) -> LoopEngine {
        retry_engine_with_executor(
            retry_config(max_tool_retries),
            Arc::new(AlwaysSucceedExecutor),
        )
    }

    fn failure_engine(max_tool_retries: u8) -> LoopEngine {
        retry_engine_with_executor(retry_config(max_tool_retries), Arc::new(AlwaysFailExecutor))
    }

    fn block_message(tool_name: &str, failures: u16) -> String {
        blocked_tool_message(
            tool_name,
            &same_call_failure_reason(failures),
            blocked_run_command_guidance(tool_name, None).as_deref(),
        )
    }

    fn permanent_block_message(tool_name: &str) -> String {
        blocked_tool_message(
            tool_name,
            &permanent_failure_reason(),
            blocked_run_command_guidance(tool_name, Some(FailureClass::Permanent)).as_deref(),
        )
    }

    fn block_signature(engine: &mut LoopEngine, call: &ToolCall) {
        let failures = engine
            .budget
            .config()
            .retry_policy()
            .max_consecutive_failures;
        seed_failures(engine, call, failures);
    }

    fn seed_failures(engine: &mut LoopEngine, call: &ToolCall, failures: u16) {
        for _ in 0..failures {
            engine.tool_retry_tracker.record_result(call, false);
        }
    }

    fn is_signature_tracked(engine: &LoopEngine, call: &ToolCall) -> bool {
        engine
            .tool_retry_tracker
            .signature_failures
            .contains_key(&ToolCallKey::from_call(call))
    }

    #[tokio::test]
    async fn successful_calls_keep_failure_counts_at_zero() {
        let mut engine = retry_engine(2);

        for id in 1..=3 {
            let call = read_file_call(&id.to_string(), &format!("file-{id}.md"));
            let results = engine
                .execute_tool_calls(std::slice::from_ref(&call))
                .await
                .expect("execute");
            assert!(results[0].success, "call {id} should succeed");
            assert_eq!(engine.tool_retry_tracker.consecutive_failures_for(&call), 0);
        }

        assert_eq!(engine.tool_retry_tracker.cycle_total_failures, 0);
    }

    #[tokio::test]
    async fn consecutive_failures_block_specific_signature() {
        let mut engine = failure_engine(2);

        for id in 1..=3 {
            let call = make_call(&id.to_string(), "read_file");
            let results = engine.execute_tool_calls(&[call]).await.expect("execute");
            assert!(
                !results[0].success,
                "call {id} should fail but not be blocked"
            );
            assert!(!results[0].output.contains("blocked"));
        }

        let call = make_call("4", "read_file");
        let results = engine
            .execute_tool_calls(std::slice::from_ref(&call))
            .await
            .expect("execute blocked call");
        assert!(!results[0].success);
        assert_eq!(results[0].output, block_message("read_file", 3));
        assert_eq!(engine.tool_retry_tracker.consecutive_failures_for(&call), 3);
        assert_eq!(engine.tool_retry_tracker.cycle_total_failures, 3);
    }

    #[tokio::test]
    async fn blocked_result_contains_tool_name_and_failure_reason() {
        let mut engine = retry_engine(2);
        let call = make_call("blocked", "network_fetch");
        block_signature(&mut engine, &call);

        let results = engine
            .execute_tool_calls(&[call])
            .await
            .expect("execute blocked call");
        let reason = same_call_failure_reason(3);
        assert!(!results[0].success);
        assert!(results[0].output.contains("network_fetch"));
        assert!(results[0].output.contains(&reason));
    }

    #[tokio::test]
    async fn blocked_tool_emits_blocked_signal() {
        let mut engine = retry_engine(2);
        let call = make_call("4", "read_file");
        block_signature(&mut engine, &call);

        engine
            .execute_tool_calls(&[call])
            .await
            .expect("execute blocked call");

        let signals = engine.signals.drain_all();
        let blocked_signals: Vec<_> = signals
            .iter()
            .filter(|signal| signal.kind == crate::signals::SignalKind::Blocked)
            .collect();
        let reason = same_call_failure_reason(3);

        assert_eq!(blocked_signals.len(), 1);
        assert_eq!(
            blocked_signals[0].metadata["tool"],
            serde_json::json!("read_file")
        );
        assert_eq!(
            blocked_signals[0].metadata["reason"],
            serde_json::json!(reason)
        );
        assert_eq!(
            blocked_signals[0].metadata["signature_failures"],
            serde_json::json!(3)
        );
        assert_eq!(
            blocked_signals[0].metadata["cycle_total_failures"],
            serde_json::json!(3)
        );
        assert_eq!(blocked_signals[0].metadata["failure_class"], "unknown");
    }

    #[tokio::test]
    async fn blocked_repeated_run_command_includes_guidance_and_failure_class_metadata() {
        let mut engine = failure_engine(0);
        let first = make_call_with_args(
            "1",
            "run_command",
            serde_json::json!({"command":"false","shell":false}),
        );
        let second = make_call_with_args(
            "2",
            "run_command",
            serde_json::json!({"shell":false,"command":"false"}),
        );

        let first_results = engine
            .execute_tool_calls(std::slice::from_ref(&first))
            .await
            .expect("execute first run_command failure");
        assert_eq!(
            first_results[0].failure_classification(),
            Some(FailureClass::Unknown)
        );

        let second_results = engine
            .execute_tool_calls(std::slice::from_ref(&second))
            .await
            .expect("execute blocked run_command retry");

        assert_eq!(second_results[0].output, block_message("run_command", 1));

        let blocked = engine
            .signals
            .drain_all()
            .into_iter()
            .find(|signal| signal.kind == crate::signals::SignalKind::Blocked)
            .expect("blocked signal");

        assert_eq!(blocked.metadata["failure_class"], "unknown");
        assert_eq!(
            blocked.metadata["guidance"],
            serde_json::json!(
                "Stop repeating the same command; try a different command or proceed with the current findings."
            )
        );
    }

    #[tokio::test]
    async fn permanent_failure_blocks_next_attempt_without_spending_retry_budget() {
        let mut engine = retry_engine_with_executor(
            retry_config(3),
            Arc::new(ClassifiedFailExecutor {
                class: FailureClass::Permanent,
                message: "binary not found",
                diagnostics: Mutex::new(None),
            }),
        );
        let first = make_call("1", "run_command");
        let first_results = engine
            .execute_tool_calls(std::slice::from_ref(&first))
            .await
            .expect("execute first failure");

        assert!(!first_results[0].success);
        assert_eq!(
            first_results[0].failure_classification(),
            Some(FailureClass::Permanent)
        );
        assert_eq!(engine.tool_retry_tracker.cycle_total_failures, 1);

        let second = make_call("2", "run_command");
        let second_results = engine
            .execute_tool_calls(std::slice::from_ref(&second))
            .await
            .expect("execute blocked retry");

        assert_eq!(
            second_results[0].output,
            permanent_block_message("run_command")
        );
        assert_eq!(engine.tool_retry_tracker.cycle_total_failures, 1);
        assert_eq!(
            engine.tool_retry_tracker.consecutive_failures_for(&second),
            1
        );
    }

    #[tokio::test]
    async fn permanent_failure_blocked_signal_includes_failure_class_metadata() {
        let mut engine = retry_engine_with_executor(
            retry_config(3),
            Arc::new(ClassifiedFailExecutor {
                class: FailureClass::Permanent,
                message: "binary not found",
                diagnostics: Mutex::new(None),
            }),
        );
        let first = make_call("1", "run_command");
        let second = make_call("2", "run_command");

        let first_results = engine
            .execute_tool_calls(std::slice::from_ref(&first))
            .await
            .expect("execute first failure");
        engine.emit_action_signals(std::slice::from_ref(&first), &first_results);

        engine
            .execute_tool_calls(std::slice::from_ref(&second))
            .await
            .expect("execute blocked retry");

        let signals = engine.signals.drain_all();
        let friction = signals
            .iter()
            .find(|signal| signal.kind == crate::signals::SignalKind::Friction)
            .expect("friction signal");
        assert_eq!(friction.metadata["failure_class"], "permanent");
        assert_eq!(friction.metadata["permanent"], serde_json::json!(true));

        let blocked = signals
            .iter()
            .find(|signal| signal.kind == crate::signals::SignalKind::Blocked)
            .expect("blocked signal");
        assert_eq!(blocked.cause_id, Some(friction.id));
        assert_eq!(blocked.metadata["failure_class"], "permanent");
        assert_eq!(blocked.metadata["permanent"], serde_json::json!(true));
        assert_eq!(
            blocked.metadata["guidance"],
            serde_json::json!(
                "Stop retrying this command; inspect the repo/files directly or use a different installed tool/command."
            )
        );
        assert_eq!(blocked.metadata["signature_failures"], 1);
    }

    #[tokio::test]
    async fn failed_run_command_friction_signal_includes_structured_diagnostics_metadata() {
        let diagnostics = ToolExecutionDiagnostics::RunCommand(RunCommandDiagnostics {
            exit_code: Some(127),
            stderr_snippet: Some("command not found".to_string()),
            duration_ms: 24,
            shell: false,
            timed_out: false,
            external_actions: Vec::new(),
        });
        let mut engine = retry_engine_with_executor(
            retry_config(3),
            Arc::new(ClassifiedFailExecutor {
                class: FailureClass::Permanent,
                message: "binary not found",
                diagnostics: Mutex::new(Some(diagnostics)),
            }),
        );
        let call = make_call_with_args(
            "1",
            "run_command",
            serde_json::json!({"command":"missingcmd"}),
        );

        let results = engine
            .execute_tool_calls(std::slice::from_ref(&call))
            .await
            .expect("execute failed run_command");
        engine.emit_action_signals(&[call], &results);

        let friction = engine
            .signals
            .drain_all()
            .into_iter()
            .find(|signal| signal.kind == crate::signals::SignalKind::Friction)
            .expect("friction signal");

        assert_eq!(friction.metadata["diagnostics"]["kind"], "run_command");
        assert_eq!(friction.metadata["diagnostics"]["exit_code"], 127);
        assert_eq!(
            friction.metadata["diagnostics"]["stderr_snippet"],
            "command not found"
        );
        assert_eq!(friction.metadata["diagnostics"]["duration_ms"], 24);
        assert_eq!(friction.metadata["diagnostics"]["shell"], false);
        assert_eq!(friction.metadata["diagnostics"]["timed_out"], false);
        assert_eq!(friction.duration_ms, Some(24));
    }

    #[tokio::test]
    async fn actual_retry_attempt_emits_retry_signal_with_prior_failure_metadata() {
        let mut engine = retry_engine_with_executor(
            retry_config(2),
            Arc::new(SequencedExecutor::new(vec![
                SequencedOutcome::failure("temporary network issue", FailureClass::Transient),
                SequencedOutcome::success("ok after retry"),
            ])),
        );
        let arguments = serde_json::json!({"command":"echo hi","shell":false});
        let first = make_call_with_args("1", "run_command", arguments.clone());
        let second = make_call_with_args("2", "run_command", arguments);

        let first_results = engine
            .execute_tool_calls(std::slice::from_ref(&first))
            .await
            .expect("execute first failure");
        engine.emit_action_signals(std::slice::from_ref(&first), &first_results);

        let second_results = engine
            .execute_tool_calls(std::slice::from_ref(&second))
            .await
            .expect("execute retry");
        assert!(second_results[0].success);

        let signals = engine.signals.drain_all();
        let friction = signals
            .iter()
            .find(|signal| signal.kind == crate::signals::SignalKind::Friction)
            .expect("friction signal");
        let retry = signals
            .iter()
            .find(|signal| signal.kind == crate::signals::SignalKind::Retry)
            .expect("retry signal");

        assert_eq!(retry.cause_id, Some(friction.id));
        assert_eq!(retry.metadata["tool"], "run_command");
        assert_eq!(retry.metadata["tool_call_id"], "2");
        assert_eq!(retry.metadata["attempt"], serde_json::json!(2));
        assert_eq!(retry.metadata["prior_failures"], serde_json::json!(1));
        assert_eq!(retry.metadata["failure_class"], "transient");
        assert_eq!(retry.metadata["cycle_total_failures"], serde_json::json!(1));
        assert_eq!(retry.metadata["decision_kind"], "retry_policy");
        assert_eq!(retry.metadata["decision"], "retry_allowed");
        assert_eq!(retry.metadata["retry_cause"], "prior_failure");
    }

    #[tokio::test]
    async fn blocked_stays_blocked_within_cycle() {
        let mut engine = retry_engine(2);
        let call = make_call("seed", "read_file");
        block_signature(&mut engine, &call);

        for id in 4..=6 {
            let blocked_call = make_call(&id.to_string(), "read_file");
            let results = engine
                .execute_tool_calls(&[blocked_call])
                .await
                .expect("execute blocked call");
            assert_eq!(results[0].output, block_message("read_file", 3));
        }
    }

    #[tokio::test]
    async fn mixed_batch_blocked_and_fresh() {
        let mut engine = retry_engine(2);
        let blocked_call = make_call("blocked", "read_file");
        block_signature(&mut engine, &blocked_call);

        let calls = vec![
            blocked_call,
            make_call("fresh-1", "write_file"),
            make_call("fresh-2", "list_dir"),
        ];
        let results = engine.execute_tool_calls(&calls).await.expect("execute");

        assert_eq!(results.len(), 3);
        assert_eq!(results[0].output, block_message("read_file", 3));
        assert!(results[1].success);
        assert!(results[2].success);
    }

    #[tokio::test]
    async fn same_batch_duplicate_browser_open_variants_are_blocked_before_execution() {
        let executor = Arc::new(RecordingMutationExecutor::new(vec![
            SequencedOutcome::success("opened url"),
        ]));
        let mut engine = retry_engine_with_executor(BudgetConfig::default(), executor.clone());
        let url = "https://example.com";
        let first = open_url_run_command("1", url);
        let duplicate = open_browser_url_call("2", url);

        let results = engine
            .execute_tool_calls(&[first.clone(), duplicate.clone()])
            .await
            .expect("execute duplicate browser-open variants");
        let family = MutationFamilyKey::BrowserOpenUrl(url.to_string());
        let reason = duplicate_mutation_family_reason(&family);
        let guidance = blocked_mutation_guidance(&family, None);

        assert!(results[0].success);
        assert_eq!(
            results[1].output,
            blocked_tool_message("open_browser_url", &reason, Some(guidance.as_str()))
        );
        assert_eq!(
            results[1].failure_classification(),
            Some(FailureClass::Permanent)
        );

        let executed = executor.executed_calls();
        assert_eq!(
            executed.len(),
            1,
            "only one browser-open mutation should execute"
        );
        assert_eq!(executed[0].id, first.id);
        let signals = engine.signals.drain_all();
        let blocked = signals
            .iter()
            .find(|signal| {
                signal.kind == crate::signals::SignalKind::Blocked
                    && signal.metadata["tool"] == "open_browser_url"
            })
            .expect("duplicate mutation blocked signal");
        assert_eq!(blocked.metadata["decision_kind"], "tool_call_guardrail");
        assert_eq!(blocked.metadata["decision"], "blocked");
        assert_eq!(blocked.metadata["source"], "mutation_guard");
        assert_eq!(blocked.metadata["block_kind"], "duplicate_variant");
    }

    #[test]
    fn run_command_argv_browser_url_maps_to_browser_url_family() {
        let call = run_command_argv_call("1", &["chrome", "https://example.com"]);

        assert_eq!(
            mutation_family_for_call(&call),
            Some(MutationFamilyKey::BrowserOpenUrl(
                "https://example.com".to_string()
            ))
        );
    }

    #[test]
    fn run_command_open_application_google_chrome_alias_maps_to_application_family() {
        let call = make_call_with_args(
            "1",
            "run_command",
            serde_json::json!({
                "command": r#"open -a "Google Chrome""#,
                "shell": false,
            }),
        );

        assert_eq!(
            mutation_family_for_call(&call),
            Some(MutationFamilyKey::BrowserOpenApplication(
                "chrome".to_string()
            ))
        );
    }

    #[tokio::test]
    async fn same_batch_run_command_browser_application_variants_are_deduped() {
        let executor = Arc::new(RecordingMutationExecutor::new(vec![
            SequencedOutcome::success("opened browser"),
        ]));
        let mut engine = retry_engine_with_executor(BudgetConfig::default(), executor.clone());
        let first = make_call_with_args(
            "1",
            "run_command",
            serde_json::json!({
                "command": r#"open -a "Google Chrome""#,
                "shell": false,
            }),
        );
        let second = run_command_argv_call("2", &["chrome", "--new-window"]);

        let results = engine
            .execute_tool_calls(&[first.clone(), second.clone()])
            .await
            .expect("execute duplicate browser-open application variants");
        let family = MutationFamilyKey::BrowserOpenApplication("chrome".to_string());
        let reason = duplicate_mutation_family_reason(&family);
        let guidance = blocked_mutation_guidance(&family, None);

        assert!(results[0].success);
        assert_eq!(
            results[1].output,
            blocked_tool_message("run_command", &reason, Some(guidance.as_str()))
        );
        assert_eq!(
            results[1].failure_classification(),
            Some(FailureClass::Permanent)
        );
        assert_eq!(
            executor
                .executed_calls()
                .iter()
                .map(|call| call.id.as_str())
                .collect::<Vec<_>>(),
            vec!["1"]
        );
    }

    #[tokio::test]
    async fn observation_calls_are_not_deduped_by_mutation_guards() {
        let executor = Arc::new(RecordingMutationExecutor::new(vec![
            SequencedOutcome::success("read ok"),
            SequencedOutcome::success("read ok"),
        ]));
        let mut engine = retry_engine_with_executor(BudgetConfig::default(), executor.clone());
        let first = read_file_call("1", "README.md");
        let second = read_file_call("2", "README.md");

        let results = engine
            .execute_tool_calls(&[first.clone(), second.clone()])
            .await
            .expect("execute observation calls");

        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|result| result.success));
        assert_eq!(
            executor
                .executed_calls()
                .iter()
                .map(|call| call.id.as_str())
                .collect::<Vec<_>>(),
            vec!["1", "2"]
        );
    }

    #[tokio::test]
    async fn permanent_browser_open_failure_blocks_follow_up_variant_without_retry() {
        let executor = Arc::new(RecordingMutationExecutor::new(vec![
            SequencedOutcome::failure("browser open failed", FailureClass::Permanent),
        ]));
        let mut engine = retry_engine_with_executor(BudgetConfig::default(), executor.clone());
        let url = "https://example.com";
        let first = open_url_run_command("1", url);
        let second = open_browser_url_call("2", url);

        let first_results = engine
            .execute_tool_calls(std::slice::from_ref(&first))
            .await
            .expect("execute first browser-open failure");
        assert_eq!(
            first_results[0].failure_classification(),
            Some(FailureClass::Permanent)
        );

        let second_results = engine
            .execute_tool_calls(std::slice::from_ref(&second))
            .await
            .expect("execute blocked follow-up variant");
        let family = MutationFamilyKey::BrowserOpenUrl(url.to_string());
        let reason = deterministic_mutation_failure_reason(&family, FailureClass::Permanent);
        let guidance = blocked_mutation_guidance(&family, Some(FailureClass::Permanent));

        assert_eq!(
            second_results[0].output,
            blocked_tool_message("open_browser_url", &reason, Some(guidance.as_str()))
        );
        assert_eq!(
            second_results[0].failure_classification(),
            Some(FailureClass::Permanent)
        );
        assert_eq!(
            executor
                .executed_calls()
                .iter()
                .map(|call| call.id.as_str())
                .collect::<Vec<_>>(),
            vec!["1"]
        );
    }

    #[tokio::test]
    async fn timeout_browser_open_failure_allows_one_retry_then_blocks_third_variant() {
        let executor = Arc::new(RecordingMutationExecutor::new(vec![
            SequencedOutcome::failure("timed out", FailureClass::Timeout),
            SequencedOutcome::failure("timed out again", FailureClass::Timeout),
        ]));
        let mut engine = retry_engine_with_executor(BudgetConfig::default(), executor.clone());
        let url = "https://example.com";
        let first = open_url_run_command("1", url);
        let second = open_browser_url_call("2", url);
        let third = open_url_run_command("3", url);

        let first_results = engine
            .execute_tool_calls(std::slice::from_ref(&first))
            .await
            .expect("execute first timeout");
        assert_eq!(
            first_results[0].failure_classification(),
            Some(FailureClass::Timeout)
        );

        let second_results = engine
            .execute_tool_calls(std::slice::from_ref(&second))
            .await
            .expect("execute bounded retry");
        assert_eq!(
            second_results[0].failure_classification(),
            Some(FailureClass::Timeout)
        );

        let third_results = engine
            .execute_tool_calls(std::slice::from_ref(&third))
            .await
            .expect("execute blocked third attempt");
        let family = MutationFamilyKey::BrowserOpenUrl(url.to_string());
        let reason = deterministic_mutation_retry_limit_reason(&family, FailureClass::Timeout);
        let guidance = blocked_mutation_guidance(&family, Some(FailureClass::Timeout));

        assert_eq!(
            third_results[0].output,
            blocked_tool_message("run_command", &reason, Some(guidance.as_str()))
        );
        assert_eq!(
            third_results[0].failure_classification(),
            Some(FailureClass::Timeout)
        );
        assert_eq!(
            executor
                .executed_calls()
                .iter()
                .map(|call| call.id.as_str())
                .collect::<Vec<_>>(),
            vec!["1", "2"]
        );
    }

    #[tokio::test]
    async fn unknown_browser_open_failure_finishes_turn_without_continuation() {
        let executor = Arc::new(RecordingMutationExecutor::new(vec![
            SequencedOutcome::failure("browser open failed", FailureClass::Unknown),
        ]));
        let mut engine = retry_engine_with_executor(BudgetConfig::default(), executor.clone());
        let llm = RecordingMockLlm::new(vec![text_response("unexpected continuation")]);
        let url = "https://example.com";
        let call = open_browser_url_call("1", url);
        let decision = Decision::UseTools(vec![call.clone()]);

        let action = engine
            .act_with_tools(
                &decision,
                std::slice::from_ref(&call),
                &llm,
                &[Message::user(format!("Open {url}"))],
                CycleStream::disabled(),
            )
            .await
            .expect("act_with_tools should stop on unknown deterministic failure");
        let family = MutationFamilyKey::BrowserOpenUrl(url.to_string());
        let expected_reason = deterministic_mutation_failure_reason(&family, FailureClass::Unknown);

        match action.next_step {
            ActionNextStep::Finish(ActionTerminal::Incomplete { reason, .. }) => {
                assert_eq!(reason, expected_reason);
            }
            other => panic!("expected incomplete terminal action, got {other:?}"),
        }
        assert_eq!(
            llm.request_count(),
            0,
            "loop should not request continuation"
        );
        assert_eq!(
            executor
                .executed_calls()
                .iter()
                .map(|executed| executed.id.as_str())
                .collect::<Vec<_>>(),
            vec!["1"]
        );

        let blocked = engine
            .signals
            .drain_all()
            .into_iter()
            .find(|signal| signal.kind == crate::signals::SignalKind::Blocked)
            .expect("blocked signal");
        assert_eq!(blocked.metadata["family"], family.label());
        assert_eq!(blocked.metadata["failure_class"], "unknown");
    }

    #[tokio::test]
    async fn prepare_cycle_allows_previously_blocked_signature() {
        let mut engine = retry_engine(2);
        let call = make_call("blocked", "read_file");
        block_signature(&mut engine, &call);

        let blocked = engine
            .execute_tool_calls(std::slice::from_ref(&call))
            .await
            .expect("execute blocked call");
        assert_eq!(blocked[0].output, block_message("read_file", 3));

        engine.prepare_cycle();

        let results = engine
            .execute_tool_calls(std::slice::from_ref(&call))
            .await
            .expect("execute");
        assert!(results[0].success);
        assert_eq!(engine.tool_retry_tracker.consecutive_failures_for(&call), 0);
        assert_eq!(engine.tool_retry_tracker.cycle_total_failures, 0);
    }

    #[tokio::test]
    async fn prepare_cycle_clears_retry_tracker() {
        let mut engine = retry_engine(2);
        let call = make_call("1", "read_file");
        seed_failures(&mut engine, &call, 1);

        assert!(!engine.tool_retry_tracker.signature_failures.is_empty());
        assert_eq!(engine.tool_retry_tracker.cycle_total_failures, 1);

        engine.prepare_cycle();

        assert!(engine.tool_retry_tracker.signature_failures.is_empty());
        assert_eq!(engine.tool_retry_tracker.cycle_total_failures, 0);
    }

    #[test]
    fn success_resets_failure_count() {
        let config = RetryPolicyConfig {
            max_consecutive_failures: 2,
            max_cycle_failures: 10,
            ..RetryPolicyConfig::default()
        };
        let call = make_call("1", "read_file");
        let mut tracker = RetryTracker::default();

        tracker.record_result(&call, false);
        assert_eq!(tracker.consecutive_failures_for(&call), 1);

        tracker.record_result(&call, true);
        assert_eq!(tracker.consecutive_failures_for(&call), 0);

        tracker.record_result(&call, false);
        assert_eq!(tracker.consecutive_failures_for(&call), 1);
        assert_eq!(tracker.cycle_total_failures, 2);
        assert!(matches!(
            tracker.should_allow(&call, &config),
            RetryVerdict::Allow
        ));
    }

    #[test]
    fn permanent_failure_blocks_next_identical_attempt() {
        let config = RetryPolicyConfig {
            max_consecutive_failures: 5,
            max_cycle_failures: 10,
            ..RetryPolicyConfig::default()
        };
        let call = make_call("1", "run_command");
        let mut tracker = RetryTracker::default();

        tracker.record_result_with_class(&call, false, Some(FailureClass::Permanent));

        assert_eq!(tracker.consecutive_failures_for(&call), 1);
        assert_eq!(
            tracker.last_failure_class_for(&call),
            Some(FailureClass::Permanent)
        );
        assert!(matches!(
            tracker.should_allow(&call, &config),
            RetryVerdict::Block {
                ref reason,
                failure_class: Some(FailureClass::Permanent),
                ..
            } if reason == &permanent_failure_reason()
        ));
    }

    #[test]
    fn transient_failure_remains_retryable_until_threshold() {
        let config = RetryPolicyConfig {
            max_consecutive_failures: 2,
            max_cycle_failures: 10,
            ..RetryPolicyConfig::default()
        };
        let call = make_call("1", "run_command");
        let mut tracker = RetryTracker::default();

        tracker.record_result_with_class(&call, false, Some(FailureClass::Transient));

        assert_eq!(
            tracker.last_failure_class_for(&call),
            Some(FailureClass::Transient)
        );
        assert!(matches!(
            tracker.should_allow(&call, &config),
            RetryVerdict::Allow
        ));
    }

    #[test]
    fn unknown_failure_remains_retryable_until_threshold() {
        let config = RetryPolicyConfig {
            max_consecutive_failures: 2,
            max_cycle_failures: 10,
            ..RetryPolicyConfig::default()
        };
        let call = make_call("1", "run_command");
        let mut tracker = RetryTracker::default();

        tracker.record_result_with_class(&call, false, Some(FailureClass::Unknown));

        assert_eq!(
            tracker.last_failure_class_for(&call),
            Some(FailureClass::Unknown)
        );
        assert!(matches!(
            tracker.should_allow(&call, &config),
            RetryVerdict::Allow
        ));
    }

    #[test]
    fn different_args_tracked_independently() {
        let config = RetryPolicyConfig {
            max_consecutive_failures: 2,
            max_cycle_failures: 10,
            ..RetryPolicyConfig::default()
        };
        let call_a = make_call_with_args("1", "read_file", serde_json::json!({"path": "a"}));
        let call_b = make_call_with_args("2", "read_file", serde_json::json!({"path": "b"}));
        let mut tracker = RetryTracker::default();

        tracker.record_result(&call_a, false);
        tracker.record_result(&call_a, false);

        assert_eq!(tracker.consecutive_failures_for(&call_a), 2);
        assert_eq!(tracker.consecutive_failures_for(&call_b), 0);
        assert!(matches!(
            tracker.should_allow(&call_a, &config),
            RetryVerdict::Block {
                kind: RetryBlockKind::SameCallFailureLimit,
                ref reason,
                ..
            } if reason == &same_call_failure_reason(2)
        ));
        assert!(matches!(
            tracker.should_allow(&call_b, &config),
            RetryVerdict::Allow
        ));
    }

    #[test]
    fn circuit_breaker_blocks_all_tools() {
        let config = RetryPolicyConfig {
            max_consecutive_failures: 10,
            max_cycle_failures: 2,
            ..RetryPolicyConfig::default()
        };
        let mut tracker = RetryTracker::default();
        let call_a = make_call_with_args("1", "read_file", serde_json::json!({"path": "a"}));
        let call_b = make_call_with_args("2", "read_file", serde_json::json!({"path": "b"}));
        let fresh_call = make_call("3", "write_file");

        tracker.record_result(&call_a, false);
        tracker.record_result(&call_b, false);

        assert_eq!(tracker.cycle_total_failures, 2);
        assert!(matches!(
            tracker.should_allow(&fresh_call, &config),
            RetryVerdict::Block {
                kind: RetryBlockKind::CycleFailureLimit,
                ref reason,
                ..
            } if reason == &cycle_failure_limit_reason()
        ));
    }

    #[test]
    fn partitioned_retry_blocks_preserve_retry_source() {
        let config = RetryPolicyConfig {
            max_consecutive_failures: 2,
            ..RetryPolicyConfig::default()
        };
        let call = make_call("1", "read_file");
        let mut tracker = RetryTracker::default();

        tracker.record_result(&call, false);
        tracker.record_result(&call, false);

        let (_allowed, blocked) =
            partition_by_retry_policy(std::slice::from_ref(&call), &tracker, &config);

        assert_eq!(blocked.len(), 1);
        assert_eq!(
            blocked[0].source,
            BlockedToolSource::Retry(RetryBlockKind::SameCallFailureLimit)
        );
    }

    #[test]
    fn no_progress_blocks_after_threshold() {
        let config = RetryPolicyConfig {
            max_no_progress: 3,
            ..RetryPolicyConfig::default()
        };
        let call = make_call("1", "read_file");
        let mut tracker = RetryTracker::default();

        for _ in 0..3 {
            tracker.record_progress(&call, "same output");
        }

        assert!(matches!(
            tracker.should_allow(&call, &config),
            RetryVerdict::Block {
                kind: RetryBlockKind::NoProgress,
                ref reason,
                ..
            } if reason.contains("no progress detected")
        ));
    }

    #[test]
    fn no_progress_resets_on_different_output() {
        let config = RetryPolicyConfig {
            max_no_progress: 3,
            ..RetryPolicyConfig::default()
        };
        let call = make_call("1", "read_file");
        let mut tracker = RetryTracker::default();

        tracker.record_progress(&call, "output A");
        tracker.record_progress(&call, "output A");
        tracker.record_progress(&call, "output B");

        assert!(matches!(
            tracker.should_allow(&call, &config),
            RetryVerdict::Allow
        ));
    }

    #[test]
    fn no_progress_independent_per_signature() {
        let config = RetryPolicyConfig {
            max_no_progress: 3,
            ..RetryPolicyConfig::default()
        };
        let call_a = make_call_with_args("1", "read_file", serde_json::json!({"path": "a"}));
        let call_b = make_call_with_args("2", "read_file", serde_json::json!({"path": "b"}));
        let mut tracker = RetryTracker::default();

        for _ in 0..3 {
            tracker.record_progress(&call_a, "same output");
        }

        assert!(matches!(
            tracker.should_allow(&call_a, &config),
            RetryVerdict::Block { .. }
        ));
        assert!(matches!(
            tracker.should_allow(&call_b, &config),
            RetryVerdict::Allow
        ));
    }

    #[test]
    fn no_progress_does_not_affect_failures() {
        let config = RetryPolicyConfig {
            max_consecutive_failures: 5,
            max_no_progress: 3,
            ..RetryPolicyConfig::default()
        };
        let call = make_call("1", "read_file");
        let mut tracker = RetryTracker::default();

        tracker.record_result(&call, false);
        tracker.record_result(&call, false);
        assert_eq!(tracker.consecutive_failures_for(&call), 2);

        tracker.record_progress(&call, "same output");
        tracker.record_progress(&call, "same output");
        assert_eq!(tracker.consecutive_failures_for(&call), 2);

        assert!(matches!(
            tracker.should_allow(&call, &config),
            RetryVerdict::Allow
        ));
    }

    #[test]
    fn successful_observation_blocks_exact_duplicate_before_no_progress_threshold() {
        let config = RetryPolicyConfig {
            max_no_progress: 99,
            ..RetryPolicyConfig::default()
        };
        let mut tracker = RetryTracker::default();
        let first = read_file_call("first", "README.md");
        let duplicate = read_file_call("duplicate", "README.md");

        tracker.record_results(
            std::slice::from_ref(&first),
            &[success_result(&first, "observed README once")],
            &AlwaysSucceedExecutor,
        );

        assert!(matches!(
            tracker.should_allow(&duplicate, &config),
            RetryVerdict::Block {
                kind: RetryBlockKind::DuplicateSuccess,
                ref reason,
                ..
            } if reason.contains("already gathered the same evidence")
        ));
    }

    #[test]
    fn successful_mutation_invalidates_prior_observation_duplicate_block() {
        let config = RetryPolicyConfig {
            max_no_progress: 99,
            ..RetryPolicyConfig::default()
        };
        let mut tracker = RetryTracker::default();
        let executor = RecordingMutationExecutor::new(Vec::new());
        let first_read = read_file_call("read-1", "README.md");
        let duplicate_read = read_file_call("read-2", "README.md");

        tracker.record_results(
            std::slice::from_ref(&first_read),
            &[success_result(
                &first_read,
                "observed README before mutation",
            )],
            &executor,
        );
        assert!(matches!(
            tracker.should_allow(&duplicate_read, &config),
            RetryVerdict::Block {
                kind: RetryBlockKind::DuplicateSuccess,
                ..
            }
        ));

        let mutation = open_browser_url_call("mutation-1", "https://example.com");
        tracker.record_results(
            std::slice::from_ref(&mutation),
            &[success_result(&mutation, "opened browser")],
            &executor,
        );

        assert!(
            matches!(tracker.should_allow(&duplicate_read, &config), RetryVerdict::Allow),
            "a successful mutation changes the observable world, so re-reading the same evidence can be legitimate"
        );
    }

    #[test]
    fn successful_mutation_blocks_exact_duplicate_across_generations() {
        let config = RetryPolicyConfig::default();
        let mut tracker = RetryTracker::default();
        let executor = RecordingMutationExecutor::new(Vec::new());
        let first = open_browser_url_call("mutation-1", "https://example.com");
        let duplicate = open_browser_url_call("mutation-2", "https://example.com");

        tracker.record_results(
            std::slice::from_ref(&first),
            &[success_result(&first, "opened browser")],
            &executor,
        );

        assert!(matches!(
            tracker.should_allow(&duplicate, &config),
            RetryVerdict::Block {
                kind: RetryBlockKind::DuplicateSuccess,
                ref reason,
                ..
            } if reason.contains("already completed the same side-effecting action")
        ));
    }

    #[test]
    fn clear_resets_no_progress() {
        let config = RetryPolicyConfig {
            max_no_progress: 3,
            ..RetryPolicyConfig::default()
        };
        let call = make_call("1", "read_file");
        let mut tracker = RetryTracker::default();

        for _ in 0..3 {
            tracker.record_progress(&call, "same output");
        }
        assert!(matches!(
            tracker.should_allow(&call, &config),
            RetryVerdict::Block { .. }
        ));

        tracker.clear();
        assert!(matches!(
            tracker.should_allow(&call, &config),
            RetryVerdict::Allow
        ));
        assert!(tracker.no_progress.is_empty());
    }

    #[test]
    fn backward_compat_max_tool_retries() {
        let mut value = serde_json::to_value(BudgetConfig::default()).expect("serialize");
        value["max_tool_retries"] = serde_json::json!(0);

        let config: BudgetConfig = serde_json::from_value(value).expect("deserialize");
        assert_eq!(config.max_tool_retries, 0);
        assert_eq!(config.max_consecutive_failures, 1);
        assert_eq!(config.retry_policy().max_consecutive_failures, 1);
    }

    #[tokio::test]
    async fn zero_retries_blocks_after_one_failure() {
        let mut engine = retry_engine(0);
        let call = make_call("1", "read_file");
        seed_failures(&mut engine, &call, 1);

        let results = engine
            .execute_tool_calls(&[call])
            .await
            .expect("execute blocked call");
        assert_eq!(results[0].output, block_message("read_file", 1));
    }

    #[tokio::test]
    async fn max_retries_effectively_unlimited() {
        let config = BudgetConfig {
            max_consecutive_failures: u16::from(u8::MAX).saturating_add(1),
            max_cycle_failures: u16::MAX,
            max_tool_retries: u8::MAX,
            ..BudgetConfig::default()
        };
        let mut engine = retry_engine_with_executor(config, Arc::new(AlwaysFailExecutor));

        for id in 1..=255_u16 {
            let call = make_call(&id.to_string(), "read_file");
            let results = engine.execute_tool_calls(&[call]).await.expect("execute");
            assert!(!results[0].success, "call {id} should not be blocked");
            assert!(!results[0].output.contains("blocked"));
        }

        let call = make_call("255", "read_file");
        assert_eq!(
            engine.tool_retry_tracker.consecutive_failures_for(&call),
            255
        );
        assert_eq!(engine.tool_retry_tracker.cycle_total_failures, 255);
    }

    #[tokio::test]
    async fn deferred_tools_do_not_count_toward_failures() {
        let config = BudgetConfig {
            max_fan_out: 2,
            max_consecutive_failures: 3,
            max_tool_retries: 2,
            ..BudgetConfig::default()
        };
        let mut engine = retry_engine_with_executor(config, Arc::new(AlwaysSucceedExecutor));
        let calls = vec![
            make_call("1", "tool_a"),
            make_call("2", "tool_b"),
            make_call("3", "tool_c"),
            make_call("4", "tool_d"),
        ];

        let (execute, deferred) = engine.apply_fan_out_cap(&calls);
        let results = engine.execute_tool_calls(&execute).await.expect("execute");

        assert_eq!(results.len(), 2);
        assert!(is_signature_tracked(&engine, &calls[0]));
        assert!(is_signature_tracked(&engine, &calls[1]));
        assert!(!is_signature_tracked(&engine, &deferred[0]));
        assert!(!is_signature_tracked(&engine, &deferred[1]));
        assert_eq!(engine.tool_retry_tracker.cycle_total_failures, 0);
    }

    #[tokio::test]
    async fn deferred_tools_start_fresh_when_executed() {
        let config = BudgetConfig {
            max_fan_out: 1,
            max_consecutive_failures: 3,
            max_tool_retries: 2,
            ..BudgetConfig::default()
        };
        let mut engine = retry_engine_with_executor(config, Arc::new(AlwaysSucceedExecutor));
        let tool_a = make_call("1", "tool_a");
        let tool_b = make_call("2", "tool_b");

        let (execute, _) = engine.apply_fan_out_cap(&[tool_a.clone(), tool_b.clone()]);
        engine.execute_tool_calls(&execute).await.expect("execute");
        assert!(is_signature_tracked(&engine, &tool_a));
        assert!(!is_signature_tracked(&engine, &tool_b));

        let results = engine
            .execute_tool_calls(std::slice::from_ref(&tool_b))
            .await
            .expect("execute deferred tool");
        assert!(results[0].success);
        assert!(is_signature_tracked(&engine, &tool_b));
        assert_eq!(
            engine.tool_retry_tracker.consecutive_failures_for(&tool_b),
            0
        );
        assert_eq!(engine.tool_retry_tracker.cycle_total_failures, 0);
    }

    #[tokio::test]
    async fn budget_low_takes_precedence_over_retry_cap() {
        use crate::budget::ActionCost;
        use fx_core::error::LlmError as CoreLlmError;
        use fx_llm::{CompletionRequest, ProviderError};
        use std::collections::VecDeque;
        use std::sync::Mutex;

        #[derive(Debug)]
        struct MockLlm {
            responses: Mutex<VecDeque<CompletionResponse>>,
        }

        impl MockLlm {
            fn new(responses: Vec<CompletionResponse>) -> Self {
                Self {
                    responses: Mutex::new(VecDeque::from(responses)),
                }
            }
        }

        #[async_trait]
        impl LlmProvider for MockLlm {
            async fn generate(&self, _: &str, _: u32) -> Result<String, CoreLlmError> {
                Ok("summary".to_string())
            }

            async fn generate_streaming(
                &self,
                _: &str,
                _: u32,
                callback: Box<dyn Fn(String) + Send + 'static>,
            ) -> Result<String, CoreLlmError> {
                callback("summary".to_string());
                Ok("summary".to_string())
            }

            fn model_name(&self) -> &str {
                "mock-budget-test"
            }

            async fn complete(
                &self,
                _: CompletionRequest,
            ) -> Result<CompletionResponse, ProviderError> {
                self.responses
                    .lock()
                    .expect("lock")
                    .pop_front()
                    .ok_or_else(|| ProviderError::Provider("no response".to_string()))
            }
        }

        let config = BudgetConfig {
            max_cost_cents: 100,
            max_consecutive_failures: 3,
            max_tool_retries: 2,
            ..BudgetConfig::default()
        };
        let mut engine = retry_engine_with_executor(config, Arc::new(AlwaysSucceedExecutor));
        let blocked_call = make_call("blocked", "read_file");
        block_signature(&mut engine, &blocked_call);
        engine.signals.drain_all();

        engine.budget.record(&ActionCost {
            cost_cents: 81,
            ..ActionCost::default()
        });
        assert_eq!(engine.budget.state(), BudgetState::Low);

        let decision = Decision::UseTools(vec![make_call("5", "read_file")]);
        let tool_calls = match &decision {
            Decision::UseTools(calls) => calls.as_slice(),
            _ => unreachable!(),
        };
        let llm = MockLlm::new(Vec::new());
        let context_messages = vec![Message::user("do something")];

        let action = engine
            .act_with_tools(
                &decision,
                tool_calls,
                &llm,
                &context_messages,
                CycleStream::disabled(),
            )
            .await
            .expect("act_with_tools should succeed with budget-low path");

        assert!(action.tool_results.is_empty());
        assert!(
            action.response_text.contains("budget")
                || action.response_text.contains("soft-ceiling")
        );

        let signals = engine.signals.drain_all();
        let blocked_signals: Vec<_> = signals
            .iter()
            .filter(|signal| signal.kind == crate::signals::SignalKind::Blocked)
            .collect();
        assert!(!blocked_signals.is_empty());
        assert_eq!(
            blocked_signals[0].metadata["reason"],
            serde_json::json!("budget_soft_ceiling")
        );
    }

    #[test]
    fn record_results_tracks_duplicate_success_end_to_end() {
        let config = RetryPolicyConfig::default();
        let mut tracker = RetryTracker::default();

        let calls = vec![make_call("c1", "read_file"), make_call("c2", "write_file")];
        let results = vec![
            ToolResult {
                tool_call_id: "c1".to_string(),
                tool_name: "read_file".to_string(),
                success: true,
                output: "same output".to_string(),
                failure_class: None,
            },
            ToolResult {
                tool_call_id: "c2".to_string(),
                tool_name: "write_file".to_string(),
                success: true,
                output: "ok".to_string(),
                failure_class: None,
            },
        ];

        for _ in 0..3 {
            tracker.record_results(&calls, &results, &AlwaysSucceedExecutor);
        }

        assert!(matches!(
            tracker.should_allow(&calls[0], &config),
            RetryVerdict::Block {
                kind: RetryBlockKind::DuplicateSuccess,
                ..
            }
        ));
        assert!(matches!(
            tracker.should_allow(&calls[1], &config),
            RetryVerdict::Block {
                kind: RetryBlockKind::DuplicateSuccess,
                ..
            }
        ));
    }

    #[test]
    fn record_results_failures_do_not_trigger_no_progress() {
        let mut tracker = RetryTracker::default();

        let calls = vec![make_call("c1", "read_file")];
        let failure_results = vec![ToolResult {
            tool_call_id: "c1".to_string(),
            tool_name: "read_file".to_string(),
            success: false,
            output: "error: not found".to_string(),
            failure_class: None,
        }];

        for _ in 0..5 {
            tracker.record_results(&calls, &failure_results, &AlwaysSucceedExecutor);
        }

        assert!(tracker.no_progress.is_empty());
        assert_eq!(tracker.consecutive_failures_for(&calls[0]), 5);
    }

    #[test]
    fn record_results_mixed_success_failure_no_progress() {
        let config = RetryPolicyConfig {
            max_no_progress: 3,
            max_consecutive_failures: 10,
            max_cycle_failures: 20,
        };
        let mut tracker = RetryTracker::default();

        let calls = vec![make_call("c1", "read_file"), make_call("c2", "write_file")];
        let results = vec![
            ToolResult {
                tool_call_id: "c1".to_string(),
                tool_name: "read_file".to_string(),
                success: true,
                output: "same output".to_string(),
                failure_class: None,
            },
            ToolResult {
                tool_call_id: "c2".to_string(),
                tool_name: "write_file".to_string(),
                success: false,
                output: "error: permission denied".to_string(),
                failure_class: None,
            },
        ];

        for _ in 0..3 {
            tracker.record_results(&calls, &results, &AlwaysSucceedExecutor);
        }

        assert!(matches!(
            tracker.should_allow(&calls[0], &config),
            RetryVerdict::Block {
                kind: RetryBlockKind::DuplicateSuccess,
                ..
            }
        ));
        assert!(!tracker
            .no_progress
            .contains_key(&ToolCallKey::from_call(&calls[1])));
        assert_eq!(tracker.consecutive_failures_for(&calls[1]), 3);
    }
}
