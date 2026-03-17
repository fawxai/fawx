use async_trait::async_trait;
use fx_kernel::act::ToolCacheability;
use fx_kernel::cancellation::CancellationToken;
use fx_llm::ToolDefinition;
use fx_loadable::{Skill, SkillError};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::oneshot;

/// Maximum capability requests per session.
const MAX_REQUESTS_PER_SESSION: u32 = 3;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
const REQUEST_CAPABILITY_TOOL: &str = "request_capability";

/// Callback for delivering capability requests to the user.
/// Returns true if granted, false if denied.
pub type CapabilityRequestHandler =
    Arc<dyn Fn(CapabilityRequest) -> oneshot::Receiver<bool> + Send + Sync>;

/// A capability request from the agent.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CapabilityRequest {
    pub capability: String,
    pub reason: String,
    pub request_number: u32,
    pub max_requests: u32,
}

/// Skill that allows the agent to request expanded capabilities.
pub struct CapabilityRequestSkill {
    handler: Option<CapabilityRequestHandler>,
    request_count: AtomicU32,
}

#[derive(Debug, Deserialize)]
struct RequestCapabilityArgs {
    capability: String,
    reason: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct CapabilityResponse {
    granted: bool,
    capability: String,
    note: String,
}

impl std::fmt::Debug for CapabilityRequestSkill {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CapabilityRequestSkill")
            .field("has_handler", &self.handler.is_some())
            .field("request_count", &self.request_count.load(Ordering::SeqCst))
            .finish()
    }
}

impl CapabilityRequestSkill {
    // Follow-up wiring note: register this skill in SkillRegistry during startup.
    // The handler should be supplied by the HTTP/TUI layer that delivers the
    // request to the user and applies any actual policy change outside this tool.
    pub fn new(handler: Option<CapabilityRequestHandler>) -> Self {
        Self {
            handler,
            request_count: AtomicU32::new(0),
        }
    }

    fn handles_tool(&self, tool_name: &str) -> bool {
        tool_name == REQUEST_CAPABILITY_TOOL
    }

    fn parse_args(&self, arguments: &str) -> Result<RequestCapabilityArgs, SkillError> {
        serde_json::from_str(arguments).map_err(|error| format!("invalid arguments: {error}"))
    }

    fn reserve_request_slot(&self) -> Result<u32, SkillError> {
        let count = self.request_count.load(Ordering::SeqCst);
        if count >= MAX_REQUESTS_PER_SESSION {
            return Err(rate_limit_message());
        }
        let previous = self.request_count.fetch_add(1, Ordering::SeqCst);
        if previous >= MAX_REQUESTS_PER_SESSION {
            self.request_count
                .store(MAX_REQUESTS_PER_SESSION, Ordering::SeqCst);
            return Err(rate_limit_message());
        }
        Ok(previous + 1)
    }

    fn build_request(&self, args: RequestCapabilityArgs, request_number: u32) -> CapabilityRequest {
        CapabilityRequest {
            capability: args.capability,
            reason: args.reason,
            request_number,
            max_requests: MAX_REQUESTS_PER_SESSION,
        }
    }

    fn validate_request_fields(&self, capability: &str, reason: &str) -> Result<(), SkillError> {
        if capability.trim().is_empty() {
            return Err("'capability' must not be empty".to_string());
        }
        if reason.trim().is_empty() {
            return Err("'reason' must not be empty".to_string());
        }
        Ok(())
    }

    async fn await_response(
        &self,
        capability: String,
        receiver: oneshot::Receiver<bool>,
        cancel: Option<&CancellationToken>,
    ) -> Result<String, SkillError> {
        let granted = wait_for_decision(receiver, cancel).await;
        let response = build_response(capability, granted);
        serde_json::to_string(&response).map_err(|error| format!("serialization failed: {error}"))
    }

