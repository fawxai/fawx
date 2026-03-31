//! Action-level permission gate executor.
//!
//! Wraps the tool executor stack and checks permission policies before
//! executing tools. Tools requiring approval pause execution, emit
//! an SSE `permission_prompt` event, and wait for the user's response.

use crate::act::{
    ConcurrencyPolicy, JournalAction, ToolCacheStats, ToolCacheability, ToolCallClassification,
    ToolExecutor, ToolExecutorError, ToolResult,
};
use crate::authority::{
    AuthorityCoordinator, AuthorityDecision, AuthorityVerdict, ToolAuthoritySurface,
};
use crate::cancellation::CancellationToken;
use crate::permission_prompt::{PermissionDecision, PermissionPrompt, PermissionPromptState};
use crate::streaming::{StreamCallback, StreamEvent};
use async_trait::async_trait;
use fx_config::CapabilityMode;
use fx_llm::{ToolCall, ToolDefinition};
use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const PROMPT_TIMEOUT_SECONDS: u64 = 300;
static PROMPT_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Permission policy for the gate executor.
/// Uses tool action category strings so fx-kernel doesn't depend on fx-config.
#[derive(Debug, Clone)]
pub struct PermissionPolicy {
    /// Tool action categories that can execute without asking.
    pub unrestricted: HashSet<String>,
    /// Tool action categories that require user approval.
    pub ask_required: HashSet<String>,
    /// If true, unmapped tool categories default to requiring approval.
    pub default_ask: bool,
    /// Whether restricted actions prompt or are silently denied.
    pub mode: CapabilityMode,
}

impl PermissionPolicy {
    /// Everything allowed — no prompts fire.
    pub fn allow_all() -> Self {
        Self {
            unrestricted: HashSet::new(),
            ask_required: HashSet::new(),
            default_ask: false,
            mode: CapabilityMode::Capability,
        }
    }

    /// Cautious — common write tools require asking.
    #[cfg(test)]
    pub fn cautious() -> Self {
        let unrestricted = to_set(&["read_any", "web_search", "web_fetch", "tool_call"]);
        let ask_required = to_set(&[
            "code_execute",
            "file_write",
            "git",
            "shell",
            "self_modify",
            "credential_change",
            "system_install",
            "network_listen",
            "outbound_message",
            "file_delete",
            "outside_workspace",
            "kernel_modify",
        ]);
        Self {
            unrestricted,
            ask_required,
            default_ask: true,
            mode: CapabilityMode::Prompt,
        }
    }

    pub(crate) fn requires_asking(&self, category: &str) -> bool {
        if self.unrestricted.contains(category) {
            return false;
        }
        if self.ask_required.contains(category) {
            return true;
        }
        self.default_ask
    }
}

#[cfg(test)]
fn to_set(items: &[&str]) -> HashSet<String> {
    items.iter().map(|s| (*s).to_string()).collect()
}

/// Executor wrapper that checks action-level permissions before tool execution.
pub struct PermissionGateExecutor<T: ToolExecutor> {
    inner: T,
    authority: Arc<AuthorityCoordinator>,
    prompt_state: Arc<PermissionPromptState>,
    stream_callback: Arc<std::sync::Mutex<Option<StreamCallback>>>,
}

impl<T: ToolExecutor> std::fmt::Debug for PermissionGateExecutor<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PermissionGateExecutor").finish()
    }
}

impl<T: ToolExecutor> PermissionGateExecutor<T> {
    pub fn new(
        inner: T,
        authority: Arc<AuthorityCoordinator>,
        prompt_state: Arc<PermissionPromptState>,
    ) -> Self {
        Self {
            inner,
            authority,
            prompt_state,
            stream_callback: Arc::new(std::sync::Mutex::new(None)),
        }
    }

    /// Replace the shared callback slot used for SSE stream events.
    pub fn with_stream_callback_slot(
        mut self,
        callback_slot: Arc<std::sync::Mutex<Option<StreamCallback>>>,
    ) -> Self {
        self.stream_callback = callback_slot;
        self
    }

