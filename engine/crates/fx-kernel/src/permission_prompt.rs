use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};
use tokio::sync::oneshot;

/// TTL for permission prompts before auto-deny.
pub const PROMPT_TTL: Duration = Duration::from_secs(300);

/// A permission prompt awaiting user response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionPrompt {
    pub id: String,
    pub tool: String,
    pub title: String,
    pub reason: String,
    pub request_summary: String,
    pub session_scoped_allow_available: bool,
    pub expires_at: u64,
}

/// User's response to a permission prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionDecision {
    Allow,
    Deny,
    AllowSession,
}

/// Manages pending permission prompts with TTL and session overrides.
pub struct PermissionPromptState {
    pending: std::sync::Mutex<HashMap<String, PendingPrompt>>,
    resolved: std::sync::Mutex<HashMap<String, ResolvedEntry>>,
    session_overrides: std::sync::Mutex<HashSet<String>>,
}

struct PendingPrompt {
    sender: oneshot::Sender<PermissionDecision>,
    created_at: Instant,
    tool: String,
}

struct ResolvedEntry {
    result: ResolveResult,
    resolved_at: Instant,
}

impl PermissionPromptState {
    pub fn new() -> Self {
        Self {
            pending: std::sync::Mutex::new(HashMap::new()),
            resolved: std::sync::Mutex::new(HashMap::new()),
            session_overrides: std::sync::Mutex::new(HashSet::new()),
        }
    }

    /// Register a prompt and return the receiver for the decision.
    /// Returns None if the tool is already session-overridden (auto-allow).
    pub fn register(
        &self,
        id: String,
        tool: String,
    ) -> Result<Option<oneshot::Receiver<PermissionDecision>>, PromptError> {
        let overrides = self
            .session_overrides
            .lock()
            .map_err(|_| PromptError::Internal)?;
        if overrides.contains(&tool) {
            return Ok(None);
        }
        drop(overrides);

        let (sender, receiver) = oneshot::channel();
        let mut pending = self.pending.lock().map_err(|_| PromptError::Internal)?;
        self.clean_expired_locked(&mut pending);
        pending.insert(
            id,
            PendingPrompt {
                sender,
                created_at: Instant::now(),
                tool,
            },
        );
        Ok(Some(receiver))
    }

    /// Resolve a pending prompt with a decision.
    pub fn resolve(
        &self,
        id: &str,
        decision: PermissionDecision,
    ) -> Result<ResolveResult, PromptError> {
        if let Some(cached) = self.check_resolved_cache(id)? {
            return Ok(cached);
        }

        let prompt = match self.take_pending_prompt(id)? {
            Some(prompt) => prompt,
            None => return self.check_resolved_cache(id)?.ok_or(PromptError::NotFound),
        };
        if prompt.created_at.elapsed() > PROMPT_TTL {
            let _ = prompt.sender.send(PermissionDecision::Deny);
            return Err(PromptError::Expired);
        }

        self.apply_session_override(&prompt.tool, decision)?;
        let result = ResolveResult {
            decision,
            tool: prompt.tool.clone(),
            session_override_applied: decision == PermissionDecision::AllowSession,
        };
        let _ = prompt.sender.send(decision);
        self.cache_resolved(id, &result)?;
        Ok(result)
    }

    /// Check if a tool is session-overridden.
    pub fn is_session_allowed(&self, tool: &str) -> bool {
        self.session_overrides
            .lock()
            .map(|overrides| overrides.contains(tool))
            .unwrap_or(false)
    }

    /// Clear all session overrides (call on session end).
    pub fn clear_session_overrides(&self) {
        if let Ok(mut overrides) = self.session_overrides.lock() {
            overrides.clear();
        }
    }

    fn take_pending_prompt(&self, id: &str) -> Result<Option<PendingPrompt>, PromptError> {
        let mut pending = self.pending.lock().map_err(|_| PromptError::Internal)?;
        Ok(pending.remove(id))
    }

    fn apply_session_override(
        &self,
        tool: &str,
        decision: PermissionDecision,
    ) -> Result<(), PromptError> {
        if decision != PermissionDecision::AllowSession {
            return Ok(());
        }

        let mut overrides = self
            .session_overrides
            .lock()
            .map_err(|_| PromptError::Internal)?;
        overrides.insert(tool.to_string());
        Ok(())
    }

    fn check_resolved_cache(&self, id: &str) -> Result<Option<ResolveResult>, PromptError> {
        let mut resolved = self.resolved.lock().map_err(|_| PromptError::Internal)?;
        self.clean_resolved_locked(&mut resolved);
        Ok(resolved.get(id).map(|entry| entry.result.clone()))
    }

    fn cache_resolved(&self, id: &str, result: &ResolveResult) -> Result<(), PromptError> {
        let mut resolved = self.resolved.lock().map_err(|_| PromptError::Internal)?;
        self.clean_resolved_locked(&mut resolved);
        resolved.insert(
            id.to_string(),
            ResolvedEntry {
                result: result.clone(),
                resolved_at: Instant::now(),
            },
        );
        Ok(())
    }

    fn clean_expired_locked(&self, pending: &mut HashMap<String, PendingPrompt>) {
        pending.retain(|_, prompt| prompt.created_at.elapsed() <= PROMPT_TTL);
    }

    fn clean_resolved_locked(&self, resolved: &mut HashMap<String, ResolvedEntry>) {
        resolved.retain(|_, entry| entry.resolved_at.elapsed() <= PROMPT_TTL);
    }
}