    #[cfg(test)]
    fn request_count(&self) -> u32 {
        self.request_count.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl Skill for CapabilityRequestSkill {
    fn name(&self) -> &str {
        "capability-request"
    }

    fn description(&self) -> &str {
        "Allows the agent to request expanded session capabilities from the user."
    }

    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        vec![request_capability_tool_definition()]
    }

    fn cacheability(&self, tool_name: &str) -> ToolCacheability {
        if self.handles_tool(tool_name) {
            ToolCacheability::SideEffect
        } else {
            ToolCacheability::NeverCache
        }
    }

    async fn execute(
        &self,
        tool_name: &str,
        arguments: &str,
        cancel: Option<&CancellationToken>,
    ) -> Option<Result<String, SkillError>> {
        if !self.handles_tool(tool_name) {
            return None;
        }

        Some(self.execute_request(arguments, cancel).await)
    }
}

impl CapabilityRequestSkill {
    async fn execute_request(
        &self,
        arguments: &str,
        cancel: Option<&CancellationToken>,
    ) -> Result<String, SkillError> {
        let args = self.parse_args(arguments)?;
        self.validate_request_fields(&args.capability, &args.reason)?;
        let request_number = self.reserve_request_slot()?;
        let handler = self
            .handler
            .as_ref()
            .ok_or_else(|| "Capability requests are not available in this session.".to_string())?;
        let request = self.build_request(args, request_number);
        let capability = request.capability.clone();

        if cancel.is_some_and(CancellationToken::is_cancelled) {
            return Err("Capability request cancelled.".to_string());
        }

        let receiver = handler(request);
        self.await_response(capability, receiver, cancel).await
    }
}

pub fn request_capability_tool_definition() -> ToolDefinition {
    ToolDefinition {
        name: REQUEST_CAPABILITY_TOOL.to_string(),
        description: "Request an expanded capability for this session. Use when an action is denied and you need the capability to complete the task. Provide a clear reason why the capability is needed. The user will approve or deny the request. Rate limited to 3 requests per session.".to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "capability": {
                    "type": "string",
                    "description": "The capability identifier to request (e.g., 'network.external', 'shell', 'filesystem.write')"
                },
                "reason": {
                    "type": "string",
                    "description": "Why this capability is needed to complete the current task"
                }
            },
            "required": ["capability", "reason"]
        }),
    }
}

fn rate_limit_message() -> String {
    format!(
        "Rate limit reached ({MAX_REQUESTS_PER_SESSION}/{MAX_REQUESTS_PER_SESSION} requests used). Adapt your approach to work within current capabilities."
    )
}

async fn wait_for_decision(
    receiver: oneshot::Receiver<bool>,
    cancel: Option<&CancellationToken>,
) -> Option<bool> {
    match cancel {
        Some(token) => {
            tokio::select! {
                biased;
                _ = token.cancelled() => None,
                result = tokio::time::timeout(REQUEST_TIMEOUT, receiver) => extract_decision(result),
            }
        }
        None => extract_decision(tokio::time::timeout(REQUEST_TIMEOUT, receiver).await),
    }
}

fn extract_decision(
    result: Result<Result<bool, oneshot::error::RecvError>, tokio::time::error::Elapsed>,
) -> Option<bool> {
    match result {
        Ok(Ok(granted)) => Some(granted),
        Ok(Err(_)) | Err(_) => None,
    }
}