    /// Set the stream callback for emitting permission prompt SSE events.
    pub fn with_stream_callback(self, callback: StreamCallback) -> Self {
        match self.stream_callback.lock() {
            Ok(mut guard) => {
                *guard = Some(callback);
            }
            Err(error) => tracing::warn!("permission gate callback mutex poisoned: {error}"),
        }
        self
    }

    /// Swap the stream callback (used per-cycle when executor is shared).
    pub fn set_stream_callback(&self, callback: Option<StreamCallback>) {
        match self.stream_callback.lock() {
            Ok(mut guard) => {
                *guard = callback;
            }
            Err(error) => tracing::warn!("permission gate callback mutex poisoned: {error}"),
        }
    }

    /// Get a reference to the shared callback slot for external callers.
    pub fn stream_callback_slot(&self) -> Arc<std::sync::Mutex<Option<StreamCallback>>> {
        Arc::clone(&self.stream_callback)
    }
}

#[async_trait]
impl<T: ToolExecutor> ToolExecutor for PermissionGateExecutor<T> {
    async fn execute_tools(
        &self,
        calls: &[ToolCall],
        cancel: Option<&CancellationToken>,
    ) -> Result<Vec<ToolResult>, ToolExecutorError> {
        let (allowed, denied) = self.classify_calls(calls, cancel).await;
        let inner_results = self.execute_allowed(&allowed, cancel).await?;
        Ok(assemble_results(
            calls.len(),
            allowed,
            inner_results,
            denied,
        ))
    }

    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        self.inner.tool_definitions()
    }

    fn cacheability(&self, tool_name: &str) -> ToolCacheability {
        self.inner.cacheability(tool_name)
    }

    fn classify_call(&self, call: &ToolCall) -> ToolCallClassification {
        self.inner.classify_call(call)
    }

    fn action_category(&self, call: &ToolCall) -> &'static str {
        self.inner.action_category(call)
    }

    fn authority_surface(&self, call: &ToolCall) -> ToolAuthoritySurface {
        self.inner.authority_surface(call)
    }

    fn journal_action(&self, call: &ToolCall, result: &ToolResult) -> Option<JournalAction> {
        self.inner.journal_action(call, result)
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

impl<T: ToolExecutor> PermissionGateExecutor<T> {
    async fn classify_calls(
        &self,
        calls: &[ToolCall],
        cancel: Option<&CancellationToken>,
    ) -> (Vec<(usize, ToolCall)>, Vec<(usize, ToolResult)>) {
        let mut allowed = Vec::new();
        let mut denied = Vec::new();

        for (index, call) in calls.iter().enumerate() {
            match self.check_permission(call, cancel).await {
                PermissionCheck::Allowed => allowed.push((index, call.clone())),
                PermissionCheck::Denied(result) => denied.push((index, result)),
            }
        }

        (allowed, denied)
    }

    async fn execute_allowed(
        &self,
        allowed: &[(usize, ToolCall)],
        cancel: Option<&CancellationToken>,
    ) -> Result<Vec<ToolResult>, ToolExecutorError> {
        if allowed.is_empty() {
            return Ok(Vec::new());
        }
        let calls: Vec<ToolCall> = allowed.iter().map(|(_, c)| c.clone()).collect();
        self.inner.execute_tools(&calls, cancel).await
    }

    async fn check_permission(
        &self,
        call: &ToolCall,
        cancel: Option<&CancellationToken>,
    ) -> PermissionCheck {
        let decision = self.authority_decision(call);
        match decision.verdict {
            AuthorityVerdict::Allow | AuthorityVerdict::Propose => {
                self.authority.cache_decision(&call.id, decision, false);
                PermissionCheck::Allowed
            }
            AuthorityVerdict::Deny => {
                PermissionCheck::Denied(capability_denied_result(call, &decision))
            }
            AuthorityVerdict::Prompt => self.ask_permission(call, decision, cancel).await,
        }
    }

    fn authority_decision(&self, call: &ToolCall) -> AuthorityDecision {
        let fallback = self.inner.action_category(call);
        let surface = self.inner.authority_surface(call);
        let request = self.authority.classify_call(call, fallback, surface);
        let session_approved = self
            .prompt_state
            .is_session_allowed(&request.approval_scope());
        self.authority
            .set_active_session_approvals(self.prompt_state.session_override_count());
        self.authority.resolve_request(request, session_approved)
    }

    async fn ask_permission(
        &self,
        call: &ToolCall,
        decision: AuthorityDecision,
        cancel: Option<&CancellationToken>,
    ) -> PermissionCheck {
        let prompt = build_prompt(call, &decision);
        let prompt_id = prompt.id.clone();
        let scope = decision.request.approval_scope();

        let receiver = match self
            .prompt_state
            .register(prompt_id, scope, call.name.clone())
        {
            Ok(Some(rx)) => rx,
            Ok(None) => {
                self.authority.cache_decision(&call.id, decision, true);
                return PermissionCheck::Allowed;
            }
            Err(_) => {
                return PermissionCheck::Denied(denied_result(call, "Permission system error"))
            }
        };

        emit_prompt(&self.stream_callback, prompt);
        let result = await_decision(call, receiver, cancel, &self.authority, decision).await;
        self.authority
            .set_active_session_approvals(self.prompt_state.session_override_count());
        self.authority.publish_runtime_info();
        result
    }
}

enum PermissionCheck {
    Allowed,
    Denied(ToolResult),
}

fn build_prompt(call: &ToolCall, decision: &AuthorityDecision) -> PermissionPrompt {
    PermissionPrompt {
        id: generate_prompt_id(),
        tool: call.name.clone(),
        title: format!("Allow {}", decision.request.capability),
        reason: extract_reason(call),
        request_summary: extract_summary(call),
        session_scoped_allow_available: true,
        expires_at: unix_now() + PROMPT_TIMEOUT_SECONDS,
    }
}

fn emit_prompt(
    callback_slot: &Arc<std::sync::Mutex<Option<StreamCallback>>>,
    prompt: PermissionPrompt,
) {
    match callback_slot.lock() {
        Ok(guard) => {
            if let Some(cb) = guard.as_ref() {
                cb(StreamEvent::PermissionPrompt(prompt));
            }
        }
        Err(error) => tracing::warn!("permission gate callback mutex poisoned: {error}"),
    }
}

async fn await_decision(
    call: &ToolCall,
    receiver: tokio::sync::oneshot::Receiver<PermissionDecision>,
    cancel: Option<&CancellationToken>,
    authority: &Arc<AuthorityCoordinator>,
    decision: AuthorityDecision,
) -> PermissionCheck {
    let timeout = Duration::from_secs(PROMPT_TIMEOUT_SECONDS);
    let result = match cancel {
        Some(token) => {
            tokio::select! {
                biased;
                _ = token.cancelled() => Err("cancelled"),
                result = tokio::time::timeout(timeout, receiver) => match result {
                    Ok(Ok(decision)) => Ok(decision),
                    Ok(Err(_)) => Err("expired"),
                    Err(_) => Err("timed out"),
                }
            }
        }
        None => match tokio::time::timeout(timeout, receiver).await {
            Ok(Ok(decision)) => Ok(decision),
            Ok(Err(_)) => Err("expired"),
            Err(_) => Err("timed out"),
        },
    };

    match result {
        Ok(PermissionDecision::Allow | PermissionDecision::AllowSession) => {
            authority.cache_decision(&call.id, decision, true);
            PermissionCheck::Allowed
        }
        Ok(PermissionDecision::Deny) => {
            PermissionCheck::Denied(denied_result(call, "Permission denied by user"))
        }
        Err("cancelled") => PermissionCheck::Denied(denied_result(call, "Cancelled")),
        Err(reason) => {
            PermissionCheck::Denied(denied_result(call, &format!("Permission prompt {reason}")))
        }
    }
}

fn assemble_results(
    total: usize,
    allowed: Vec<(usize, ToolCall)>,
    inner_results: Vec<ToolResult>,
    denied: Vec<(usize, ToolResult)>,
) -> Vec<ToolResult> {
    let mut indexed: Vec<(usize, ToolResult)> = Vec::with_capacity(total);
    for ((original_idx, _), result) in allowed.into_iter().zip(inner_results) {
        indexed.push((original_idx, result));
    }
    indexed.extend(denied);
    indexed.sort_by_key(|(idx, _)| *idx);
    indexed.into_iter().map(|(_, result)| result).collect()
}

fn capability_denied_result(call: &ToolCall, decision: &AuthorityDecision) -> ToolResult {
    let message = match decision.reason.as_str() {
        "kernel blind invariant" => "DENIED: This file is not available.",
        "sovereign write boundary" => {
            "DENIED: This action requires elevated privileges not available in this session."
        }
        _ => match decision.request.capability.as_str() {
        "network_listen" | "outbound_message" => {
            "DENIED: This action is not available in this session. Request a capability grant or use an alternative approach."
        }
        "credential_change" | "system_install" | "kernel_modify" => {
            "DENIED: This action requires elevated privileges not available in this session."
        }
        "file_delete" | "outside_workspace" => {
            "DENIED: This action is outside the current session's permitted scope."
        }
        _ => "DENIED: This action is not permitted in the current session configuration.",
        },
    };
    ToolResult {
        tool_call_id: call.id.clone(),
        tool_name: call.name.clone(),
        success: false,
        output: message.to_string(),
    }
}

fn denied_result(call: &ToolCall, reason: &str) -> ToolResult {
    ToolResult {
        tool_call_id: call.id.clone(),
        tool_name: call.name.clone(),
        success: false,
        output: format!("PERMISSION DENIED: {reason}"),
    }
}

fn extract_reason(call: &ToolCall) -> String {
    call.arguments
        .get("reason")
        .or_else(|| call.arguments.get("query"))
        .or_else(|| call.arguments.get("command"))
        .or_else(|| call.arguments.get("path"))
        .and_then(|v| v.as_str())
        .map(|s| s.chars().take(200).collect::<String>())
        .unwrap_or_else(|| format!("Tool '{}' requires permission", call.name))
}

fn extract_summary(call: &ToolCall) -> String {
    call.arguments
        .get("command")
        .or_else(|| call.arguments.get("query"))
        .or_else(|| call.arguments.get("path"))
        .or_else(|| call.arguments.get("url"))
        .and_then(|v| v.as_str())
        .map(|s| s.chars().take(500).collect::<String>())
        .unwrap_or_else(|| call.name.clone())
}

fn generate_prompt_id() -> String {
    let count = PROMPT_COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("perm_{nanos:x}_{count}")
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::authority::{AuthorityRequest, ToolAuthoritySurface};
    use crate::proposal_gate::ProposalGateState;
    use fx_core::self_modify::SelfModifyConfig;
    use std::path::PathBuf;

    #[derive(Debug)]
    struct PassthroughExecutor;

    #[async_trait]
    impl ToolExecutor for PassthroughExecutor {
        async fn execute_tools(
            &self,
            calls: &[ToolCall],
            _cancel: Option<&CancellationToken>,
        ) -> Result<Vec<ToolResult>, ToolExecutorError> {
            Ok(calls
                .iter()
                .map(|c| ToolResult {
                    tool_call_id: c.id.clone(),
                    tool_name: c.name.clone(),
                    success: true,
                    output: "executed".to_string(),
                })
                .collect())
        }

        fn tool_definitions(&self) -> Vec<ToolDefinition> {
            vec![]
        }

        fn cacheability(&self, _: &str) -> ToolCacheability {
            ToolCacheability::NeverCache
        }

        fn action_category(&self, call: &ToolCall) -> &'static str {
            match call.name.as_str() {
                "web_search" => "web_search",
                "shell" => "shell",
                "write_file" => "file_write",
                _ => "unknown",
            }
        }

        fn authority_surface(&self, call: &ToolCall) -> ToolAuthoritySurface {
            match call.name.as_str() {
                "web_search" => ToolAuthoritySurface::Network,
                "shell" => ToolAuthoritySurface::Command,
                "write_file" => ToolAuthoritySurface::PathWrite,
                "read_file" => ToolAuthoritySurface::PathRead,
                _ => ToolAuthoritySurface::Other,
            }
        }

        fn clear_cache(&self) {}

        fn cache_stats(&self) -> Option<ToolCacheStats> {
            None
        }

        fn concurrency_policy(&self) -> ConcurrencyPolicy {
            ConcurrencyPolicy::default()
        }
    }

    fn test_call(name: &str) -> ToolCall {
        ToolCall {
            id: format!("call_{name}"),
            name: name.to_string(),
            arguments: serde_json::json!({}),
        }
    }

    fn write_call(path: &str) -> ToolCall {
        ToolCall {
            id: "call_write_file".to_string(),
            name: "write_file".to_string(),
            arguments: serde_json::json!({"path": path, "content": "data"}),
        }
    }

    fn read_call(path: &str) -> ToolCall {
        ToolCall {
            id: "call_read_file".to_string(),
            name: "read_file".to_string(),
            arguments: serde_json::json!({"path": path}),
        }
    }

    fn search_call() -> ToolCall {
        test_call("web_search")
    }

    fn shell_call() -> ToolCall {
        test_call("shell")
    }

    fn unknown_call() -> ToolCall {
        test_call("unknown_tool")
    }

    fn capture_prompt_id() -> (Arc<std::sync::Mutex<Option<String>>>, StreamCallback) {
        let captured_id = Arc::new(std::sync::Mutex::new(None));
        let captured = Arc::clone(&captured_id);
        let callback: StreamCallback = Arc::new(move |event| {
            if let StreamEvent::PermissionPrompt(prompt) = event {
                *captured.lock().expect("capture prompt") = Some(prompt.id.clone());
            }
        });
        (captured_id, callback)
    }

    fn spawn_resolution(
        prompt_state: Arc<PermissionPromptState>,
        captured_id: Arc<std::sync::Mutex<Option<String>>>,
        decision: PermissionDecision,
    ) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            for _ in 0..100 {
                tokio::time::sleep(Duration::from_millis(10)).await;
                let prompt_id = captured_id.lock().expect("read prompt id").clone();
                if let Some(id) = prompt_id {
                    let _ = prompt_state.resolve(&id, decision);
                    break;
                }
            }
        })
    }

    fn cautious_policy(mode: CapabilityMode) -> PermissionPolicy {
        PermissionPolicy {
            mode,
            ..PermissionPolicy::cautious()
        }
    }

    fn test_authority(policy: PermissionPolicy, working_dir: &str) -> Arc<AuthorityCoordinator> {
        Arc::new(AuthorityCoordinator::new(
            policy,
            ProposalGateState::new(
                SelfModifyConfig::default(),
                PathBuf::from(working_dir),
                PathBuf::from("/tmp/fawx-proposals"),
            ),
        ))
    }

    fn test_executor(
        policy: PermissionPolicy,
        prompt_state: Arc<PermissionPromptState>,
    ) -> PermissionGateExecutor<PassthroughExecutor> {
        PermissionGateExecutor::new(
            PassthroughExecutor,
            test_authority(policy, "/Users/joseph"),
            prompt_state,
        )
    }

    #[tokio::test]
    async fn unrestricted_tool_passes_through() {
        let executor = test_executor(
            PermissionPolicy::allow_all(),
            Arc::new(PermissionPromptState::new()),
        );

        let results = executor
            .execute_tools(&[test_call("web_search")], None)
            .await
            .expect("execute");

        assert_eq!(results.len(), 1);
        assert!(results[0].success);
        assert_eq!(results[0].output, "executed");
    }

    #[tokio::test]
    async fn capability_mode_silently_denies_restricted_tool() {
        let (captured_id, callback) = capture_prompt_id();
        let executor = test_executor(
            cautious_policy(CapabilityMode::Capability),
            Arc::new(PermissionPromptState::new()),
        )
        .with_stream_callback(callback);

        let results = executor
            .execute_tools(&[test_call("shell")], None)
            .await
            .expect("execute");

        assert_eq!(results.len(), 1);
        assert!(!results[0].success);
        assert!(results[0].output.contains("DENIED"));
        assert!(captured_id.lock().expect("captured").is_none());
    }

    #[tokio::test]
    async fn capability_mode_allows_unrestricted_tool() {
        let executor = test_executor(
            cautious_policy(CapabilityMode::Capability),
            Arc::new(PermissionPromptState::new()),
        );

        let results = executor
            .execute_tools(&[test_call("web_search")], None)
            .await
            .expect("execute");

        assert_eq!(results.len(), 1);
        assert!(results[0].success);
    }

    #[tokio::test]
    async fn capability_mode_with_default_ask_disabled_allows_unknown_tool() {
        let mut policy = cautious_policy(CapabilityMode::Capability);
        policy.default_ask = false;
        let executor = test_executor(policy, Arc::new(PermissionPromptState::new()));

        let results = executor
            .execute_tools(&[test_call("current_time")], None)
            .await
            .expect("execute");

        assert_eq!(results.len(), 1);
        assert!(results[0].success);
        assert_eq!(results[0].tool_name, "current_time");
    }

    #[tokio::test]
    async fn capability_mode_session_override_still_works() {
        let authority =
            test_authority(cautious_policy(CapabilityMode::Capability), "/Users/joseph");
        let prompt_state = Arc::new(PermissionPromptState::new());
        let request =
            authority.classify_call(&test_call("shell"), "shell", ToolAuthoritySurface::Command);
        let receiver = prompt_state
            .register("setup".into(), request.approval_scope(), "shell".into())
            .expect("register")
            .expect("receiver");
        prompt_state
            .resolve("setup", PermissionDecision::AllowSession)
            .expect("resolve");
        drop(receiver);

        let executor = PermissionGateExecutor::new(PassthroughExecutor, authority, prompt_state);

        let results = executor
            .execute_tools(&[test_call("shell")], None)
            .await
            .expect("execute");

        assert_eq!(results.len(), 1);
        assert!(results[0].success);
    }

    #[tokio::test]
    async fn prompt_mode_still_prompts() {
        let prompt_state = Arc::new(PermissionPromptState::new());
        let (captured_id, callback) = capture_prompt_id();
        let resolver = spawn_resolution(
            Arc::clone(&prompt_state),
            Arc::clone(&captured_id),
            PermissionDecision::Allow,
        );
        let executor = test_executor(cautious_policy(CapabilityMode::Prompt), prompt_state)
            .with_stream_callback(callback);

        let results = tokio::time::timeout(
            Duration::from_secs(1),
            executor.execute_tools(&[test_call("shell")], None),
        )
        .await
        .expect("permission resolution timeout")
        .expect("execute");
        resolver.await.expect("resolver join");

        assert_eq!(results.len(), 1);
        assert!(results[0].success);
        assert!(captured_id.lock().expect("captured").is_some());
    }

    #[tokio::test]
    async fn prompt_deny_returns_denied_result() {
        let prompt_state = Arc::new(PermissionPromptState::new());
        let (captured_id, callback) = capture_prompt_id();
        let resolver = spawn_resolution(
            Arc::clone(&prompt_state),
            Arc::clone(&captured_id),
            PermissionDecision::Deny,
        );
        let executor = test_executor(cautious_policy(CapabilityMode::Prompt), prompt_state)
            .with_stream_callback(callback);

        let results = tokio::time::timeout(
            Duration::from_secs(1),
            executor.execute_tools(&[test_call("shell")], None),
        )
        .await
        .expect("permission resolution timeout")
        .expect("execute");
        resolver.await.expect("resolver join");

        assert_eq!(results.len(), 1);
        assert!(!results[0].success);
        assert!(results[0].output.contains("PERMISSION DENIED"));
    }

    #[tokio::test]
    async fn prompt_cancel_returns_denied_result() {
        let prompt_state = Arc::new(PermissionPromptState::new());
        let (captured_id, callback) = capture_prompt_id();
        let token = CancellationToken::new();
        let cancel_token = token.clone();
        let wait_for_prompt = Arc::clone(&captured_id);
        let canceller = tokio::spawn(async move {
            for _ in 0..100 {
                tokio::time::sleep(Duration::from_millis(10)).await;
                if wait_for_prompt.lock().expect("wait for prompt").is_some() {
                    cancel_token.cancel();
                    break;
                }
            }
        });
        let executor = test_executor(cautious_policy(CapabilityMode::Prompt), prompt_state)
            .with_stream_callback(callback);

        let results = tokio::time::timeout(
            Duration::from_secs(1),
            executor.execute_tools(&[test_call("shell")], Some(&token)),
        )
        .await
        .expect("cancellation timeout")
        .expect("execute");
        canceller.await.expect("canceller join");

        assert_eq!(results.len(), 1);
        assert!(!results[0].success);
        assert!(results[0].output.contains("Cancelled"));
    }

    #[test]
    fn action_category_delegates_to_inner_executor() {
        let executor = test_executor(
            PermissionPolicy::allow_all(),
            Arc::new(PermissionPromptState::new()),
        );

        assert_eq!(executor.action_category(&search_call()), "web_search");
        assert_eq!(executor.action_category(&shell_call()), "shell");
        assert_eq!(
            executor.action_category(&write_call("file.txt")),
            "file_write"
        );
        assert_eq!(executor.action_category(&unknown_call()), "unknown");
    }

    #[test]
    fn authority_request_uses_project_write_capability() {
        let authority = test_authority(PermissionPolicy::allow_all(), "/Users/joseph");
        let request = authority.classify_call(
            &write_call("/Users/joseph/project/file.txt"),
            "file_write",
            ToolAuthoritySurface::PathWrite,
        );

        assert_eq!(request.capability, "file_write");
    }

    #[test]
    fn authority_request_uses_self_modify_capability() {
        let authority = test_authority(PermissionPolicy::allow_all(), "/Users/joseph");
        let request = authority.classify_call(
            &write_call("/Users/joseph/.fawx/skills/demo/SKILL.md"),
            "file_write",
            ToolAuthoritySurface::PathWrite,
        );

        assert_eq!(request.capability, "self_modify");
    }

    #[test]
    fn authority_request_uses_kernel_modify_capability() {
        let authority = test_authority(PermissionPolicy::allow_all(), "/Users/joseph");
        let request = authority.classify_call(
            &write_call("/Users/joseph/fawx/engine/crates/fx-kernel/src/lib.rs"),
            "file_write",
            ToolAuthoritySurface::PathWrite,
        );

        assert_eq!(request.capability, "kernel_modify");
    }

    #[test]
    fn authority_request_uses_outside_workspace_capability_for_write() {
        let authority = test_authority(PermissionPolicy::allow_all(), "/Users/joseph/workspace");
        let request = authority.classify_call(
            &write_call("/etc/hosts"),
            "file_write",
            ToolAuthoritySurface::PathWrite,
        );

        assert_eq!(request.capability, "outside_workspace");
    }

    #[test]
    fn authority_request_uses_outside_workspace_capability_for_read() {
        let authority = test_authority(PermissionPolicy::allow_all(), "/Users/joseph");
        let request = authority.classify_call(
            &read_call("/etc/hosts"),
            "read_any",
            ToolAuthoritySurface::PathRead,
        );

        assert_eq!(request.capability, "outside_workspace");
    }

    #[test]
    fn authority_request_preserves_inner_category_for_workspace_read() {
        let workspace = std::env::temp_dir().join("fx-kernel-authority-workspace");
        let note_path = workspace.join("notes.txt");
        let workspace_str = workspace.to_string_lossy().into_owned();
        let note_path_str = note_path.to_string_lossy().into_owned();
        let authority = test_authority(PermissionPolicy::allow_all(), &workspace_str);
        let request = authority.classify_call(
            &read_call(&note_path_str),
            "read_any",
            ToolAuthoritySurface::PathRead,
        );

        assert_eq!(request.capability, "read_any");
    }

    #[test]
    fn denied_result_contains_reason() {
        let call = test_call("shell");
        let result = denied_result(&call, "User denied");

        assert!(!result.success);
        assert!(result.output.contains("User denied"));
        assert_eq!(result.tool_call_id, "call_shell");
    }

    #[test]
    fn capability_denied_result_contains_category() {
        let call = test_call("delete_file");
        let decision = AuthorityDecision {
            request: AuthorityRequest {
                tool_name: "delete_file".to_string(),
                capability: "file_delete".to_string(),
                effect: crate::authority::AuthorityEffect::Delete,
                target_kind: crate::authority::AuthorityTargetKind::Path,
                domain: crate::authority::AuthorityDomain::Project,
                target_summary: "README.md".to_string(),
                target_identity: "README.md".to_string(),
                paths: vec!["README.md".to_string()],
                command: None,
                invariant: None,
            },
            verdict: AuthorityVerdict::Deny,
            reason: "capability mode denied restricted request".to_string(),
        };
        let result = capability_denied_result(&call, &decision);

        assert!(!result.success);
        assert!(result.output.contains("DENIED"));
        assert!(
            !result.output.contains("file_delete"),
            "should not leak category name"
        );
    }

    #[test]
    fn extract_reason_uses_query_argument() {
        let call = ToolCall {
            id: "c1".into(),
            name: "web_search".into(),
            arguments: serde_json::json!({"query": "rust async patterns"}),
        };

        assert_eq!(extract_reason(&call), "rust async patterns");
    }

    #[test]
    fn extract_summary_uses_command_argument() {
        let call = ToolCall {
            id: "c1".into(),
            name: "shell".into(),
            arguments: serde_json::json!({"command": "ls -la"}),
        };

        assert_eq!(extract_summary(&call), "ls -la");
    }

    #[test]
    fn extract_reason_falls_back_to_tool_name() {
        let call = test_call("shell");
        assert_eq!(extract_reason(&call), "Tool 'shell' requires permission");
    }

    #[test]
    fn assemble_results_preserves_order() {
        let allowed = vec![(0, test_call("a")), (2, test_call("c"))];
        let inner = vec![
            ToolResult {
                tool_call_id: "a".into(),
                tool_name: "a".into(),
                success: true,
                output: "ok".into(),
            },
            ToolResult {
                tool_call_id: "c".into(),
                tool_name: "c".into(),
                success: true,
                output: "ok".into(),
            },
        ];
        let denied = vec![(1, denied_result(&test_call("b"), "denied"))];

        let results = assemble_results(3, allowed, inner, denied);

        assert_eq!(results.len(), 3);
        assert_eq!(results[0].tool_name, "a");
        assert_eq!(results[1].tool_name, "b");
        assert!(!results[1].success);
        assert_eq!(results[2].tool_name, "c");
    }

    #[test]
    fn policy_requires_asking_for_ask_required_category() {
        let policy = PermissionPolicy::cautious();

        assert!(policy.requires_asking("shell"));
        assert!(policy.requires_asking("file_write"));
        assert!(!policy.requires_asking("web_search"));
        assert!(policy.requires_asking("unknown_category"));
    }
}
