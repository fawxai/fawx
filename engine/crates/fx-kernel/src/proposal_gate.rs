//! Kernel-level tool enforcement via the proposal gate.
//!
//! `ProposalGateExecutor` wraps any `ToolExecutor` and intercepts read and
//! write operations against compiled kernel invariants. Reads from blind paths
//! are blocked when the `kernel-blind` feature is enabled; writes to immutable
//! paths are blocked; writes to propose-tier paths create proposals instead of
//! executing; writes to allow-tier paths pass through.

use crate::act::{
    ConcurrencyPolicy, ToolCacheStats, ToolCacheability, ToolExecutor, ToolExecutorError,
    ToolResult,
};
use crate::cancellation::CancellationToken;
use async_trait::async_trait;
use fx_core::self_modify::{classify_path, PathTier, SelfModifyConfig};
use fx_llm::{ToolCall, ToolDefinition};
use fx_propose::{build_proposal_content, current_file_hash, Proposal, ProposalWriter};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Tool names that represent write operations subject to gating.
const WRITE_TOOLS: &[&str] = &["write_file", "edit_file", "git_checkpoint"];

/// Tier 3 immutable paths — blocked regardless of configuration.
/// These are compiled kernel invariants that cannot be overridden.
const TIER3_PATHS: &[&str] = &[
    "engine/crates/fx-kernel/",
    "engine/crates/fx-auth/src/crypto/",
    ".github/",
    "fawx-ripcord/",
    "tests/invariant/",
    "prompt-ledger/",
    "snapshots/",
];

/// Kernel-blind paths — blocked for agent read access when enforcement is on.
/// These are compiled invariants and cannot be overridden.
const KERNEL_BLIND_PATHS: &[&str] = &[
    "engine/crates/fx-kernel/",
    "engine/crates/fx-auth/",
    "engine/crates/fx-security/",
    "engine/crates/fx-consensus/",
    "fawx-ripcord/",
    "tests/invariant/",
];

/// An approved proposal that allows writes to specific paths.
#[derive(Debug, Clone)]
pub struct ActiveProposal {
    pub id: String,
    pub allowed_paths: Vec<PathBuf>,
    pub approved_at: u64,
    pub expires_at: Option<u64>,
}

/// Mutable state for the proposal gate.
#[derive(Debug)]
pub struct ProposalGateState {
    active: Option<ActiveProposal>,
    config: SelfModifyConfig,
    working_dir: PathBuf,
    proposals_dir: PathBuf,
}

impl ProposalGateState {
    #[must_use]
    pub fn new(config: SelfModifyConfig, working_dir: PathBuf, proposals_dir: PathBuf) -> Self {
        Self {
            active: None,
            config,
            working_dir,
            proposals_dir,
        }
    }

    /// Set an active approved proposal.
    pub fn set_active_proposal(&mut self, proposal: ActiveProposal) {
        self.active = Some(proposal);
    }

    /// Clear the active proposal.
    pub fn clear_active_proposal(&mut self) {
        self.active = None;
    }
}

/// A `ToolExecutor` wrapper that enforces the self-modification proposal gate.
///
/// Sits between the kernel and the inner executor (typically `CachingExecutor`).
/// All tool calls pass through; write operations are classified and gated.
///
/// # Mutex note
/// Uses `std::sync::Mutex` (not `tokio::sync::Mutex`) because the lock is never
/// held across `.await` points — `classify_calls` is fully synchronous. If future
/// changes require holding the lock across an await, switch to `tokio::sync::Mutex`
/// to avoid deadlocks in the async runtime.
#[derive(Debug)]
pub struct ProposalGateExecutor<T: ToolExecutor> {
    inner: T,
    state: std::sync::Mutex<ProposalGateState>,
}

impl<T: ToolExecutor> ProposalGateExecutor<T> {
    #[must_use]
    pub fn new(inner: T, state: ProposalGateState) -> Self {
        Self {
            inner,
            state: std::sync::Mutex::new(state),
        }
    }
}

/// Outcome of classifying a single tool call through the gate.
enum GateDecision {
    /// Pass through to inner executor.
    PassThrough,
    /// Block with an error result (Tier 3 or Deny).
    Block(ToolResult),
    /// Create a proposal instead of executing.
    Propose(ToolResult),
}

fn is_write_tool(name: &str) -> bool {
    WRITE_TOOLS.contains(&name)
}

pub fn is_tier3_path(relative_path: &str) -> bool {
    let normalized = normalize_relative(relative_path);
    TIER3_PATHS
        .iter()
        .any(|prefix| normalized.starts_with(prefix))
}

pub fn is_kernel_blind_path(relative_path: &str) -> bool {
    let normalized = normalize_relative(relative_path);
    KERNEL_BLIND_PATHS
        .iter()
        .any(|prefix| normalized.starts_with(prefix))
}

fn is_kernel_blind_enforced() -> bool {
    cfg!(feature = "kernel-blind")
}