fn build_response(capability: String, granted: Option<bool>) -> CapabilityResponse {
    match granted {
        Some(true) => CapabilityResponse {
            granted: true,
            capability,
            note: "Capability granted for this session. You may retry the denied action."
                .to_string(),
        },
        Some(false) => CapabilityResponse {
            granted: false,
            capability,
            note: "Request denied by user. Use an alternative approach.".to_string(),
        },
        None => CapabilityResponse {
            granted: false,
            capability,
            note: "Request timed out. User did not respond.".to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn mock_handler(grant: bool) -> CapabilityRequestHandler {
        Arc::new(move |_request| {
            let (tx, rx) = oneshot::channel();
            let _ = tx.send(grant);
            rx
        })
    }

    fn parse_output(output: &str) -> CapabilityResponse {
        serde_json::from_str(output).expect("response json")
    }

    #[test]
    fn tool_definition_has_correct_schema() {
        let definition = request_capability_tool_definition();
        let properties = definition.parameters["properties"]
            .as_object()
            .expect("properties object");
        let required = definition.parameters["required"]
            .as_array()
            .expect("required array");

        assert_eq!(definition.name, REQUEST_CAPABILITY_TOOL);
        assert!(properties.contains_key("capability"));
        assert!(properties.contains_key("reason"));
        assert_eq!(required, &vec![json!("capability"), json!("reason")]);
    }

    #[tokio::test]
    async fn execute_without_handler_returns_unavailable() {
        let skill = CapabilityRequestSkill::new(None);
        let result = skill
            .execute(
                REQUEST_CAPABILITY_TOOL,
                r#"{"capability":"shell","reason":"need it"}"#,
                None,
            )
            .await
            .expect("handled");

        assert_eq!(
            result.expect_err("expected unavailable error"),
            "Capability requests are not available in this session."
        );
    }

    #[tokio::test]
    async fn execute_with_grant_returns_granted() {
        let skill = CapabilityRequestSkill::new(Some(mock_handler(true)));
        let result = skill
            .execute(
                REQUEST_CAPABILITY_TOOL,
                r#"{"capability":"shell","reason":"need it"}"#,
                None,
            )
            .await
            .expect("handled")
            .expect("successful result");
        let response = parse_output(&result);

        assert!(response.granted);
        assert_eq!(response.capability, "shell");
        assert_eq!(
            response.note,
            "Capability granted for this session. You may retry the denied action."
        );
    }

    #[tokio::test]
    async fn execute_with_deny_returns_denied() {
        let skill = CapabilityRequestSkill::new(Some(mock_handler(false)));
        let result = skill
            .execute(
                REQUEST_CAPABILITY_TOOL,
                r#"{"capability":"network.external","reason":"need it"}"#,
                None,
            )
            .await
            .expect("handled")
            .expect("successful result");
        let response = parse_output(&result);

        assert!(!response.granted);
        assert_eq!(response.capability, "network.external");
        assert_eq!(
            response.note,
            "Request denied by user. Use an alternative approach."
        );
    }

    #[tokio::test]
    async fn execute_timeout_returns_denied() {
        tokio::time::pause();
        let handler: CapabilityRequestHandler = Arc::new(|_request| {
            let (_tx, rx) = oneshot::channel();
            rx
        });
        let skill = CapabilityRequestSkill::new(Some(handler));

        let execution = tokio::spawn(async move {
            skill
                .execute(
                    REQUEST_CAPABILITY_TOOL,
                    r#"{"capability":"shell","reason":"need it"}"#,
                    None,
                )
                .await
        });

        tokio::task::yield_now().await;
        tokio::time::advance(Duration::from_secs(61)).await;

        let result = execution
            .await
            .expect("task should complete")
            .expect("handled")
            .expect("successful result");
        let response = parse_output(&result);

        assert!(!response.granted);
        assert_eq!(response.capability, "shell");
        assert!(response.note.contains("timed out"));
    }

    #[tokio::test]
    async fn execute_cancelled_returns_error() {
        let skill = CapabilityRequestSkill::new(Some(mock_handler(true)));
        let cancel = CancellationToken::new();
        cancel.cancel();

        let result = skill
            .execute(
                REQUEST_CAPABILITY_TOOL,
                r#"{"capability":"shell","reason":"need it"}"#,
                Some(&cancel),
            )
            .await
            .expect("handled");

        assert!(result
            .expect_err("expected cancellation error")
            .contains("cancelled"));
    }

    #[tokio::test]
    async fn empty_capability_returns_error() {
        let skill = CapabilityRequestSkill::new(Some(mock_handler(true)));
        let result = skill
            .execute(
                REQUEST_CAPABILITY_TOOL,
                r#"{"capability":"   ","reason":"need it"}"#,
                None,
            )
            .await
            .expect("handled");

        assert_eq!(
            result.expect_err("expected empty capability error"),
            "'capability' must not be empty"
        );
    }

    #[tokio::test]
    async fn empty_reason_returns_error() {
        let skill = CapabilityRequestSkill::new(Some(mock_handler(true)));
        let result = skill
            .execute(
                REQUEST_CAPABILITY_TOOL,
                r#"{"capability":"shell","reason":"   "}"#,
                None,
            )
            .await
            .expect("handled");

        assert_eq!(
            result.expect_err("expected empty reason error"),
            "'reason' must not be empty"
        );
    }

    #[tokio::test]
    async fn rate_limit_enforced_after_max_requests() {
        let skill = CapabilityRequestSkill::new(Some(mock_handler(true)));
        let args = r#"{"capability":"shell","reason":"need it"}"#;

        for _ in 0..MAX_REQUESTS_PER_SESSION {
            let result = skill
                .execute(REQUEST_CAPABILITY_TOOL, args, None)
                .await
                .expect("handled");
            assert!(result.is_ok());
        }

        let result = skill
            .execute(REQUEST_CAPABILITY_TOOL, args, None)
            .await
            .expect("handled");

        assert_eq!(
            result.expect_err("expected rate limit"),
            rate_limit_message()
        );
    }

    #[tokio::test]
    async fn rate_limit_counts_correctly() {
        let skill = CapabilityRequestSkill::new(Some(mock_handler(true)));
        let args = r#"{"capability":"shell","reason":"need it"}"#;

        assert_eq!(skill.request_count(), 0);
        let _ = skill
            .execute(REQUEST_CAPABILITY_TOOL, args, None)
            .await
            .expect("handled")
            .expect("ok");
        assert_eq!(skill.request_count(), 1);
        let _ = skill
            .execute(REQUEST_CAPABILITY_TOOL, args, None)
            .await
            .expect("handled")
            .expect("ok");
        assert_eq!(skill.request_count(), 2);
    }

    #[tokio::test]
    async fn missing_capability_argument_returns_error() {
        let skill = CapabilityRequestSkill::new(Some(mock_handler(true)));
        let result = skill
            .execute(REQUEST_CAPABILITY_TOOL, r#"{"reason":"need it"}"#, None)
            .await
            .expect("handled");

        assert!(result
            .expect_err("expected parse error")
            .contains("capability"));
    }

    #[tokio::test]
    async fn missing_reason_argument_returns_error() {
        let skill = CapabilityRequestSkill::new(Some(mock_handler(true)));
        let result = skill
            .execute(REQUEST_CAPABILITY_TOOL, r#"{"capability":"shell"}"#, None)
            .await
            .expect("handled");

        assert!(result.expect_err("expected parse error").contains("reason"));
    }
}