impl Default for PermissionPromptState {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ResolveResult {
    pub decision: PermissionDecision,
    pub tool: String,
    pub session_override_applied: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PromptError {
    NotFound,
    Expired,
    Internal,
}

impl std::fmt::Display for PromptError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound => write!(f, "Permission prompt not found"),
            Self::Expired => write!(f, "Permission prompt expired"),
            Self::Internal => write!(f, "Internal error"),
        }
    }
}

impl std::error::Error for PromptError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decision_serializes_as_snake_case() {
        let json = serde_json::to_string(&PermissionDecision::AllowSession).expect("serialize");
        assert_eq!(json, "\"allow_session\"");
    }

    #[test]
    fn decision_deserializes() {
        let decision: PermissionDecision =
            serde_json::from_str("\"allow_session\"").expect("deserialize");
        assert_eq!(decision, PermissionDecision::AllowSession);
    }

    #[test]
    fn prompt_round_trips_through_json() {
        let prompt = PermissionPrompt {
            id: "prompt-1".to_string(),
            tool: "shell".to_string(),
            title: "Allow shell command".to_string(),
            reason: "Needed to inspect the repo".to_string(),
            request_summary: "git status --short --branch".to_string(),
            session_scoped_allow_available: true,
            expires_at: 1_742_000_000,
        };

        let json = serde_json::to_value(&prompt).expect("serialize");
        let round_trip: PermissionPrompt =
            serde_json::from_value(json.clone()).expect("deserialize");
        assert_eq!(round_trip, prompt);
        assert_eq!(json["expires_at"], 1_742_000_000u64);
    }

    #[test]
    fn state_register_and_resolve() {
        let state = PermissionPromptState::new();
        let receiver = state
            .register("prompt-1".to_string(), "shell".to_string())
            .expect("register prompt")
            .expect("pending prompt");

        let result = state
            .resolve("prompt-1", PermissionDecision::Allow)
            .expect("resolve prompt");

        assert_eq!(result.decision, PermissionDecision::Allow);
        assert_eq!(result.tool, "shell");
        assert!(!result.session_override_applied);
        assert_eq!(receiver.blocking_recv(), Ok(PermissionDecision::Allow));
    }

    #[test]
    fn state_resolve_is_idempotent() {
        let state = PermissionPromptState::new();
        let _receiver = state
            .register("p1".into(), "web_search".into())
            .unwrap()
            .unwrap();

        let first = state.resolve("p1", PermissionDecision::Allow).unwrap();
        let second = state.resolve("p1", PermissionDecision::Allow).unwrap();

        assert_eq!(first.decision, second.decision);
        assert_eq!(first.tool, second.tool);
    }

    #[test]
    fn state_register_propagates_lock_errors() {
        let state = std::sync::Arc::new(PermissionPromptState::new());
        let poisoned = std::sync::Arc::clone(&state);
        let _ = std::thread::spawn(move || {
            let _guard = poisoned.pending.lock().expect("lock pending");
            panic!("poison pending mutex");
        })
        .join();

        let error = state
            .register("prompt-1".to_string(), "shell".to_string())
            .expect_err("poisoned pending lock should fail");
        assert_eq!(error, PromptError::Internal);
    }

    #[test]
    fn state_resolve_unknown_returns_not_found() {
        let state = PermissionPromptState::new();
        let error = state
            .resolve("prompt-missing", PermissionDecision::Deny)
            .expect_err("missing prompt should fail");
        assert_eq!(error, PromptError::NotFound);
    }

    #[test]
    fn state_session_override_auto_allows() {
        let state = PermissionPromptState::new();
        let receiver = state
            .register("prompt-1".to_string(), "shell".to_string())
            .expect("register prompt")
            .expect("pending prompt");

        let result = state
            .resolve("prompt-1", PermissionDecision::AllowSession)
            .expect("resolve prompt");

        assert_eq!(result.decision, PermissionDecision::AllowSession);
        assert!(result.session_override_applied);
        assert!(state.is_session_allowed("shell"));
        assert_eq!(
            receiver.blocking_recv(),
            Ok(PermissionDecision::AllowSession)
        );
        assert!(state
            .register("prompt-2".to_string(), "shell".to_string())
            .expect("register prompt")
            .is_none());
    }

    #[test]
    fn state_clear_session_overrides() {
        let state = PermissionPromptState::new();
        let _ = state
            .register("prompt-1".to_string(), "shell".to_string())
            .expect("register prompt");
        state
            .resolve("prompt-1", PermissionDecision::AllowSession)
            .expect("resolve prompt");
        assert!(state.is_session_allowed("shell"));

        state.clear_session_overrides();

        assert!(!state.is_session_allowed("shell"));
        assert!(state
            .register("prompt-2".to_string(), "shell".to_string())
            .expect("register prompt")
            .is_some());
    }

    #[test]
    fn state_resolve_expired_returns_expired_and_sends_deny() {
        let state = PermissionPromptState::new();
        let receiver = state
            .register("prompt-1".to_string(), "shell".to_string())
            .expect("register prompt")
            .expect("pending prompt");
        let mut pending = state.pending.lock().expect("lock pending");
        let prompt = pending.get_mut("prompt-1").expect("stored prompt");
        prompt.created_at = Instant::now() - PROMPT_TTL - Duration::from_secs(1);
        drop(pending);

        let error = state
            .resolve("prompt-1", PermissionDecision::Deny)
            .expect_err("expired prompt should fail");
        assert_eq!(error, PromptError::Expired);
        assert_eq!(receiver.blocking_recv(), Ok(PermissionDecision::Deny));
    }

    #[test]
    fn prompt_error_display() {
        assert_eq!(
            PromptError::NotFound.to_string(),
            "Permission prompt not found"
        );
        assert_eq!(
            PromptError::Expired.to_string(),
            "Permission prompt expired"
        );
        assert_eq!(PromptError::Internal.to_string(), "Internal error");
    }
}