fn normalize_relative(path: &str) -> String {
    let unified = path.replace('\\', "/");
    // Strip leading ./ prefix
    let stripped = unified.strip_prefix("./").unwrap_or(&unified);
    // Strip leading / (absolute paths treated as relative to working dir)
    let stripped = stripped.strip_prefix('/').unwrap_or(stripped);
    // Collapse ../ segments
    let mut parts: Vec<&str> = Vec::new();
    for segment in stripped.split('/') {
        match segment {
            "" | "." => continue,
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    parts.join("/")
}

fn extract_path_argument(call: &ToolCall) -> Option<String> {
    call.arguments
        .get("path")
        .and_then(serde_json::Value::as_str)
        .map(String::from)
}

fn blocked_result(call: &ToolCall, path: &str, reason: &str) -> ToolResult {
    tracing::debug!(tool = %call.name, path, reason, "proposal gate blocked tool call");
    ToolResult {
        tool_call_id: call.id.clone(),
        tool_name: call.name.clone(),
        success: false,
        output: "This operation is not permitted.".to_string(),
    }
}

fn blind_read_result(call: &ToolCall) -> ToolResult {
    ToolResult {
        tool_call_id: call.id.clone(),
        tool_name: call.name.clone(),
        success: false,
        output: "This file is not available.".to_string(),
    }
}

fn proposal_result(call: &ToolCall, path: &str, proposal_path: &Path) -> ToolResult {
    ToolResult {
        tool_call_id: call.id.clone(),
        tool_name: call.name.clone(),
        success: true,
        output: format!(
            "PROPOSAL CREATED: write to '{path}' requires approval. \
             Proposal saved to: {}",
            proposal_path.display()
        ),
    }
}

fn epoch_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Default risk level for proposals. Future iterations should derive risk
/// from path tier context (e.g., Tier 2 propose paths vs config changes).
const DEFAULT_RISK_LEVEL: &str = "medium";

fn build_proposal(
    call: &ToolCall,
    path: &str,
    working_dir: &Path,
    file_hash: Option<String>,
) -> Result<Proposal, String> {
    Ok(Proposal {
        title: format!("Write to {path}"),
        description: format!("Agent requested {tool} on {path}", tool = call.name),
        target_path: PathBuf::from(path),
        proposed_content: build_proposal_payload(call, path, working_dir)?,
        risk: DEFAULT_RISK_LEVEL.to_string(),
        timestamp: epoch_seconds(),
        file_hash,
    })
}

fn build_proposal_payload(
    call: &ToolCall,
    path: &str,
    working_dir: &Path,
) -> Result<String, String> {
    match call.name.as_str() {
        "edit_file" => build_edit_proposal_payload(call, path, working_dir),
        _ => build_write_proposal_payload(call, path, working_dir),
    }
}

fn build_write_proposal_payload(
    call: &ToolCall,
    path: &str,
    working_dir: &Path,
) -> Result<String, String> {
    let proposed = string_argument(call, "content").unwrap_or_default();
    let original = read_existing_target(working_dir, path)?;
    Ok(build_proposal_content(original.as_deref(), &proposed))
}

fn build_edit_proposal_payload(
    call: &ToolCall,
    path: &str,
    working_dir: &Path,
) -> Result<String, String> {
    let original = read_existing_target(working_dir, path)?.ok_or_else(|| {
        format!("Failed to inspect target file: edit_file target '{path}' does not exist.")
    })?;
    let old_text = required_string_argument(call, "old_text")?;
    let new_text = string_argument(call, "new_text").unwrap_or_default();
    let updated = apply_exact_edit(&original, &old_text, &new_text)?;
    Ok(build_proposal_content(Some(&original), &updated))
}

fn string_argument(call: &ToolCall, key: &str) -> Option<String> {
    call.arguments
        .get(key)
        .and_then(serde_json::Value::as_str)
        .map(ToString::to_string)
}

fn required_string_argument(call: &ToolCall, key: &str) -> Result<String, String> {
    string_argument(call, key)
        .ok_or_else(|| format!("Failed to inspect target file: missing '{key}' argument."))
}

fn read_existing_target(working_dir: &Path, path: &str) -> Result<Option<String>, String> {
    let target_path = resolve_target_path(working_dir, path);
    match fs::read_to_string(target_path) {
        Ok(content) => Ok(Some(content)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("Failed to inspect target file: {error}")),
    }
}

fn resolve_target_path(working_dir: &Path, path: &str) -> PathBuf {
    let target_path = Path::new(path);
    if target_path.is_absolute() {
        return target_path.to_path_buf();
    }
    working_dir.join(target_path)
}

fn apply_exact_edit(content: &str, old_text: &str, new_text: &str) -> Result<String, String> {
    if old_text.is_empty() {
        return Err(
            "Failed to inspect target file: edit_file old_text cannot be empty.".to_string(),
        );
    }
    let matches = content
        .match_indices(old_text)
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [] => Err(
            "Failed to inspect target file: edit_file old_text not found in target file."
                .to_string(),
        ),
        [start] => Ok(replace_exact_match(content, *start, old_text, new_text)),
        _ => Err(
            "Failed to inspect target file: edit_file old_text matched multiple regions."
                .to_string(),
        ),
    }
}

fn replace_exact_match(content: &str, start: usize, old_text: &str, new_text: &str) -> String {
    let end = start + old_text.len();
    let mut updated = String::with_capacity(content.len() - old_text.len() + new_text.len());
    updated.push_str(&content[..start]);
    updated.push_str(new_text);
    updated.push_str(&content[end..]);
    updated
}

fn classify_and_gate(
    call: &ToolCall,
    config: &SelfModifyConfig,
    working_dir: &Path,
    proposals_dir: &Path,
    active: &Option<ActiveProposal>,
) -> GateDecision {
    if let Some(decision) = classify_read_call(call) {
        return decision;
    }
    if let Some(decision) = classify_shell_blind(call) {
        return decision;
    }
    classify_write_call(call, config, working_dir, proposals_dir, active)
}

fn classify_read_call(call: &ToolCall) -> Option<GateDecision> {
    if !is_kernel_blind_enforced() {
        return None;
    }

    let is_read_tool = matches!(
        call.name.as_str(),
        "read_file" | "search_text" | "list_directory"
    );
    if !is_read_tool {
        return None;
    }

    let Some(path) = extract_path_argument(call) else {
        return Some(GateDecision::PassThrough);
    };

    if is_kernel_blind_path(&path) {
        return Some(GateDecision::Block(blind_read_result(call)));
    }

    Some(GateDecision::PassThrough)
}

#[cfg_attr(not(feature = "kernel-blind"), allow(dead_code))]
fn classify_shell_blind(call: &ToolCall) -> Option<GateDecision> {
    if !is_kernel_blind_enforced() {
        return None;
    }
    if !matches!(call.name.as_str(), "shell" | "bash" | "execute_command") {
        return None;
    }
    let command = call
        .arguments
        .get("command")
        .and_then(serde_json::Value::as_str)?;

    if shell_targets_kernel_path(command) {
        return Some(GateDecision::Block(blind_read_result(call)));
    }
    None
}

#[cfg_attr(not(feature = "kernel-blind"), allow(dead_code))]
fn shell_targets_kernel_path(command: &str) -> bool {
    let read_commands = ["cat ", "head ", "tail ", "less ", "more ", "bat "];
    let search_commands = ["grep ", "rg ", "ag ", "find "];
    let git_commands = ["git show ", "git log -p", "git diff ", "git blame "];
    let re_tools = [
        "strings ", "objdump ", "otool ", "nm ", "readelf ", "hexdump ", "xxd ",
    ];

    for cmd_prefix in read_commands
        .iter()
        .chain(search_commands.iter())
        .chain(git_commands.iter())
        .chain(re_tools.iter())
    {
        if command.contains(cmd_prefix) {
            for path in KERNEL_BLIND_PATHS {
                if command.contains(path) {
                    return true;
                }
            }
        }
    }

    if command.contains("/proc/self/exe") || command.contains("/proc/self/maps") {
        return true;
    }

    false
}

fn classify_write_call(
    call: &ToolCall,
    config: &SelfModifyConfig,
    working_dir: &Path,
    proposals_dir: &Path,
    active: &Option<ActiveProposal>,
) -> GateDecision {
    if !is_write_tool(&call.name) {
        return GateDecision::PassThrough;
    }

    let Some(path) = extract_path_argument(call) else {
        return GateDecision::PassThrough;
    };

    // Tier 3 always blocked — compiled kernel invariant that cannot be
    // disabled by config or overridden by active proposals.
    if is_tier3_path(&path) {
        return GateDecision::Block(blocked_result(
            call,
            &path,
            "Tier 3 immutable path (kernel invariant).",
        ));
    }

    // Active proposal covers this path → allow
    if covers_path(active, &path) {
        return GateDecision::PassThrough;
    }

    let tier = classify_path(Path::new(&path), working_dir, config);
    apply_tier(call, &path, tier, working_dir, proposals_dir)
}

fn covers_path(active: &Option<ActiveProposal>, path: &str) -> bool {
    let Some(proposal) = active else {
        return false;
    };
    // Reject expired proposals
    if let Some(expires) = proposal.expires_at {
        if epoch_seconds() > expires {
            return false;
        }
    }
    let normalized = normalize_relative(path);
    proposal
        .allowed_paths
        .iter()
        .any(|p| normalize_relative(&p.to_string_lossy()) == normalized)
}

fn apply_tier(
    call: &ToolCall,
    path: &str,
    tier: PathTier,
    working_dir: &Path,
    proposals_dir: &Path,
) -> GateDecision {
    match tier {
        PathTier::Allow => GateDecision::PassThrough,
        PathTier::Deny => {
            GateDecision::Block(blocked_result(call, path, "Path is in the deny tier."))
        }
        PathTier::Propose => create_proposal_decision(call, path, working_dir, proposals_dir),
    }
}

fn create_proposal_decision(
    call: &ToolCall,
    path: &str,
    working_dir: &Path,
    proposals_dir: &Path,
) -> GateDecision {
    let file_hash = match current_file_hash(working_dir, Path::new(path)) {
        Ok(hash) => hash,
        Err(err) => {
            return GateDecision::Block(blocked_result(
                call,
                path,
                &format!("Failed to inspect target file: {err}"),
            ));
        }
    };

    let proposal = match build_proposal(call, path, working_dir, file_hash) {
        Ok(proposal) => proposal,
        Err(error) => return GateDecision::Block(blocked_result(call, path, &error)),
    };
    let writer = ProposalWriter::new(proposals_dir.to_path_buf());
    match writer.write(&proposal) {
        Ok(proposal_path) => GateDecision::Propose(proposal_result(call, path, &proposal_path)),
        Err(err) => GateDecision::Block(blocked_result(
            call,
            path,
            &format!("Failed to create proposal: {err}"),
        )),
    }
}

#[async_trait]
impl<T: ToolExecutor> ToolExecutor for ProposalGateExecutor<T> {
    async fn execute_tools(
        &self,
        calls: &[ToolCall],
        cancel: Option<&CancellationToken>,
    ) -> Result<Vec<ToolResult>, ToolExecutorError> {
        let (decisions, pass_through) = self.classify_calls(calls);
        self.execute_with_decisions(calls, decisions, &pass_through, cancel)
            .await
    }

    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        self.inner.tool_definitions()
    }

    fn cacheability(&self, tool_name: &str) -> ToolCacheability {
        self.inner.cacheability(tool_name)
    }

    fn clear_cache(&self) {
        self.inner.clear_cache();
    }

    fn cache_stats(&self) -> Option<ToolCacheStats> {
        self.inner.cache_stats()
    }

    fn concurrency_policy(&self) -> ConcurrencyPolicy {
        self.inner.concurrency_policy()
    }
}

impl<T: ToolExecutor> ProposalGateExecutor<T> {
    fn classify_calls(&self, calls: &[ToolCall]) -> (Vec<GateDecision>, Vec<ToolCall>) {
        let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let mut decisions = Vec::with_capacity(calls.len());
        let mut pass_through = Vec::new();

        for call in calls {
            let decision = classify_and_gate(
                call,
                &state.config,
                &state.working_dir,
                &state.proposals_dir,
                &state.active,
            );
            if matches!(decision, GateDecision::PassThrough) {
                pass_through.push(call.clone());
            }
            decisions.push(decision);
        }

        (decisions, pass_through)
    }

    async fn execute_with_decisions(
        &self,
        calls: &[ToolCall],
        decisions: Vec<GateDecision>,
        pass_through: &[ToolCall],
        cancel: Option<&CancellationToken>,
    ) -> Result<Vec<ToolResult>, ToolExecutorError> {
        let inner_results = if pass_through.is_empty() {
            Vec::new()
        } else {
            self.inner.execute_tools(pass_through, cancel).await?
        };

        assemble_results(calls, decisions, inner_results)
    }
}

fn assemble_results(
    calls: &[ToolCall],
    decisions: Vec<GateDecision>,
    mut inner_results: Vec<ToolResult>,
) -> Result<Vec<ToolResult>, ToolExecutorError> {
    // inner_results is ordered matching pass_through calls; drain from front
    inner_results.reverse();
    let mut results = Vec::with_capacity(calls.len());

    for decision in decisions {
        match decision {
            GateDecision::PassThrough => {
                let result = inner_results.pop().ok_or_else(|| ToolExecutorError {
                    message: "proposal gate: missing inner result".to_string(),
                    recoverable: false,
                })?;
                results.push(result);
            }
            GateDecision::Block(result) | GateDecision::Propose(result) => {
                results.push(result);
            }
        }
    }

    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::act::{ToolCacheStats, ToolCacheability, ToolExecutorError, ToolResult};
    use async_trait::async_trait;
    use fx_llm::ToolCall;
    use fx_propose::{extract_proposed_content, sha256_hex};
    use std::fs;
    use std::sync::{Arc, Mutex};

    #[derive(Debug, Clone)]
    struct MockInner {
        calls: Arc<Mutex<Vec<ToolCall>>>,
        definitions: Vec<ToolDefinition>,
    }

    impl MockInner {
        fn new() -> Self {
            Self {
                calls: Arc::new(Mutex::new(Vec::new())),
                definitions: vec![ToolDefinition {
                    name: "write_file".to_string(),
                    description: "write a file".to_string(),
                    parameters: serde_json::json!({"type":"object"}),
                }],
            }
        }

        fn call_count(&self) -> usize {
            self.calls.lock().unwrap().len()
        }
    }

    #[async_trait]
    impl ToolExecutor for MockInner {
        async fn execute_tools(
            &self,
            calls: &[ToolCall],
            _cancel: Option<&CancellationToken>,
        ) -> Result<Vec<ToolResult>, ToolExecutorError> {
            self.calls.lock().unwrap().extend(calls.iter().cloned());
            Ok(calls
                .iter()
                .map(|c| ToolResult {
                    tool_call_id: c.id.clone(),
                    tool_name: c.name.clone(),
                    success: true,
                    output: format!("executed:{}", c.name),
                })
                .collect())
        }

        fn tool_definitions(&self) -> Vec<ToolDefinition> {
            self.definitions.clone()
        }

        fn cacheability(&self, _tool_name: &str) -> ToolCacheability {
            ToolCacheability::NeverCache
        }

        fn clear_cache(&self) {}

        fn cache_stats(&self) -> Option<ToolCacheStats> {
            Some(ToolCacheStats {
                hits: 42,
                misses: 7,
                entries: 10,
                evictions: 1,
            })
        }
    }

    fn enabled_config() -> SelfModifyConfig {
        SelfModifyConfig {
            enabled: true,
            allow_paths: vec!["docs/**".to_string()],
            propose_paths: vec!["config/**".to_string()],
            deny_paths: vec![
                ".git/**".to_string(),
                "*.key".to_string(),
                "*.pem".to_string(),
                "credentials.*".to_string(),
            ],
            ..SelfModifyConfig::default()
        }
    }

    fn make_executor(config: SelfModifyConfig) -> (ProposalGateExecutor<MockInner>, MockInner) {
        let proposals_dir =
            std::env::temp_dir().join(format!("fx-proposal-gate-test-{}", epoch_seconds()));
        make_executor_in(config, PathBuf::from(""), proposals_dir)
    }

    fn make_executor_in(
        config: SelfModifyConfig,
        working_dir: PathBuf,
        proposals_dir: PathBuf,
    ) -> (ProposalGateExecutor<MockInner>, MockInner) {
        let inner = MockInner::new();
        let probe = inner.clone();
        let state = ProposalGateState::new(config, working_dir, proposals_dir);
        (ProposalGateExecutor::new(inner, state), probe)
    }

    fn write_call(id: &str, path: &str, content: &str) -> ToolCall {
        ToolCall {
            id: id.to_string(),
            name: "write_file".to_string(),
            arguments: serde_json::json!({"path": path, "content": content}),
        }
    }

    fn read_call(id: &str, path: &str) -> ToolCall {
        ToolCall {
            id: id.to_string(),
            name: "read_file".to_string(),
            arguments: serde_json::json!({"path": path}),
        }
    }

    fn search_text_call(id: &str, path: &str) -> ToolCall {
        ToolCall {
            id: id.to_string(),
            name: "search_text".to_string(),
            arguments: serde_json::json!({"query": "test", "path": path}),
        }
    }

    fn list_directory_call(id: &str, path: &str) -> ToolCall {
        ToolCall {
            id: id.to_string(),
            name: "list_directory".to_string(),
            arguments: serde_json::json!({"path": path}),
        }
    }

    fn checkpoint_call(id: &str, path: &str) -> ToolCall {
        ToolCall {
            id: id.to_string(),
            name: "git_checkpoint".to_string(),
            arguments: serde_json::json!({"path": path}),
        }
    }

    #[cfg_attr(not(feature = "kernel-blind"), allow(dead_code))]
    fn shell_call(id: &str, command: &str) -> ToolCall {
        ToolCall {
            id: id.to_string(),
            name: "shell".to_string(),
            arguments: serde_json::json!({"command": command}),
        }
    }

    fn edit_call(id: &str, path: &str, old_text: &str, new_text: &str) -> ToolCall {
        ToolCall {
            id: id.to_string(),
            name: "edit_file".to_string(),
            arguments: serde_json::json!({
                "path": path,
                "old_text": old_text,
                "new_text": new_text,
            }),
        }
    }

    #[test]
    fn blocked_result_does_not_contain_path() {
        let call = write_call("t1", "sensitive/path/file.rs", "content");
        let result = blocked_result(&call, "sensitive/path/file.rs", "Tier3 violation");

        assert!(!result.output.contains("sensitive/path"));
        assert!(!result.output.contains("Tier3"));
        assert_eq!(result.output, "This operation is not permitted.");
    }

    #[test]
    fn blind_read_result_does_not_contain_path() {
        let call = read_call("t1", "engine/crates/fx-kernel/src/lib.rs");
        let result = blind_read_result(&call);

        assert!(!result.output.contains("fx-kernel"));
        assert_eq!(result.output, "This file is not available.");
    }

    // Test 1: Tier 3 path always blocked regardless of config
    #[tokio::test]
    async fn tier3_path_always_blocked_regardless_of_config() {
        let mut config = enabled_config();
        config.allow_paths = vec!["**".to_string()];
        let (executor, probe) = make_executor(config);

        let results = executor
            .execute_tools(
                &[write_call(
                    "1",
                    "engine/crates/fx-kernel/src/lib.rs",
                    "data",
                )],
                None,
            )
            .await
            .unwrap();

        assert_operation_not_permitted(&results[0]);
        assert_eq!(probe.call_count(), 0);
    }

    // Test 2: Propose tier creates proposal without executing
    #[tokio::test]
    async fn propose_tier_creates_proposal_without_executing() {
        let (executor, probe) = make_executor(enabled_config());

        let results = executor
            .execute_tools(&[write_call("1", "config/settings.toml", "data")], None)
            .await
            .unwrap();

        assert!(results[0].success);
        assert!(results[0].output.contains("PROPOSAL CREATED"));
        assert_eq!(probe.call_count(), 0);
    }

    #[tokio::test]
    async fn proposal_sidecar_records_target_hash_at_creation() {
        let working_dir =
            std::env::temp_dir().join(format!("fx-proposal-gate-work-{}", epoch_seconds()));
        let proposals_dir =
            std::env::temp_dir().join(format!("fx-proposal-gate-proposals-{}", epoch_seconds()));
        fs::create_dir_all(working_dir.join("config")).unwrap();
        fs::write(working_dir.join("config/settings.toml"), b"before = true\n").unwrap();

        let (executor, _) = make_executor_in(enabled_config(), working_dir.clone(), proposals_dir);
        let results = executor
            .execute_tools(&[write_call("1", "config/settings.toml", "data")], None)
            .await
            .unwrap();

        assert!(results[0].success);
        let sidecar_path = std::fs::read_dir(executor.state.lock().unwrap().proposals_dir.clone())
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .find(|path| path.extension().and_then(|ext| ext.to_str()) == Some("json"))
            .unwrap();
        let sidecar = std::fs::read_to_string(sidecar_path).unwrap();
        let value: serde_json::Value = serde_json::from_str(&sidecar).unwrap();
        let expected = format!("sha256:{}", sha256_hex(b"before = true\n"));

        assert_eq!(
            value["file_hash_at_creation"],
            serde_json::Value::String(expected)
        );
    }

    #[tokio::test]
    async fn edit_file_proposal_captures_updated_content() {
        let working_dir =
            std::env::temp_dir().join(format!("fx-proposal-gate-edit-work-{}", epoch_seconds()));
        let proposals_dir = std::env::temp_dir().join(format!(
            "fx-proposal-gate-edit-proposals-{}",
            epoch_seconds()
        ));
        fs::create_dir_all(working_dir.join("config")).unwrap();
        fs::write(
            working_dir.join("config/settings.toml"),
            b"name = \"before\"\nmode = \"old\"\n",
        )
        .unwrap();

        let (executor, _) = make_executor_in(enabled_config(), working_dir.clone(), proposals_dir);
        let results = executor
            .execute_tools(
                &[edit_call(
                    "1",
                    "config/settings.toml",
                    "mode = \"old\"",
                    "mode = \"new\"",
                )],
                None,
            )
            .await
            .unwrap();

        assert!(results[0].success);
        let sidecar_path = std::fs::read_dir(executor.state.lock().unwrap().proposals_dir.clone())
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .find(|path| path.extension().and_then(|ext| ext.to_str()) == Some("json"))
            .unwrap();
        let sidecar = std::fs::read_to_string(sidecar_path).unwrap();
        let value: serde_json::Value = serde_json::from_str(&sidecar).unwrap();
        let payload = value["proposed_content"].as_str().unwrap();

        assert!(payload.contains("--- original"));
        assert_eq!(
            extract_proposed_content(payload),
            "name = \"before\"\nmode = \"new\"\n"
        );
    }

    // Test 3: Allow tier passes through to inner
    #[tokio::test]
    async fn allow_tier_passes_through_to_inner() {
        let (executor, probe) = make_executor(enabled_config());

        let results = executor
            .execute_tools(&[write_call("1", "docs/readme.md", "data")], None)
            .await
            .unwrap();

        assert!(results[0].success);
        assert!(results[0].output.contains("executed:write_file"));
        assert_eq!(probe.call_count(), 1);
    }

    // Test 4: Deny tier blocked with error
    #[tokio::test]
    async fn deny_tier_blocked_with_error() {
        let (executor, probe) = make_executor(enabled_config());

        let results = executor
            .execute_tools(&[write_call("1", "credentials.json", "data")], None)
            .await
            .unwrap();

        assert_operation_not_permitted(&results[0]);
        assert_eq!(probe.call_count(), 0);
    }

    #[tokio::test]
    async fn always_propose_key_creates_proposal_without_executing() {
        let (executor, probe) = make_executor(enabled_config());

        let results = executor
            .execute_tools(&[write_call("1", "server.key", "data")], None)
            .await
            .unwrap();

        assert!(results[0].success);
        assert!(results[0].output.contains("PROPOSAL CREATED"));
        assert_eq!(probe.call_count(), 0);
    }

    fn assert_tool_passed_through(result: &ToolResult, tool_call_id: &str, tool_name: &str) {
        assert_eq!(result.tool_call_id, tool_call_id);
        assert_eq!(result.tool_name, tool_name);
        assert!(result.success);
        assert_eq!(result.output, format!("executed:{tool_name}"));
    }

    fn assert_read_passed_through(result: &ToolResult, tool_call_id: &str) {
        assert_tool_passed_through(result, tool_call_id, "read_file");
    }

    fn assert_operation_not_permitted(result: &ToolResult) {
        assert!(!result.success);
        assert_eq!(result.output, "This operation is not permitted.");
    }

    async fn execute_single_tool(call: ToolCall) -> (ToolResult, usize) {
        let (executor, probe) = make_executor(enabled_config());
        let results = executor.execute_tools(&[call], None).await.unwrap();

        (results.into_iter().next().unwrap(), probe.call_count())
    }

    async fn execute_single_read(path: &str) -> (ToolResult, usize) {
        execute_single_tool(read_call("1", path)).await
    }

    #[test]
    fn kernel_blind_path_matching_is_available_without_enforcement() {
        assert!(is_kernel_blind_path("engine/crates/fx-kernel/src/lib.rs"));
        assert!(is_kernel_blind_path(
            "./engine/crates/fx-auth/src/crypto/keys.rs"
        ));
        assert!(is_kernel_blind_path(
            "engine\\crates\\fx-security\\src\\audit\\mod.rs"
        ));
        assert!(!is_kernel_blind_path("docs/specs/kernel-blindness.md"));
    }

    #[tokio::test]
    async fn kernel_blind_paths_allow_read_file_on_loadable_layer() {
        let (result, call_count) =
            execute_single_read("engine/crates/fx-loadable/src/lib.rs").await;

        assert_read_passed_through(&result, "1");
        assert_eq!(call_count, 1);
    }

    #[tokio::test]
    async fn kernel_blind_paths_allow_read_file_on_docs() {
        let (result, call_count) = execute_single_read("docs/specs/kernel-blindness.md").await;

        assert_read_passed_through(&result, "1");
        assert_eq!(call_count, 1);
    }

    #[cfg(feature = "kernel-blind")]
    mod kernel_blind_tests {
        use super::*;

        fn assert_blind_read_denied(result: &ToolResult, tool_call_id: &str, tool_name: &str) {
            assert_eq!(result.tool_call_id, tool_call_id);
            assert_eq!(result.tool_name, tool_name);
            assert!(!result.success);
            assert!(result.output.contains("This file is not available."));
        }

        async fn assert_blind_path_denied(call: ToolCall) {
            let tool_name = call.name.clone();
            let (result, call_count) = execute_single_tool(call).await;

            assert_blind_read_denied(&result, "1", &tool_name);
            assert_eq!(call_count, 0);
        }

        #[tokio::test]
        async fn kernel_blind_feature_controls_enforcement() {
            let (result, call_count) =
                execute_single_read("engine/crates/fx-kernel/src/lib.rs").await;

            assert!(is_kernel_blind_enforced());
            assert!(is_kernel_blind_path("engine/crates/fx-kernel/src/lib.rs"));
            assert_blind_read_denied(&result, "1", "read_file");
            assert_eq!(call_count, 0);
        }

        #[tokio::test]
        async fn kernel_blind_paths_block_read_file_on_proposal_gate_source() {
            assert_blind_path_denied(read_call(
                "1",
                "engine/crates/fx-kernel/src/proposal_gate.rs",
            ))
            .await;
        }

        #[tokio::test]
        async fn kernel_blind_paths_block_read_file_on_auth_keys() {
            assert_blind_path_denied(read_call("1", "engine/crates/fx-auth/src/crypto/keys.rs"))
                .await;
        }

        #[tokio::test]
        async fn kernel_blind_paths_block_read_file_on_security_layer() {
            assert_blind_path_denied(read_call("1", "engine/crates/fx-security/src/audit/mod.rs"))
                .await;
        }

        #[tokio::test]
        async fn kernel_blind_paths_block_read_file_on_consensus_layer() {
            assert_blind_path_denied(read_call("1", "engine/crates/fx-consensus/src/lib.rs")).await;
        }

        #[tokio::test]
        async fn kernel_blind_paths_block_read_file_on_ripcord_shell() {
            assert_blind_path_denied(read_call("1", "fawx-ripcord/src/main.rs")).await;
        }

        #[tokio::test]
        async fn kernel_blind_paths_block_read_file_on_invariant_tests() {
            assert_blind_path_denied(read_call("1", "tests/invariant/tier3_test.rs")).await;
        }

        #[tokio::test]
        async fn kernel_blind_paths_block_read_file_with_dot_slash_prefix() {
            assert_blind_path_denied(read_call("1", "./engine/crates/fx-kernel/src/lib.rs")).await;
        }

        #[tokio::test]
        async fn kernel_blind_paths_block_read_file_with_backslash_separators() {
            assert_blind_path_denied(read_call("1", "engine\\crates\\fx-kernel\\src\\lib.rs"))
                .await;
        }

        #[tokio::test]
        async fn kernel_blind_paths_block_read_file_path_traversal() {
            assert_blind_path_denied(read_call("1", "../../engine/crates/fx-kernel/foo.rs")).await;
        }

        #[tokio::test]
        async fn kernel_blind_blocks_search_text_on_kernel_path() {
            assert_blind_path_denied(search_text_call("1", "engine/crates/fx-kernel/src/")).await;
        }

        #[tokio::test]
        async fn kernel_blind_blocks_list_directory_on_kernel_path() {
            assert_blind_path_denied(list_directory_call("1", "engine/crates/fx-kernel/")).await;
        }

        #[tokio::test]
        async fn shell_blocks_cat_kernel_path() {
            assert_blind_path_denied(shell_call("1", "cat engine/crates/fx-kernel/src/lib.rs"))
                .await;
        }

        #[tokio::test]
        async fn shell_blocks_grep_kernel_path() {
            assert_blind_path_denied(shell_call("1", "grep -r pattern engine/crates/fx-kernel/"))
                .await;
        }

        #[tokio::test]
        async fn shell_blocks_git_show_kernel_path() {
            assert_blind_path_denied(shell_call(
                "1",
                "git show HEAD:engine/crates/fx-kernel/src/lib.rs",
            ))
            .await;
        }

        #[tokio::test]
        async fn shell_blocks_strings_on_proc_self_exe() {
            assert_blind_path_denied(shell_call("1", "strings /proc/self/exe")).await;
        }

        #[tokio::test]
        async fn shell_allows_cat_loadable_path() {
            let (result, call_count) =
                execute_single_tool(shell_call("1", "cat engine/crates/fx-loadable/src/lib.rs"))
                    .await;

            assert_tool_passed_through(&result, "1", "shell");
            assert_eq!(call_count, 1);
        }

        #[tokio::test]
        async fn shell_allows_grep_docs() {
            let (result, call_count) =
                execute_single_tool(shell_call("1", "grep -r pattern docs/")).await;

            assert_tool_passed_through(&result, "1", "shell");
            assert_eq!(call_count, 1);
        }

        #[tokio::test]
        async fn kernel_blind_allows_search_text_on_loadable_path() {
            let (result, call_count) =
                execute_single_tool(search_text_call("1", "engine/crates/fx-loadable/src/")).await;

            assert_tool_passed_through(&result, "1", "search_text");
            assert_eq!(call_count, 1);
        }

        #[tokio::test]
        async fn kernel_blind_allows_list_directory_on_docs() {
            let (result, call_count) = execute_single_tool(list_directory_call("1", "docs/")).await;

            assert_tool_passed_through(&result, "1", "list_directory");
            assert_eq!(call_count, 1);
        }
    }

    #[cfg(not(feature = "kernel-blind"))]
    mod kernel_blind_disabled_tests {
        use super::*;

        async fn assert_blind_path_allowed(path: &str) {
            let (result, call_count) = execute_single_read(path).await;

            assert_read_passed_through(&result, "1");
            assert_eq!(call_count, 1);
        }

        #[tokio::test]
        async fn kernel_blind_feature_controls_enforcement() {
            let (result, call_count) =
                execute_single_read("engine/crates/fx-kernel/src/lib.rs").await;

            assert!(!is_kernel_blind_enforced());
            assert!(is_kernel_blind_path("engine/crates/fx-kernel/src/lib.rs"));
            assert_read_passed_through(&result, "1");
            assert_eq!(call_count, 1);
        }

        #[tokio::test]
        async fn kernel_blind_paths_allow_reads_when_feature_is_disabled() {
            assert_blind_path_allowed("engine/crates/fx-kernel/src/proposal_gate.rs").await;
            assert_blind_path_allowed("engine/crates/fx-auth/src/crypto/keys.rs").await;
            assert_blind_path_allowed("../../engine/crates/fx-kernel/foo.rs").await;
        }

        #[tokio::test]
        async fn kernel_blind_paths_allow_backslash_reads_when_feature_is_disabled() {
            assert_blind_path_allowed("engine\\crates\\fx-kernel\\src\\lib.rs").await;
        }
    }

    #[tokio::test]
    async fn allowed_read_tools_and_non_read_tools_still_pass_through() {
        let (executor, probe) = make_executor(enabled_config());

        let read_tools = vec![
            list_directory_call("1", ".github/"),
            search_text_call("2", "src/"),
            ToolCall {
                id: "3".to_string(),
                name: "memory_read".to_string(),
                arguments: serde_json::json!({"key": "notes"}),
            },
            ToolCall {
                id: "4".to_string(),
                name: "memory_list".to_string(),
                arguments: serde_json::json!({}),
            },
            ToolCall {
                id: "5".to_string(),
                name: "current_time".to_string(),
                arguments: serde_json::json!({}),
            },
        ];

        let results = executor.execute_tools(&read_tools, None).await.unwrap();

        assert_eq!(results.len(), 5);
        for result in &results {
            assert!(result.success);
        }
        assert_eq!(probe.call_count(), 5);
    }

    // Test 6: git_checkpoint gated by tier
    #[tokio::test]
    async fn git_checkpoint_gated_by_tier() {
        let (executor, probe) = make_executor(enabled_config());

        let results = executor
            .execute_tools(
                &[checkpoint_call("1", "engine/crates/fx-kernel/src/act.rs")],
                None,
            )
            .await
            .unwrap();

        assert_operation_not_permitted(&results[0]);
        assert_eq!(probe.call_count(), 0);
    }

    // Test 7: Disabled config still allows normal non-Tier-3 writes
    #[tokio::test]
    async fn disabled_config_allows_normal_non_tier3_writes() {
        let config = SelfModifyConfig::default(); // enabled=false
        let (executor, probe) = make_executor(config);

        let results = executor
            .execute_tools(
                &[
                    write_call("1", "docs/readme.md", "data"),
                    write_call("2", "notes/todo.txt", "data"),
                ],
                None,
            )
            .await
            .unwrap();

        assert!(results[0].success);
        assert!(results[1].success);
        assert_eq!(probe.call_count(), 2);
    }

    #[tokio::test]
    async fn disabled_config_proposes_sensitive_writes() {
        let config = SelfModifyConfig::default(); // enabled=false
        let (executor, probe) = make_executor(config);

        let results = executor
            .execute_tools(
                &[
                    write_call("1", "config.toml", "data"),
                    write_call("2", "credentials.db", "data"),
                    write_call("3", "auth.db", "data"),
                    write_call("4", "keys/server.key", "data"),
                    write_call("5", "certs/server.pem", "data"),
                ],
                None,
            )
            .await
            .unwrap();

        for result in &results {
            assert!(result.success);
            assert!(result.output.contains("PROPOSAL CREATED"));
        }
        assert_eq!(probe.call_count(), 0);
    }

    #[tokio::test]
    async fn disabled_config_proposes_absolute_fawx_config_path() {
        let config = SelfModifyConfig::default(); // enabled=false
        let working_dir = std::env::temp_dir().join(format!(
            "fx-proposal-gate-disabled-config-{}",
            epoch_seconds()
        ));
        let proposals_dir = std::env::temp_dir().join(format!(
            "fx-proposal-gate-disabled-config-proposals-{}",
            epoch_seconds()
        ));
        fs::create_dir_all(&working_dir).unwrap();
        let absolute_path = working_dir.join("config.toml");
        let (executor, probe) = make_executor_in(config, working_dir.clone(), proposals_dir);

        let results = executor
            .execute_tools(
                &[write_call(
                    "1",
                    absolute_path.to_string_lossy().as_ref(),
                    "data",
                )],
                None,
            )
            .await
            .unwrap();

        assert!(results[0].success);
        assert!(results[0].output.contains("PROPOSAL CREATED"));
        assert_eq!(probe.call_count(), 0);
    }

    // Test 7b: Tier 3 blocked even when config disabled (regression for bypass bug)
    #[tokio::test]
    async fn tier3_blocked_even_when_config_disabled() {
        let config = SelfModifyConfig::default(); // enabled=false
        let (executor, probe) = make_executor(config);

        let results = executor
            .execute_tools(
                &[write_call(
                    "1",
                    "engine/crates/fx-kernel/src/lib.rs",
                    "data",
                )],
                None,
            )
            .await
            .unwrap();

        assert_operation_not_permitted(&results[0]);
        assert_eq!(probe.call_count(), 0);
    }

    // Test 8: Mixed batch gates individually
    #[tokio::test]
    async fn mixed_batch_gates_individually() {
        let (executor, probe) = make_executor(enabled_config());

        let calls = vec![
            read_call("1", "docs/readme.md"),
            write_call("2", "docs/guide.md", "data"),
            write_call("3", "credentials.json", "data"),
        ];

        let results = executor.execute_tools(&calls, None).await.unwrap();

        assert_eq!(results.len(), 3);
        // read passes
        assert!(results[0].success);
        assert_eq!(results[0].tool_call_id, "1");
        // allow-tier write passes
        assert!(results[1].success);
        assert_eq!(results[1].tool_call_id, "2");
        assert!(results[1].output.contains("executed:write_file"));
        // deny-tier write blocked
        assert_eq!(results[2].tool_call_id, "3");
        assert_operation_not_permitted(&results[2]);
        // Inner only saw 2 calls (read + allow write)
        assert_eq!(probe.call_count(), 2);
    }

    // Test 9: Tool definitions delegated from inner
    #[tokio::test]
    async fn tool_definitions_delegated_from_inner() {
        let (executor, _) = make_executor(enabled_config());
        let defs = executor.tool_definitions();
        assert_eq!(defs.len(), 1);
        assert_eq!(defs[0].name, "write_file");
    }

    // Test 10: Cache operations delegated
    #[tokio::test]
    async fn cache_operations_delegated() {
        let (executor, _) = make_executor(enabled_config());

        assert_eq!(
            executor.cacheability("write_file"),
            ToolCacheability::NeverCache
        );

        let stats = executor.cache_stats().unwrap();
        assert_eq!(stats.hits, 42);
        assert_eq!(stats.misses, 7);

        // clear_cache should not panic
        executor.clear_cache();
    }

    // Test 11: Active proposal allows covered path
    #[tokio::test]
    async fn active_proposal_allows_covered_path() {
        let inner = MockInner::new();
        let probe = inner.clone();
        let tmp =
            std::env::temp_dir().join(format!("fx-proposal-gate-test-active-{}", epoch_seconds()));
        let mut state = ProposalGateState::new(enabled_config(), PathBuf::from(""), tmp);
        state.set_active_proposal(ActiveProposal {
            id: "p-1".to_string(),
            allowed_paths: vec![PathBuf::from("config/settings.toml")],
            approved_at: epoch_seconds(),
            expires_at: None,
        });
        let executor = ProposalGateExecutor::new(inner, state);

        let results = executor
            .execute_tools(&[write_call("1", "config/settings.toml", "data")], None)
            .await
            .unwrap();

        assert!(results[0].success);
        assert!(results[0].output.contains("executed:write_file"));
        assert_eq!(probe.call_count(), 1);
    }

    // Test 12: Active proposal does not cover other paths
    #[tokio::test]
    async fn active_proposal_does_not_cover_other_paths() {
        let inner = MockInner::new();
        let probe = inner.clone();
        let tmp =
            std::env::temp_dir().join(format!("fx-proposal-gate-test-nocover-{}", epoch_seconds()));
        let mut state = ProposalGateState::new(enabled_config(), PathBuf::from(""), tmp);
        state.set_active_proposal(ActiveProposal {
            id: "p-1".to_string(),
            allowed_paths: vec![PathBuf::from("config/a.toml")],
            approved_at: epoch_seconds(),
            expires_at: None,
        });
        let executor = ProposalGateExecutor::new(inner, state);

        let results = executor
            .execute_tools(&[write_call("1", "config/b.toml", "data")], None)
            .await
            .unwrap();

        // config/b.toml is propose-tier, not covered by proposal → proposal created
        assert!(results[0].success);
        assert!(results[0].output.contains("PROPOSAL CREATED"));
        assert_eq!(probe.call_count(), 0);
    }

    // Test 13: Expired proposal does not grant access (regression for expiry bug)
    #[tokio::test]
    async fn expired_proposal_does_not_grant_access() {
        let inner = MockInner::new();
        let probe = inner.clone();
        let tmp =
            std::env::temp_dir().join(format!("fx-proposal-gate-test-expired-{}", epoch_seconds()));
        let mut state = ProposalGateState::new(enabled_config(), PathBuf::from(""), tmp);
        state.set_active_proposal(ActiveProposal {
            id: "p-expired".to_string(),
            allowed_paths: vec![PathBuf::from("config/settings.toml")],
            approved_at: 1000,
            expires_at: Some(1001), // expired in the past
        });
        let executor = ProposalGateExecutor::new(inner, state);

        let results = executor
            .execute_tools(&[write_call("1", "config/settings.toml", "data")], None)
            .await
            .unwrap();

        // Expired proposal → falls through to propose tier → creates proposal
        assert!(results[0].success);
        assert!(results[0].output.contains("PROPOSAL CREATED"));
        assert_eq!(probe.call_count(), 0);
    }

    // Test 14: Tier 3 blocked even with active proposal
    #[tokio::test]
    async fn tier3_blocked_even_with_active_proposal() {
        let inner = MockInner::new();
        let probe = inner.clone();
        let tmp = std::env::temp_dir().join(format!(
            "fx-proposal-gate-test-tier3-proposal-{}",
            epoch_seconds()
        ));
        let mut state = ProposalGateState::new(enabled_config(), PathBuf::from(""), tmp);
        state.set_active_proposal(ActiveProposal {
            id: "p-1".to_string(),
            allowed_paths: vec![PathBuf::from("engine/crates/fx-kernel/src/lib.rs")],
            approved_at: epoch_seconds(),
            expires_at: None,
        });
        let executor = ProposalGateExecutor::new(inner, state);

        let results = executor
            .execute_tools(
                &[write_call(
                    "1",
                    "engine/crates/fx-kernel/src/lib.rs",
                    "data",
                )],
                None,
            )
            .await
            .unwrap();

        assert_operation_not_permitted(&results[0]);
        assert_eq!(probe.call_count(), 0);
    }

    // Test 15: Tier 3 caught via ../ path traversal
    #[tokio::test]
    async fn tier3_caught_via_dotdot_traversal() {
        let (executor, probe) = make_executor(enabled_config());

        let results = executor
            .execute_tools(
                &[write_call(
                    "1",
                    "engine/../engine/crates/fx-kernel/src/lib.rs",
                    "data",
                )],
                None,
            )
            .await
            .unwrap();

        assert_operation_not_permitted(&results[0]);
        assert_eq!(probe.call_count(), 0);
    }

    // Test 16: Tier 3 caught via absolute path
    #[tokio::test]
    async fn tier3_caught_via_absolute_path() {
        let (executor, probe) = make_executor(enabled_config());

        let results = executor
            .execute_tools(
                &[write_call(
                    "1",
                    "/engine/crates/fx-kernel/src/lib.rs",
                    "data",
                )],
                None,
            )
            .await
            .unwrap();

        assert_operation_not_permitted(&results[0]);
        assert_eq!(probe.call_count(), 0);
    }

    #[test]
    fn edit_file_is_treated_as_write_tool() {
        assert!(is_write_tool("edit_file"));
    }

    // Test 17: normalize_relative unit tests
    #[test]
    fn normalize_relative_handles_variants() {
        assert_eq!(normalize_relative("./foo/bar"), "foo/bar");
        assert_eq!(normalize_relative("a/../b/c"), "b/c");
        assert_eq!(normalize_relative("/absolute/path"), "absolute/path");
        assert_eq!(
            normalize_relative("engine/../engine/crates/fx-kernel/src/lib.rs"),
            "engine/crates/fx-kernel/src/lib.rs"
        );
        assert_eq!(normalize_relative("a/./b/../c"), "a/c");
        assert_eq!(normalize_relative("foo\\bar\\baz"), "foo/bar/baz");
    }
}
