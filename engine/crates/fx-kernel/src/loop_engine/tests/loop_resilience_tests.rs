use super::test_fixtures::{text_response, tool_use_response, RecordingLlm};
use super::*;
use crate::act::{
    ExternalActionEvidence, FailureClass, RunCommandDiagnostics, ToolCallClassification,
    ToolDiagnostics, ToolExecutionDiagnostics, ToolExecutor, ToolResult,
};
use crate::budget::{ActionCost, BudgetConfig, BudgetTracker, TerminationConfig};
use crate::cancellation::CancellationToken;
use crate::context_manager::ContextCompactor;
use async_trait::async_trait;
use fx_core::error::LlmError as CoreLlmError;
use fx_core::types::{InputSource, ScreenState, UserInput};
use fx_llm::{CompletionResponse, ContentBlock, Message, ProviderError, ToolCall, ToolDefinition};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Debug, Default)]
struct StubToolExecutor;

#[async_trait]
impl ToolExecutor for StubToolExecutor {
    async fn execute_tools(
        &self,
        calls: &[ToolCall],
        _cancel: Option<&CancellationToken>,
    ) -> Result<Vec<ToolResult>, crate::act::ToolExecutorError> {
        Ok(calls
            .iter()
            .map(|call| ToolResult {
                tool_call_id: call.id.clone(),
                tool_name: call.name.clone(),
                success: true,
                output: "ok".to_string(),
                failure_class: None,
            })
            .collect())
    }

    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        vec![ToolDefinition {
            name: "read_file".to_string(),
            description: "Read a file".to_string(),
            parameters: serde_json::json!({"type":"object"}),
        }]
    }
}

#[derive(Debug, Default)]
struct ObservationMixedToolExecutor;

#[derive(Debug)]
struct StatefulReadWriteExecutor {
    readme: Arc<Mutex<String>>,
}

impl StatefulReadWriteExecutor {
    fn new(readme: &str) -> Self {
        Self {
            readme: Arc::new(Mutex::new(readme.to_string())),
        }
    }

    fn readme_contents(&self) -> String {
        self.readme.lock().expect("readme lock").clone()
    }
}

#[async_trait]
impl ToolExecutor for StatefulReadWriteExecutor {
    async fn execute_tools(
        &self,
        calls: &[ToolCall],
        _cancel: Option<&CancellationToken>,
    ) -> Result<Vec<ToolResult>, crate::act::ToolExecutorError> {
        let mut readme = self.readme.lock().expect("readme lock");
        Ok(calls
            .iter()
            .map(|call| {
                let success = true;
                let output = match call.name.as_str() {
                    "read_file" => readme.clone(),
                    "write_file" => {
                        let content = call
                            .arguments
                            .get("content")
                            .and_then(serde_json::Value::as_str)
                            .expect("write_file content")
                            .to_string();
                        *readme = content;
                        "wrote README.md".to_string()
                    }
                    other => format!("unsupported tool: {other}"),
                };
                ToolResult {
                    tool_call_id: call.id.clone(),
                    tool_name: call.name.clone(),
                    success,
                    output,
                    failure_class: None,
                }
            })
            .collect())
    }

    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        vec![
            ToolDefinition {
                name: "read_file".to_string(),
                description: "Read a file".to_string(),
                parameters: serde_json::json!({"type":"object"}),
            },
            ToolDefinition {
                name: "write_file".to_string(),
                description: "Write a file".to_string(),
                parameters: serde_json::json!({"type":"object"}),
            },
        ]
    }

    fn cacheability(&self, tool_name: &str) -> crate::act::ToolCacheability {
        match tool_name {
            "write_file" => crate::act::ToolCacheability::SideEffect,
            "read_file" => crate::act::ToolCacheability::Cacheable,
            _ => crate::act::ToolCacheability::NeverCache,
        }
    }
}

#[derive(Debug)]
struct ReadEvidenceLlm {
    call_count: AtomicUsize,
    expected_tool_text: String,
}

impl ReadEvidenceLlm {
    fn new(expected_tool_text: &str) -> Self {
        Self {
            call_count: AtomicUsize::new(0),
            expected_tool_text: expected_tool_text.to_string(),
        }
    }
}

#[async_trait]
impl LlmProvider for ReadEvidenceLlm {
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
        "read-evidence"
    }

    async fn complete(
        &self,
        request: CompletionRequest,
    ) -> Result<CompletionResponse, ProviderError> {
        let index = self.call_count.fetch_add(1, Ordering::SeqCst);
        Ok(match index {
            0 => tool_use_response(vec![ToolCall {
                id: "read-1".to_string(),
                name: "read_file".to_string(),
                arguments: serde_json::json!({"path":"README.md"}),
            }]),
            1 => {
                if request_contains_tool_result_text(&request, &self.expected_tool_text) {
                    text_response("ACTUAL FINAL LINE")
                } else {
                    text_response("WRONG SYNTHETIC FINAL LINE")
                }
            }
            other => {
                return Err(ProviderError::Provider(format!(
                    "unexpected completion call {other}"
                )))
            }
        })
    }
}

#[derive(Debug)]
struct AppendEvidenceLlm {
    call_count: AtomicUsize,
    baseline_readme: String,
    verification_line: String,
}

impl AppendEvidenceLlm {
    fn new(baseline_readme: &str, verification_line: &str) -> Self {
        Self {
            call_count: AtomicUsize::new(0),
            baseline_readme: baseline_readme.to_string(),
            verification_line: verification_line.to_string(),
        }
    }
}

#[async_trait]
impl LlmProvider for AppendEvidenceLlm {
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
        "append-evidence"
    }

    async fn complete(
        &self,
        request: CompletionRequest,
    ) -> Result<CompletionResponse, ProviderError> {
        let index = self.call_count.fetch_add(1, Ordering::SeqCst);
        Ok(match index {
            0 => tool_use_response(vec![ToolCall {
                id: "read-1".to_string(),
                name: "read_file".to_string(),
                arguments: serde_json::json!({"path":"README.md"}),
            }]),
            1 => {
                let rewritten = format!("README summary only\n{}", self.verification_line);
                let appended = format!("{}\n{}", self.baseline_readme, self.verification_line);
                let content = if request_contains_tool_result_text(&request, &self.baseline_readme)
                {
                    appended
                } else {
                    rewritten
                };
                tool_use_response(vec![ToolCall {
                    id: "write-1".to_string(),
                    name: "write_file".to_string(),
                    arguments: serde_json::json!({
                        "path":"README.md",
                        "content": content,
                    }),
                }])
            }
            2 | 3 => text_response("Appended the verification line."),
            other => {
                return Err(ProviderError::Provider(format!(
                    "unexpected completion call {other}"
                )))
            }
        })
    }
}

#[derive(Debug)]
struct MixedBatchMutationBarrierLlm {
    call_count: AtomicUsize,
    baseline_readme: String,
    verification_line: String,
}

impl MixedBatchMutationBarrierLlm {
    fn new(baseline_readme: &str, verification_line: &str) -> Self {
        Self {
            call_count: AtomicUsize::new(0),
            baseline_readme: baseline_readme.to_string(),
            verification_line: verification_line.to_string(),
        }
    }
}

#[async_trait]
impl LlmProvider for MixedBatchMutationBarrierLlm {
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
        "mixed-mutation-barrier"
    }

    async fn complete(
        &self,
        request: CompletionRequest,
    ) -> Result<CompletionResponse, ProviderError> {
        let index = self.call_count.fetch_add(1, Ordering::SeqCst);
        Ok(match index {
            0 => tool_use_response(vec![
                ToolCall {
                    id: "read-1".to_string(),
                    name: "read_file".to_string(),
                    arguments: serde_json::json!({"path":"README.md"}),
                },
                ToolCall {
                    id: "write-1".to_string(),
                    name: "write_file".to_string(),
                    arguments: serde_json::json!({
                        "path":"README.md",
                        "content":"SHOULD NOT RUN BEFORE REVIEW",
                    }),
                },
            ]),
            1 => {
                assert!(
                    request_contains_tool_result_text(&request, &self.baseline_readme),
                    "second reasoning pass should see the read_file result"
                );
                assert_eq!(
                    request_tool_use_names(&request),
                    vec!["read_file".to_string()],
                    "only the observation call should have executed in the first round"
                );
                assert!(
                    request_contains_text(
                        &request,
                        "Calls from the first mutation onward were deferred",
                    ),
                    "next reasoning pass should be told that mutation calls were deferred"
                );
                tool_use_response(vec![ToolCall {
                    id: "write-2".to_string(),
                    name: "write_file".to_string(),
                    arguments: serde_json::json!({
                        "path":"README.md",
                        "content": format!("{}\n{}", self.baseline_readme, self.verification_line),
                    }),
                }])
            }
            2 | 3 => text_response("Applied the write after reviewing the file."),
            other => {
                return Err(ProviderError::Provider(format!(
                    "unexpected completion call {other}"
                )))
            }
        })
    }
}

#[derive(Debug)]
struct SequentialMutationBarrierLlm {
    call_count: AtomicUsize,
}

impl SequentialMutationBarrierLlm {
    fn new() -> Self {
        Self {
            call_count: AtomicUsize::new(0),
        }
    }
}

#[async_trait]
impl LlmProvider for SequentialMutationBarrierLlm {
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
        "sequential-mutation-barrier"
    }

    async fn complete(
        &self,
        request: CompletionRequest,
    ) -> Result<CompletionResponse, ProviderError> {
        let index = self.call_count.fetch_add(1, Ordering::SeqCst);
        Ok(match index {
            0 => tool_use_response(vec![
                ToolCall {
                    id: "write-1".to_string(),
                    name: "write_file".to_string(),
                    arguments: serde_json::json!({"path":"README.md","content":"first write"}),
                },
                ToolCall {
                    id: "write-2".to_string(),
                    name: "write_file".to_string(),
                    arguments: serde_json::json!({"path":"README.md","content":"second write"}),
                },
            ]),
            1 => {
                assert_eq!(
                    request_tool_use_names(&request),
                    vec!["write_file".to_string()],
                    "only one mutation should execute per round"
                );
                assert!(
                    request_contains_text(
                        &request,
                        "Mutation-capable tool calls are executed one at a time.",
                    ),
                    "next reasoning pass should be told the remaining mutation was deferred"
                );
                tool_use_response(vec![ToolCall {
                    id: "write-3".to_string(),
                    name: "write_file".to_string(),
                    arguments: serde_json::json!({"path":"README.md","content":"second write"}),
                }])
            }
            2 | 3 => text_response("Applied both writes safely."),
            other => {
                return Err(ProviderError::Provider(format!(
                    "unexpected completion call {other}"
                )))
            }
        })
    }
}

#[async_trait]
impl ToolExecutor for ObservationMixedToolExecutor {
    async fn execute_tools(
        &self,
        calls: &[ToolCall],
        _cancel: Option<&CancellationToken>,
    ) -> Result<Vec<ToolResult>, crate::act::ToolExecutorError> {
        Ok(calls
            .iter()
            .map(|call| ToolResult {
                tool_call_id: call.id.clone(),
                tool_name: call.name.clone(),
                success: true,
                output: "ok".to_string(),
                failure_class: None,
            })
            .collect())
    }

    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        vec![
            ToolDefinition {
                name: "read_file".to_string(),
                description: "Read a file".to_string(),
                parameters: serde_json::json!({"type":"object"}),
            },
            ToolDefinition {
                name: "write_file".to_string(),
                description: "Write a file".to_string(),
                parameters: serde_json::json!({"type":"object"}),
            },
        ]
    }

    fn cacheability(&self, tool_name: &str) -> crate::act::ToolCacheability {
        match tool_name {
            "write_file" => crate::act::ToolCacheability::SideEffect,
            "read_file" => crate::act::ToolCacheability::Cacheable,
            _ => crate::act::ToolCacheability::NeverCache,
        }
    }
}

#[derive(Debug, Default)]
struct DirectUtilityToolExecutor;

#[async_trait]
impl ToolExecutor for DirectUtilityToolExecutor {
    async fn execute_tools(
        &self,
        calls: &[ToolCall],
        _cancel: Option<&CancellationToken>,
    ) -> Result<Vec<ToolResult>, crate::act::ToolExecutorError> {
        Ok(calls
            .iter()
            .map(|call| ToolResult {
                tool_call_id: call.id.clone(),
                tool_name: call.name.clone(),
                success: true,
                output: match call.name.as_str() {
                    "weather" => "Bradenton, Florida is sunny and about 66F.".to_string(),
                    "current_time" => "2026-03-28T07:05:00-06:00".to_string(),
                    other => format!("{other} ok"),
                },
                failure_class: None,
            })
            .collect())
    }

    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        vec![
            ToolDefinition {
                name: "weather".to_string(),
                description: "Get the weather for a location".to_string(),
                parameters: serde_json::json!({
                    "type":"object",
                    "properties": {
                        "location": {
                            "type": "string",
                            "description": "City or location to check weather for"
                        },
                        "units": {
                            "type": "string",
                            "description": "Optional units override"
                        }
                    },
                    "required": ["location"],
                    "x-fawx-direct-utility": {
                        "enabled": true,
                        "profile": "weather",
                        "trigger_patterns": ["weather", "forecast"]
                    }
                }),
            },
            ToolDefinition {
                name: "current_time".to_string(),
                description: "Get the current time".to_string(),
                parameters: serde_json::json!({
                    "type":"object",
                    "properties":{},
                    "required": [],
                    "x-fawx-direct-utility": {
                        "enabled": true,
                        "profile": "current_time",
                        "trigger_patterns": [
                            "current time",
                            "what time",
                            "what's the time",
                            "whats the time",
                            "time is it"
                        ]
                    }
                }),
            },
            ToolDefinition {
                name: "web_search".to_string(),
                description: "Search the web".to_string(),
                parameters: serde_json::json!({"type":"object"}),
            },
            ToolDefinition {
                name: "run_command".to_string(),
                description: "Run a shell command".to_string(),
                parameters: serde_json::json!({"type":"object"}),
            },
        ]
    }

    fn cacheability(&self, tool_name: &str) -> crate::act::ToolCacheability {
        match tool_name {
            "run_command" => crate::act::ToolCacheability::SideEffect,
            "weather" | "web_search" => crate::act::ToolCacheability::Cacheable,
            "current_time" => crate::act::ToolCacheability::NeverCache,
            _ => crate::act::ToolCacheability::NeverCache,
        }
    }
}

#[derive(Debug, Default)]
struct FailingDirectWeatherExecutor;

#[derive(Debug)]
struct DeterministicLocalToolExecutor {
    executed_calls: Arc<Mutex<Vec<ToolCall>>>,
}

impl DeterministicLocalToolExecutor {
    fn new(executed_calls: Arc<Mutex<Vec<ToolCall>>>) -> Self {
        Self { executed_calls }
    }
}

fn direct_weather_profile() -> DirectUtilityProfile {
    DirectUtilityProfile::test_single_required_string(
        "weather",
        "Get the weather for a location",
        "location",
        "city or location",
        &["weather", "forecast"],
    )
}

fn direct_current_time_profile() -> DirectUtilityProfile {
    DirectUtilityProfile::test_empty_object(
        "current_time",
        "Get the current time",
        &[
            "current time",
            "what time",
            "what's the time",
            "whats the time",
            "time is it",
        ],
    )
}

#[async_trait]
impl ToolExecutor for FailingDirectWeatherExecutor {
    async fn execute_tools(
        &self,
        calls: &[ToolCall],
        _cancel: Option<&CancellationToken>,
    ) -> Result<Vec<ToolResult>, crate::act::ToolExecutorError> {
        Ok(calls
            .iter()
            .map(|call| ToolResult {
                tool_call_id: call.id.clone(),
                tool_name: call.name.clone(),
                success: false,
                output: "No weather results found for 'Denver, CO'.".to_string(),
                failure_class: None,
            })
            .collect())
    }

    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        vec![ToolDefinition {
            name: "weather".to_string(),
            description: "Get the weather for a location".to_string(),
            parameters: serde_json::json!({
                "type":"object",
                "properties": {
                    "location": {
                        "type": "string",
                        "description": "City or location to check weather for"
                    }
                },
                "required": ["location"],
                "x-fawx-direct-utility": {
                    "enabled": true,
                    "profile": "weather",
                    "trigger_patterns": ["weather", "forecast"]
                }
            }),
        }]
    }

    fn cacheability(&self, _tool_name: &str) -> crate::act::ToolCacheability {
        crate::act::ToolCacheability::Cacheable
    }
}

#[async_trait]
impl ToolExecutor for DeterministicLocalToolExecutor {
    async fn execute_tools(
        &self,
        calls: &[ToolCall],
        _cancel: Option<&CancellationToken>,
    ) -> Result<Vec<ToolResult>, crate::act::ToolExecutorError> {
        self.executed_calls
            .lock()
            .expect("deterministic local calls lock")
            .extend(calls.iter().cloned());

        if let Some(call) = calls.iter().find(|call| call.name == "run_command") {
            return Err(crate::act::ToolExecutorError {
                message: format!("run_command should not be used: {}", call.id),
                recoverable: false,
            });
        }

        Ok(calls
            .iter()
            .map(|call| ToolResult {
                tool_call_id: call.id.clone(),
                tool_name: call.name.clone(),
                success: true,
                output: match call.name.as_str() {
                    "open_browser_application" => match call.arguments["browser"].as_str() {
                        Some("chrome") => "Opened Google Chrome.".to_string(),
                        Some("firefox") => "Opened Firefox.".to_string(),
                        Some("safari") => "Opened Safari.".to_string(),
                        Some("brave") => "Opened Brave Browser.".to_string(),
                        Some("edge") => "Opened Microsoft Edge.".to_string(),
                        other => format!("Opened browser: {:?}.", other),
                    },
                    "open_browser_url" => format!(
                        "Opened {} in the default browser.",
                        call.arguments["url"].as_str().unwrap_or("unknown url")
                    ),
                    other => format!("{other} ok"),
                },
                failure_class: None,
            })
            .collect())
    }

    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        vec![
            ToolDefinition {
                name: "open_browser_application".to_string(),
                description: "Open a supported browser application locally.".to_string(),
                parameters: serde_json::json!({
                    "type":"object",
                    "properties": {
                        "browser": {
                            "type": "string",
                            "enum": ["chrome", "safari", "firefox", "brave", "edge"]
                        }
                    },
                    "required": ["browser"]
                }),
            },
            ToolDefinition {
                name: "open_browser_url".to_string(),
                description: "Open an HTTP or HTTPS URL in the default browser.".to_string(),
                parameters: serde_json::json!({
                    "type":"object",
                    "properties": {
                        "url": { "type": "string" }
                    },
                    "required": ["url"]
                }),
            },
            ToolDefinition {
                name: "run_command".to_string(),
                description: "Run a shell command".to_string(),
                parameters: serde_json::json!({"type":"object"}),
            },
        ]
    }

    fn cacheability(&self, tool_name: &str) -> crate::act::ToolCacheability {
        match tool_name {
            "open_browser_application" | "open_browser_url" | "run_command" => {
                crate::act::ToolCacheability::SideEffect
            }
            _ => crate::act::ToolCacheability::NeverCache,
        }
    }
}

#[derive(Debug, Default)]
struct ObservationMixedNoDecomposeExecutor;

#[derive(Debug, Default)]
struct LegacyWrappedWeatherExecutor;

#[derive(Debug, Default)]
struct UnannotatedStructuredWeatherExecutor;

#[async_trait]
impl ToolExecutor for LegacyWrappedWeatherExecutor {
    async fn execute_tools(
        &self,
        calls: &[ToolCall],
        _cancel: Option<&CancellationToken>,
    ) -> Result<Vec<ToolResult>, crate::act::ToolExecutorError> {
        Ok(calls
            .iter()
            .map(|call| ToolResult {
                tool_call_id: call.id.clone(),
                tool_name: call.name.clone(),
                success: true,
                output: "ok".to_string(),
                failure_class: None,
            })
            .collect())
    }

    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        vec![ToolDefinition {
            name: "weather".to_string(),
            description: "Get the weather for a location".to_string(),
            parameters: serde_json::json!({
                "type":"object",
                "properties": {
                    "input": {
                        "type": "string",
                        "description": "JSON input for the WASM skill"
                    }
                },
                "required": ["input"],
                "x-fawx-direct-utility": {
                    "enabled": true,
                    "trigger_patterns": ["weather", "forecast"]
                }
            }),
        }]
    }

    fn cacheability(&self, _tool_name: &str) -> crate::act::ToolCacheability {
        crate::act::ToolCacheability::Cacheable
    }
}

#[async_trait]
impl ToolExecutor for UnannotatedStructuredWeatherExecutor {
    async fn execute_tools(
        &self,
        calls: &[ToolCall],
        _cancel: Option<&CancellationToken>,
    ) -> Result<Vec<ToolResult>, crate::act::ToolExecutorError> {
        Ok(calls
            .iter()
            .map(|call| ToolResult {
                tool_call_id: call.id.clone(),
                tool_name: call.name.clone(),
                success: true,
                output: "ok".to_string(),
                failure_class: None,
            })
            .collect())
    }

    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        vec![ToolDefinition {
            name: "weather".to_string(),
            description: "Get the weather for a location".to_string(),
            parameters: serde_json::json!({
                "type":"object",
                "properties": {
                    "location": {
                        "type": "string",
                        "description": "City or location to check weather for"
                    }
                },
                "required": ["location"]
            }),
        }]
    }

    fn cacheability(&self, _tool_name: &str) -> crate::act::ToolCacheability {
        crate::act::ToolCacheability::Cacheable
    }
}

#[async_trait]
impl ToolExecutor for ObservationMixedNoDecomposeExecutor {
    async fn execute_tools(
        &self,
        calls: &[ToolCall],
        _cancel: Option<&CancellationToken>,
    ) -> Result<Vec<ToolResult>, crate::act::ToolExecutorError> {
        if let Some(call) = calls.iter().find(|call| call.name == DECOMPOSE_TOOL_NAME) {
            return Err(crate::act::ToolExecutorError {
                message: format!("decompose leaked to tool executor: {}", call.id),
                recoverable: false,
            });
        }

        Ok(calls
            .iter()
            .map(|call| ToolResult {
                tool_call_id: call.id.clone(),
                tool_name: call.name.clone(),
                success: true,
                output: "ok".to_string(),
                failure_class: None,
            })
            .collect())
    }

    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        vec![
            ToolDefinition {
                name: "read_file".to_string(),
                description: "Read a file".to_string(),
                parameters: serde_json::json!({"type":"object"}),
            },
            ToolDefinition {
                name: "write_file".to_string(),
                description: "Write a file".to_string(),
                parameters: serde_json::json!({"type":"object"}),
            },
        ]
    }

    fn cacheability(&self, tool_name: &str) -> crate::act::ToolCacheability {
        match tool_name {
            "write_file" => crate::act::ToolCacheability::SideEffect,
            "read_file" => crate::act::ToolCacheability::Cacheable,
            _ => crate::act::ToolCacheability::NeverCache,
        }
    }
}

#[derive(Debug, Default)]
struct ObservationRunCommandExecutor;

#[derive(Debug, Default)]
struct ObservationRunCommandFingerprintExecutor;

#[async_trait]
impl ToolExecutor for ObservationRunCommandExecutor {
    async fn execute_tools(
        &self,
        calls: &[ToolCall],
        _cancel: Option<&CancellationToken>,
    ) -> Result<Vec<ToolResult>, crate::act::ToolExecutorError> {
        Ok(calls
            .iter()
            .map(|call| ToolResult {
                tool_call_id: call.id.clone(),
                tool_name: call.name.clone(),
                success: true,
                output: "ok".to_string(),
                failure_class: None,
            })
            .collect())
    }

    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        vec![
            ToolDefinition {
                name: "run_command".to_string(),
                description: "Run a command".to_string(),
                parameters: serde_json::json!({"type":"object"}),
            },
            ToolDefinition {
                name: "write_file".to_string(),
                description: "Write a file".to_string(),
                parameters: serde_json::json!({"type":"object"}),
            },
        ]
    }

    fn cacheability(&self, tool_name: &str) -> crate::act::ToolCacheability {
        match tool_name {
            "run_command" | "write_file" => crate::act::ToolCacheability::SideEffect,
            _ => crate::act::ToolCacheability::NeverCache,
        }
    }

    fn classify_call(&self, call: &ToolCall) -> ToolCallClassification {
        if call.name == "run_command"
            && call.arguments.get("command")
                == Some(&serde_json::Value::String("cat README.md".to_string()))
        {
            ToolCallClassification::Observation
        } else {
            ToolCallClassification::Mutation
        }
    }
}

#[async_trait]
impl ToolExecutor for ObservationRunCommandFingerprintExecutor {
    async fn execute_tools(
        &self,
        calls: &[ToolCall],
        _cancel: Option<&CancellationToken>,
    ) -> Result<Vec<ToolResult>, crate::act::ToolExecutorError> {
        Ok(calls
            .iter()
            .map(|call| ToolResult {
                tool_call_id: call.id.clone(),
                tool_name: call.name.clone(),
                success: true,
                output: "ok".to_string(),
                failure_class: None,
            })
            .collect())
    }

    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        vec![
            ToolDefinition {
                name: "run_command".to_string(),
                description: "Run a command".to_string(),
                parameters: serde_json::json!({"type":"object"}),
            },
            ToolDefinition {
                name: "write_file".to_string(),
                description: "Write a file".to_string(),
                parameters: serde_json::json!({"type":"object"}),
            },
        ]
    }

    fn cacheability(&self, tool_name: &str) -> crate::act::ToolCacheability {
        match tool_name {
            "run_command" | "write_file" => crate::act::ToolCacheability::SideEffect,
            _ => crate::act::ToolCacheability::NeverCache,
        }
    }

    fn classify_call(&self, call: &ToolCall) -> ToolCallClassification {
        if call.name == "run_command" {
            ToolCallClassification::Observation
        } else {
            ToolCallClassification::Mutation
        }
    }
}

#[derive(Debug, Default)]
struct FailingBoundedLocalEditExecutor;

#[async_trait]
impl ToolExecutor for FailingBoundedLocalEditExecutor {
    async fn execute_tools(
        &self,
        calls: &[ToolCall],
        _cancel: Option<&CancellationToken>,
    ) -> Result<Vec<ToolResult>, crate::act::ToolExecutorError> {
        Ok(calls
            .iter()
            .map(|call| ToolResult {
                tool_call_id: call.id.clone(),
                tool_name: call.name.clone(),
                success: false,
                output: match call.name.as_str() {
                    "edit_file" => "old_text not found in file".to_string(),
                    "read_file" | "search_text" => "ok".to_string(),
                    _ => "blocked".to_string(),
                },
                failure_class: None,
            })
            .collect())
    }

    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        vec![
            ToolDefinition {
                name: "search_text".to_string(),
                description: "Search text".to_string(),
                parameters: serde_json::json!({"type":"object"}),
            },
            ToolDefinition {
                name: "read_file".to_string(),
                description: "Read a file".to_string(),
                parameters: serde_json::json!({"type":"object"}),
            },
            ToolDefinition {
                name: "edit_file".to_string(),
                description: "Edit a file".to_string(),
                parameters: serde_json::json!({"type":"object"}),
            },
            ToolDefinition {
                name: "write_file".to_string(),
                description: "Write a file".to_string(),
                parameters: serde_json::json!({"type":"object"}),
            },
        ]
    }

    fn cacheability(&self, tool_name: &str) -> crate::act::ToolCacheability {
        match tool_name {
            "edit_file" | "write_file" => crate::act::ToolCacheability::SideEffect,
            "read_file" | "search_text" => crate::act::ToolCacheability::Cacheable,
            _ => crate::act::ToolCacheability::NeverCache,
        }
    }
}

/// Tool executor that returns large outputs for truncation testing.
#[derive(Debug)]
struct LargeOutputToolExecutor {
    output_size: usize,
}

#[async_trait]
impl ToolExecutor for LargeOutputToolExecutor {
    async fn execute_tools(
        &self,
        calls: &[ToolCall],
        _cancel: Option<&CancellationToken>,
    ) -> Result<Vec<ToolResult>, crate::act::ToolExecutorError> {
        Ok(calls
            .iter()
            .map(|call| ToolResult {
                tool_call_id: call.id.clone(),
                tool_name: call.name.clone(),
                success: true,
                output: "x".repeat(self.output_size),
                failure_class: None,
            })
            .collect())
    }

    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        vec![ToolDefinition {
            name: "read_file".to_string(),
            description: "Read a file".to_string(),
            parameters: serde_json::json!({"type":"object"}),
        }]
    }
}

#[derive(Debug)]
struct SequentialMockLlm {
    responses: Mutex<VecDeque<CompletionResponse>>,
}

impl SequentialMockLlm {
    fn new(responses: Vec<CompletionResponse>) -> Self {
        Self {
            responses: Mutex::new(VecDeque::from(responses)),
        }
    }
}

#[async_trait]
impl LlmProvider for SequentialMockLlm {
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
        "mock"
    }

    async fn complete(&self, _: CompletionRequest) -> Result<CompletionResponse, ProviderError> {
        self.responses
            .lock()
            .expect("lock")
            .pop_front()
            .ok_or_else(|| ProviderError::Provider("no response".to_string()))
    }
}

fn high_budget_engine() -> LoopEngine {
    LoopEngine::builder()
        .budget(BudgetTracker::new(BudgetConfig::default(), 0, 0))
        .context(ContextCompactor::new(2048, 256))
        .max_iterations(3)
        .tool_executor(Arc::new(StubToolExecutor))
        .synthesis_instruction("Summarize".to_string())
        .build()
        .expect("build")
}

fn mixed_tool_engine(config: BudgetConfig) -> LoopEngine {
    mixed_tool_engine_with_executor(config, Arc::new(ObservationMixedToolExecutor))
}

fn mixed_tool_engine_with_executor(
    config: BudgetConfig,
    tool_executor: Arc<dyn ToolExecutor>,
) -> LoopEngine {
    LoopEngine::builder()
        .budget(BudgetTracker::new(config, 0, 0))
        .context(ContextCompactor::new(2048, 256))
        .max_iterations(3)
        .tool_executor(tool_executor)
        .synthesis_instruction("Summarize".to_string())
        .build()
        .expect("build")
}

fn stateful_mixed_tool_engine(tool_executor: Arc<dyn ToolExecutor>) -> LoopEngine {
    LoopEngine::builder()
        .budget(BudgetTracker::new(BudgetConfig::default(), 0, 0))
        .context(ContextCompactor::new(2048, 256))
        .max_iterations(5)
        .tool_executor(tool_executor)
        .synthesis_instruction("Summarize".to_string())
        .build()
        .expect("build")
}

fn run_command_observation_engine(config: BudgetConfig) -> LoopEngine {
    LoopEngine::builder()
        .budget(BudgetTracker::new(config, 0, 0))
        .context(ContextCompactor::new(2048, 256))
        .max_iterations(3)
        .tool_executor(Arc::new(ObservationRunCommandExecutor))
        .synthesis_instruction("Summarize".to_string())
        .build()
        .expect("build")
}

fn observation_restriction_config() -> BudgetConfig {
    let mut config = BudgetConfig::default();
    config.termination.observation_only_round_strip_after_nudge = 1;
    config
}

fn fingerprint_run_command_engine(config: BudgetConfig) -> LoopEngine {
    LoopEngine::builder()
        .budget(BudgetTracker::new(config, 0, 0))
        .context(ContextCompactor::new(2048, 256))
        .max_iterations(3)
        .tool_executor(Arc::new(ObservationRunCommandFingerprintExecutor))
        .synthesis_instruction("Summarize".to_string())
        .build()
        .expect("build")
}

fn low_budget_engine() -> LoopEngine {
    let config = BudgetConfig {
        max_cost_cents: 100,
        soft_ceiling_percent: 80,
        ..BudgetConfig::default()
    };
    let mut tracker = BudgetTracker::new(config, 0, 0);
    // Push past the soft ceiling (81%)
    tracker.record(&ActionCost {
        cost_cents: 81,
        ..ActionCost::default()
    });
    LoopEngine::builder()
        .budget(tracker)
        .context(ContextCompactor::new(2048, 256))
        .max_iterations(3)
        .tool_executor(Arc::new(StubToolExecutor))
        .synthesis_instruction("Summarize".to_string())
        .build()
        .expect("build")
}

fn fan_out_engine(max_fan_out: usize) -> LoopEngine {
    let config = BudgetConfig {
        max_fan_out,
        max_tool_retries: u8::MAX,
        ..BudgetConfig::default()
    };
    LoopEngine::builder()
        .budget(BudgetTracker::new(config, 0, 0))
        .context(ContextCompactor::new(2048, 256))
        .max_iterations(5)
        .tool_executor(Arc::new(StubToolExecutor))
        .synthesis_instruction("Summarize".to_string())
        .build()
        .expect("build")
}

fn engine_with_tracker(budget: BudgetTracker) -> LoopEngine {
    LoopEngine::builder()
        .budget(budget)
        .context(ContextCompactor::new(2048, 256))
        .max_iterations(3)
        .tool_executor(Arc::new(StubToolExecutor))
        .synthesis_instruction("Summarize".to_string())
        .build()
        .expect("build")
}

fn engine_with_budget(config: BudgetConfig) -> LoopEngine {
    engine_with_tracker(BudgetTracker::new(config, 0, 0))
}

fn test_snapshot(text: &str) -> PerceptionSnapshot {
    PerceptionSnapshot {
        timestamp_ms: 1,
        screen: ScreenState {
            current_app: "terminal".to_string(),
            elements: Vec::new(),
            text_content: text.to_string(),
        },
        notifications: Vec::new(),
        active_app: "terminal".to_string(),
        user_input: Some(UserInput {
            text: text.to_string(),
            source: InputSource::Text,
            timestamp: 1,
            context_id: None,
            images: Vec::new(),
            documents: Vec::new(),
        }),
        sensor_data: None,
        conversation_history: vec![Message::user(text)],
        steer_context: None,
    }
}

fn request_contains_tool_result_text(request: &CompletionRequest, needle: &str) -> bool {
    request.messages.iter().any(|message| {
        message.content.iter().any(|block| match block {
            ContentBlock::ToolResult { content, .. } => {
                content.as_str().is_some_and(|text| text.contains(needle))
            }
            _ => false,
        })
    })
}

fn request_contains_text(request: &CompletionRequest, needle: &str) -> bool {
    request.messages.iter().any(|message| {
        message.content.iter().any(|block| match block {
            ContentBlock::Text { text } => text.contains(needle),
            ContentBlock::ToolResult { content, .. } => {
                content.as_str().is_some_and(|text| text.contains(needle))
            }
            _ => false,
        })
    })
}

fn request_tool_use_names(request: &CompletionRequest) -> Vec<String> {
    request
        .messages
        .iter()
        .flat_map(|message| message.content.iter())
        .filter_map(|block| match block {
            ContentBlock::ToolUse { name, .. } => Some(name.clone()),
            _ => None,
        })
        .collect()
}

fn complete_response(result: LoopResult) -> String {
    match result {
        LoopResult::Complete { response, .. } => response,
        other => panic!("expected complete result, got {other:?}"),
    }
}

// --- Test 4: Tool dispatch blocked when state() == Low ---
#[tokio::test]
async fn tool_dispatch_blocked_when_budget_low() {
    let mut engine = low_budget_engine();
    let decision = Decision::UseTools(vec![ToolCall {
        id: "1".to_string(),
        name: "read_file".to_string(),
        arguments: serde_json::json!({"path": "test.rs"}),
    }]);
    let context = vec![Message::user("read file")];
    let llm = SequentialMockLlm::new(vec![]);

    let result = engine
        .act(&decision, &llm, &context, CycleStream::disabled())
        .await
        .expect("act should succeed");

    assert!(
        result.response_text.contains("soft-ceiling"),
        "response should mention soft-ceiling: {}",
        result.response_text,
    );
    assert!(result.tool_results.is_empty(), "no tools should execute");
}

// --- Test 5: Decompose blocked at 85% cost ---
#[tokio::test]
async fn decompose_blocked_when_budget_low() {
    let config = BudgetConfig {
        max_cost_cents: 100,
        soft_ceiling_percent: 80,
        ..BudgetConfig::default()
    };
    let mut tracker = BudgetTracker::new(config, 0, 0);
    tracker.record(&ActionCost {
        cost_cents: 85,
        ..ActionCost::default()
    });
    let mut engine = LoopEngine::builder()
        .budget(tracker)
        .context(ContextCompactor::new(2048, 256))
        .max_iterations(3)
        .tool_executor(Arc::new(StubToolExecutor))
        .synthesis_instruction("Summarize".to_string())
        .build()
        .expect("build");

    let plan = fx_decompose::DecompositionPlan {
        sub_goals: vec![fx_decompose::SubGoal {
            description: "sub-goal".to_string(),
            required_tools: vec![],
            completion_contract: SubGoalContract::from_definition_of_done(None),
            complexity_hint: None,
        }],
        strategy: fx_decompose::AggregationStrategy::Sequential,
        reasoning_mode: fx_decompose::ReasoningMode::Standard,
        truncated_from: None,
    };
    let decision = Decision::Decompose(plan.clone());
    let context = vec![Message::user("do stuff")];
    let llm = SequentialMockLlm::new(vec![]);

    let result = engine
        .act(&decision, &llm, &context, CycleStream::disabled())
        .await
        .expect("act should succeed");

    assert!(
        result.response_text.contains("soft-ceiling"),
        "decompose should be blocked by soft-ceiling: {}",
        result.response_text,
    );
}

// --- Test 7: Performance signal emitted on Normal→Low transition ---
#[tokio::test]
async fn performance_signal_emitted_on_budget_low_transition() {
    let config = BudgetConfig {
        max_cost_cents: 100,
        soft_ceiling_percent: 80,
        ..BudgetConfig::default()
    };
    let mut tracker = BudgetTracker::new(config, 0, 0);
    // Push past soft ceiling
    tracker.record(&ActionCost {
        cost_cents: 81,
        ..ActionCost::default()
    });
    let mut engine = LoopEngine::builder()
        .budget(tracker)
        .context(ContextCompactor::new(2048, 256))
        .max_iterations(3)
        .tool_executor(Arc::new(StubToolExecutor))
        .synthesis_instruction("Summarize".to_string())
        .build()
        .expect("build");

    let snapshot = test_snapshot("hello");
    let _processed = engine.perceive(&snapshot).await.expect("perceive");

    let signals = engine.signals.drain_all();
    let perf_signals: Vec<_> = signals
        .iter()
        .filter(|s| s.kind == SignalKind::Performance && s.message.contains("budget soft-ceiling"))
        .collect();
    assert_eq!(
        perf_signals.len(),
        1,
        "exactly one performance signal on Normal→Low transition"
    );
}

// --- Test 7b: Performance signal fires only once across multiple perceive calls ---
#[tokio::test]
async fn performance_signal_emitted_only_once_across_perceive_calls() {
    let mut engine = low_budget_engine();
    let snapshot = test_snapshot("hello");

    // First perceive — should emit the signal
    let _first = engine.perceive(&snapshot).await.expect("perceive 1");
    // Second perceive — should NOT emit again
    let _second = engine.perceive(&snapshot).await.expect("perceive 2");

    let signals = engine.signals.drain_all();
    let perf_signals: Vec<_> = signals
        .iter()
        .filter(|s| s.kind == SignalKind::Performance && s.message.contains("budget soft-ceiling"))
        .collect();
    assert_eq!(
        perf_signals.len(),
        1,
        "performance signal should fire exactly once, not on every perceive()"
    );
}

// --- Test 7c: Wrap-up directive is system message, not user ---
#[tokio::test]
async fn wrap_up_directive_is_system_message() {
    let mut engine = low_budget_engine();
    let snapshot = test_snapshot("hello");
    let processed = engine.perceive(&snapshot).await.expect("perceive");

    let wrap_up_msg = processed
        .context_window
        .iter()
        .find(|msg| {
            msg.content.iter().any(|block| match block {
                ContentBlock::Text { text } => text.contains("running low on budget"),
                _ => false,
            })
        })
        .expect("wrap-up directive should exist");
    assert_eq!(
        wrap_up_msg.role,
        MessageRole::System,
        "wrap-up directive should be a system message, not user"
    );
}

// --- Test 8: Wrap-up directive present in perceive() when state() == Low ---
#[tokio::test]
async fn wrap_up_directive_injected_when_budget_low() {
    let mut engine = low_budget_engine();
    let snapshot = test_snapshot("hello");
    let processed = engine.perceive(&snapshot).await.expect("perceive");

    let has_wrap_up = processed.context_window.iter().any(|msg| {
        msg.content.iter().any(|block| match block {
            ContentBlock::Text { text } => text.contains("running low on budget"),
            _ => false,
        })
    });
    assert!(has_wrap_up, "wrap-up directive should be in context window");
}

// --- Test 8b: Wrap-up directive NOT present when budget Normal ---
#[tokio::test]
async fn no_wrap_up_directive_when_budget_normal() {
    let mut engine = high_budget_engine();
    let snapshot = test_snapshot("hello");
    let processed = engine.perceive(&snapshot).await.expect("perceive");

    let has_wrap_up = processed.context_window.iter().any(|msg| {
        msg.content.iter().any(|block| match block {
            ContentBlock::Text { text } => text.contains("running low on budget"),
            _ => false,
        })
    });
    assert!(!has_wrap_up, "no wrap-up directive when budget normal");
}

#[tokio::test]
async fn malformed_tool_args_skipped_with_error_result() {
    let mut engine = high_budget_engine();
    let calls = vec![
        ToolCall {
            id: "valid-1".to_string(),
            name: "read_file".to_string(),
            arguments: serde_json::json!({"path": "/tmp/test.md"}),
        },
        ToolCall {
            id: "malformed-1".to_string(),
            name: "write_file".to_string(),
            arguments: fx_llm::parse_tool_arguments_object("{broken json"),
        },
    ];
    let results = engine
        .execute_allowed_tool_calls(&calls, CycleStream::disabled())
        .await
        .expect("execute");

    // Valid call should produce a result from the executor
    let valid_result = results.iter().find(|r| r.tool_call_id == "valid-1");
    assert!(valid_result.is_some(), "valid call should have a result");

    // Malformed call should produce an error result without hitting the executor
    let malformed_result = results
        .iter()
        .find(|r| r.tool_call_id == "malformed-1")
        .expect("malformed call should have a result");
    assert!(!malformed_result.success);
    assert!(
        malformed_result.output.contains("could not be parsed"),
        "should explain the failure: {}",
        malformed_result.output
    );
    assert!(
        malformed_result.output.contains("line 1 column"),
        "should surface the parser error: {}",
        malformed_result.output
    );
    assert!(
        malformed_result.output.contains("Retry the same tool call"),
        "should provide retry guidance: {}",
        malformed_result.output
    );
}

#[tokio::test]
async fn tool_only_turn_nudge_injected_at_threshold() {
    let mut engine = high_budget_engine();
    engine.consecutive_tool_turns = 6;

    let processed = engine
        .perceive(&test_snapshot("hello"))
        .await
        .expect("perceive");

    let has_nudge = processed.context_window.iter().any(|msg| {
        msg.content.iter().any(|block| match block {
            ContentBlock::Text { text } => text.contains("working for several steps"),
            _ => false,
        })
    });
    assert!(has_nudge, "tool-only nudge should be in context window");
}

#[tokio::test]
async fn tool_only_turn_nudge_not_injected_below_threshold() {
    let mut engine = high_budget_engine();
    engine.consecutive_tool_turns = 6 - 1;

    let processed = engine
        .perceive(&test_snapshot("hello"))
        .await
        .expect("perceive");

    let has_nudge = processed.context_window.iter().any(|msg| {
        msg.content.iter().any(|block| match block {
            ContentBlock::Text { text } => text.contains("working for several steps"),
            _ => false,
        })
    });
    assert!(!has_nudge, "tool-only nudge should stay below threshold");
}

#[tokio::test]
async fn nudge_threshold_from_config() {
    let config = BudgetConfig {
        termination: TerminationConfig {
            nudge_after_tool_turns: 4,
            ..TerminationConfig::default()
        },
        ..BudgetConfig::default()
    };
    let mut engine = LoopEngine::builder()
        .budget(BudgetTracker::new(config, 0, 0))
        .context(ContextCompactor::new(2048, 256))
        .max_iterations(3)
        .tool_executor(Arc::new(StubToolExecutor))
        .synthesis_instruction("Summarize".to_string())
        .build()
        .expect("build");
    engine.consecutive_tool_turns = 4;

    let processed = engine
        .perceive(&test_snapshot("hello"))
        .await
        .expect("perceive");

    let has_nudge = processed.context_window.iter().any(|msg| {
        msg.content.iter().any(|block| match block {
            ContentBlock::Text { text } => text.contains("working for several steps"),
            _ => false,
        })
    });
    assert!(has_nudge, "nudge should fire at custom threshold 4");
}

#[tokio::test]
async fn nudge_disabled_when_zero() {
    let config = BudgetConfig {
        termination: TerminationConfig {
            nudge_after_tool_turns: 0,
            ..TerminationConfig::default()
        },
        ..BudgetConfig::default()
    };
    let mut engine = LoopEngine::builder()
        .budget(BudgetTracker::new(config, 0, 0))
        .context(ContextCompactor::new(2048, 256))
        .max_iterations(3)
        .tool_executor(Arc::new(StubToolExecutor))
        .synthesis_instruction("Summarize".to_string())
        .build()
        .expect("build");
    engine.consecutive_tool_turns = 100;

    let processed = engine
        .perceive(&test_snapshot("hello"))
        .await
        .expect("perceive");

    let has_nudge = processed.context_window.iter().any(|msg| {
        msg.content.iter().any(|block| match block {
            ContentBlock::Text { text } => text.contains("working for several steps"),
            _ => false,
        })
    });
    assert!(!has_nudge, "nudge should never fire when threshold is 0");
}

#[tokio::test]
async fn tools_stripped_immediately_when_grace_is_zero() {
    let config = BudgetConfig {
        termination: TerminationConfig {
            nudge_after_tool_turns: 3,
            strip_tools_after_nudge: 0,
            ..TerminationConfig::default()
        },
        ..BudgetConfig::default()
    };
    let mut engine = engine_with_budget(config);
    engine.consecutive_tool_turns = 3;
    let llm = RecordingLlm::ok(vec![CompletionResponse {
        content: vec![ContentBlock::Text {
            text: "Here is my summary.".to_string(),
        }],
        tool_calls: Vec::new(),
        usage: None,
        stop_reason: None,
    }]);

    let processed = engine
        .perceive(&test_snapshot("hello"))
        .await
        .expect("perceive");
    let _ = engine
        .reason(&processed, &llm, CycleStream::disabled())
        .await
        .expect("reason");

    assert!(llm.requests()[0].tools.is_empty());
}

#[tokio::test]
async fn tools_stripped_after_nudge_grace() {
    let config = BudgetConfig {
        termination: TerminationConfig {
            nudge_after_tool_turns: 3,
            strip_tools_after_nudge: 2,
            ..TerminationConfig::default()
        },
        ..BudgetConfig::default()
    };
    let mut engine = LoopEngine::builder()
        .budget(BudgetTracker::new(config, 0, 0))
        .context(ContextCompactor::new(2048, 256))
        .max_iterations(3)
        .tool_executor(Arc::new(StubToolExecutor))
        .synthesis_instruction("Summarize".to_string())
        .build()
        .expect("build");
    // At turn 5 (3 nudge + 2 grace), tools should be stripped
    engine.consecutive_tool_turns = 5;

    let llm = RecordingLlm::ok(vec![CompletionResponse {
        content: vec![ContentBlock::Text {
            text: "Here is my summary.".to_string(),
        }],
        tool_calls: Vec::new(),
        usage: None,
        stop_reason: None,
    }]);

    let processed = engine
        .perceive(&test_snapshot("hello"))
        .await
        .expect("perceive");
    let _ = engine
        .reason(&processed, &llm, CycleStream::disabled())
        .await
        .expect("reason");

    let requests = llm.requests();
    assert_eq!(requests.len(), 1);
    assert!(
        requests[0].tools.is_empty(),
        "tools should be stripped at turn {}, threshold {}",
        5,
        5
    );
}

#[tokio::test]
async fn reason_strip_preserves_mutation_tools_when_available() {
    let config = BudgetConfig {
        termination: TerminationConfig {
            nudge_after_tool_turns: 3,
            strip_tools_after_nudge: 0,
            ..TerminationConfig::default()
        },
        ..BudgetConfig::default()
    };
    let mut engine = mixed_tool_engine(config);
    engine.consecutive_tool_turns = 3;

    let llm = RecordingLlm::ok(vec![CompletionResponse {
        content: vec![ContentBlock::Text {
            text: "ready to implement".to_string(),
        }],
        tool_calls: Vec::new(),
        usage: None,
        stop_reason: None,
    }]);

    let processed = engine
        .perceive(&test_snapshot("Implement it now"))
        .await
        .expect("perceive");
    let _ = engine
        .reason(&processed, &llm, CycleStream::disabled())
        .await
        .expect("reason");

    let requests = llm.requests();
    assert_eq!(requests.len(), 1);
    assert!(
        requests[0]
            .tools
            .iter()
            .any(|tool| tool.name == "write_file"),
        "mutation tools should remain available after progress strip"
    );
    assert!(
        !requests[0]
            .tools
            .iter()
            .any(|tool| tool.name == "read_file"),
        "read-only tools should be removed after progress strip"
    );
}

#[tokio::test]
async fn direct_weather_profile_limits_reasoning_to_weather_and_disables_decompose() {
    let mut engine = mixed_tool_engine_with_executor(
        BudgetConfig::default(),
        Arc::new(DirectUtilityToolExecutor),
    );
    let processed = engine
        .perceive(&test_snapshot("What's the weather in Bradenton Florida?"))
        .await
        .expect("perceive");
    assert_eq!(
        engine.turn_execution_profile,
        TurnExecutionProfile::DirectUtility(direct_weather_profile())
    );

    let llm = RecordingLlm::ok(Vec::new());

    let response = engine
        .reason(&processed, &llm, CycleStream::disabled())
        .await
        .expect("reason");

    assert!(
        llm.requests().is_empty(),
        "direct tool path should bypass the LLM"
    );
    assert_eq!(response.tool_calls.len(), 1);
    assert_eq!(response.tool_calls[0].name, "weather");
    assert_eq!(
        response.tool_calls[0].arguments,
        serde_json::json!({"location":"Bradenton Florida"})
    );
}

#[tokio::test]
async fn direct_weather_tool_round_finishes_after_answering_from_results() {
    let mut engine = mixed_tool_engine_with_executor(
        BudgetConfig::default(),
        Arc::new(DirectUtilityToolExecutor),
    );
    engine.turn_execution_profile = TurnExecutionProfile::DirectUtility(direct_weather_profile());
    let decision = Decision::UseTools(vec![ToolCall {
        id: "weather-1".to_string(),
        name: "weather".to_string(),
        arguments: serde_json::json!({"location":"Bradenton, Florida"}),
    }]);
    let llm = RecordingLlm::ok(Vec::new());

    let action = engine
        .act(
            &decision,
            &llm,
            &[Message::user("What's the weather in Bradenton Florida?")],
            CycleStream::disabled(),
        )
        .await
        .expect("act should succeed");

    match action.next_step {
        ActionNextStep::Finish(ActionTerminal::Complete { response }) => {
            assert_eq!(response, "Bradenton, Florida is sunny and about 66F.");
        }
        other => panic!("expected direct tool completion, got {other:?}"),
    }
    assert!(
        llm.requests().is_empty(),
        "direct tool answers should not need a follow-up completion request"
    );
}

#[tokio::test]
async fn direct_weather_failure_returns_clean_kernel_authored_response() {
    let mut engine = mixed_tool_engine_with_executor(
        BudgetConfig::default(),
        Arc::new(FailingDirectWeatherExecutor),
    );
    engine.turn_execution_profile = TurnExecutionProfile::DirectUtility(direct_weather_profile());
    let decision = Decision::UseTools(vec![ToolCall {
        id: "weather-1".to_string(),
        name: "weather".to_string(),
        arguments: serde_json::json!({"location":"Denver, CO"}),
    }]);
    let llm = RecordingLlm::ok(Vec::new());

    let action = engine
        .act(
            &decision,
            &llm,
            &[Message::user("What's the weather in Denver, CO?")],
            CycleStream::disabled(),
        )
        .await
        .expect("act should succeed");

    match action.next_step {
        ActionNextStep::Finish(ActionTerminal::Complete { response }) => {
            assert_eq!(
                response,
                "I couldn't get the weather right now: No weather results found for 'Denver, CO'."
            );
        }
        other => panic!("expected direct tool completion, got {other:?}"),
    }
    assert!(
        llm.requests().is_empty(),
        "direct tool failures should not fall back into a follow-up completion request"
    );
}

#[tokio::test]
async fn direct_weather_reason_asks_for_location_when_missing() {
    let mut engine = mixed_tool_engine_with_executor(
        BudgetConfig::default(),
        Arc::new(DirectUtilityToolExecutor),
    );
    let processed = engine
        .perceive(&test_snapshot("What's the weather?"))
        .await
        .expect("perceive");
    let llm = RecordingLlm::ok(Vec::new());

    let response = engine
        .reason(&processed, &llm, CycleStream::disabled())
        .await
        .expect("reason");

    assert!(
        llm.requests().is_empty(),
        "direct tool path should bypass the LLM"
    );
    assert!(response.tool_calls.is_empty());
    assert_eq!(
        extract_response_text(&response),
        "Please tell me the city or location."
    );
}

#[tokio::test]
async fn legacy_wrapped_weather_schema_with_direct_utility_metadata_does_not_trigger_profile() {
    let mut engine = mixed_tool_engine_with_executor(
        BudgetConfig::default(),
        Arc::new(LegacyWrappedWeatherExecutor),
    );
    let _processed = engine
        .perceive(&test_snapshot("What's the weather in Miami?"))
        .await
        .expect("perceive");

    assert!(matches!(
        engine.turn_execution_profile,
        TurnExecutionProfile::Standard
    ));
}

#[tokio::test]
async fn structured_weather_schema_without_direct_utility_metadata_does_not_trigger_profile() {
    let mut engine = mixed_tool_engine_with_executor(
        BudgetConfig::default(),
        Arc::new(UnannotatedStructuredWeatherExecutor),
    );
    let _processed = engine
        .perceive(&test_snapshot("What's the weather in Miami?"))
        .await
        .expect("perceive");

    assert!(matches!(
        engine.turn_execution_profile,
        TurnExecutionProfile::Standard
    ));
}

#[tokio::test]
#[ignore = "legacy harness behavior replaced by simple agent loop"]
async fn deterministic_local_browser_request_uses_one_bounded_tool_path() {
    let executed_calls = Arc::new(Mutex::new(Vec::new()));
    let mut engine = mixed_tool_engine_with_executor(
        BudgetConfig::default(),
        Arc::new(DeterministicLocalToolExecutor::new(executed_calls.clone())),
    );
    let llm = RecordingLlm::ok(Vec::new());

    let result = engine
        .run_cycle(test_snapshot("Open a chrome browser tab"), &llm)
        .await
        .expect("run_cycle");

    assert!(
        llm.requests().is_empty(),
        "deterministic local browser intent should bypass the LLM"
    );
    assert!(matches!(
        engine.turn_execution_profile,
        TurnExecutionProfile::DeterministicLocal(_)
    ));
    assert_eq!(complete_response(result), "Opened Google Chrome.");

    let calls = executed_calls
        .lock()
        .expect("deterministic local calls lock")
        .clone();
    assert_eq!(calls.len(), 1, "exactly one local action should execute");
    assert_eq!(calls[0].name, "open_browser_application");
    assert_eq!(calls[0].arguments, serde_json::json!({"browser":"chrome"}));
    assert!(
        calls.iter().all(|call| call.name != "run_command"),
        "deterministic local browser intent must not improvise run_command"
    );
}

#[tokio::test]
async fn deterministic_local_url_request_bypasses_preflight_and_broad_reasoning() {
    let executed_calls = Arc::new(Mutex::new(Vec::new()));
    let mut engine = mixed_tool_engine_with_executor(
        BudgetConfig::default(),
        Arc::new(DeterministicLocalToolExecutor::new(executed_calls.clone())),
    );
    let processed = engine
        .perceive(&test_snapshot("Open https://example.com"))
        .await
        .expect("perceive");
    let llm = RecordingLlm::ok(Vec::new());

    assert!(matches!(
        engine.turn_execution_profile,
        TurnExecutionProfile::DeterministicLocal(_)
    ));
    assert!(
        engine.preflight_route_plan.is_none(),
        "deterministic local URL intent should not enter the preflight route planner"
    );

    let response = engine
        .reason(&processed, &llm, CycleStream::disabled())
        .await
        .expect("reason");

    assert!(
        llm.requests().is_empty(),
        "deterministic local URL intent should bypass the LLM"
    );
    assert_eq!(response.tool_calls.len(), 1);
    assert_eq!(response.tool_calls[0].name, "open_browser_url");
    assert_eq!(
        response.tool_calls[0].arguments,
        serde_json::json!({"url":"https://example.com"})
    );

    let action = engine
        .act(
            &Decision::UseTools(response.tool_calls.clone()),
            &llm,
            &[Message::user("Open https://example.com")],
            CycleStream::disabled(),
        )
        .await
        .expect("act should succeed");

    match action.next_step {
        ActionNextStep::Finish(ActionTerminal::Complete { response }) => {
            assert_eq!(
                response,
                "Opened https://example.com in the default browser."
            );
        }
        other => panic!("expected deterministic local completion, got {other:?}"),
    }

    let calls = executed_calls
        .lock()
        .expect("deterministic local calls lock")
        .clone();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].name, "open_browser_url");
}

#[tokio::test]
async fn ambiguous_local_open_request_falls_back_to_standard_profile() {
    let executed_calls = Arc::new(Mutex::new(Vec::new()));
    let mut engine = mixed_tool_engine_with_executor(
        BudgetConfig::default(),
        Arc::new(DeterministicLocalToolExecutor::new(executed_calls)),
    );
    let _processed = engine
        .perceive(&test_snapshot("Open chrome and https://example.com"))
        .await
        .expect("perceive");

    assert!(matches!(
        engine.turn_execution_profile,
        TurnExecutionProfile::Standard
    ));
}

#[tokio::test]
async fn unrelated_requests_still_use_standard_reasoning_path() {
    let executed_calls = Arc::new(Mutex::new(Vec::new()));
    let mut engine = mixed_tool_engine_with_executor(
        BudgetConfig::default(),
        Arc::new(DeterministicLocalToolExecutor::new(executed_calls)),
    );
    let processed = engine
        .perceive(&test_snapshot("Summarize the trade-offs in this plan."))
        .await
        .expect("perceive");
    let llm = RecordingLlm::ok(vec![text_response("Here are the trade-offs.")]);

    assert!(matches!(
        engine.turn_execution_profile,
        TurnExecutionProfile::Standard
    ));

    let response = engine
        .reason(&processed, &llm, CycleStream::disabled())
        .await
        .expect("reason");

    assert_eq!(llm.requests().len(), 1);
    assert_eq!(extract_response_text(&response), "Here are the trade-offs.");
}

#[tokio::test]
async fn observation_tool_continuation_requests_mutation_only_next() {
    let mut engine = mixed_tool_engine(BudgetConfig::default());
    engine.requested_artifact_target = Some("src/lib.rs".to_string());
    let decision = Decision::UseTools(vec![ToolCall {
        id: "call-1".to_string(),
        name: "read_file".to_string(),
        arguments: serde_json::json!({"path":"README.md"}),
    }]);
    let llm = SequentialMockLlm::new(vec![text_response(
        "I have enough context to implement it now.",
    )]);

    let action = engine
        .act(
            &decision,
            &llm,
            &[Message::user("Research first, then implement.")],
            CycleStream::disabled(),
        )
        .await
        .expect("act should succeed");

    match action.next_step {
        ActionNextStep::Continue(continuation) => {
            assert_eq!(
                continuation.next_tool_scope,
                Some(ContinuationToolScope::MutationOnly)
            );
            assert_eq!(
                    continuation.turn_commitment,
                    Some(TurnCommitment::ProceedUnderConstraints(
                        ProceedUnderConstraints {
                            goal: "Continue the active task with concrete execution using the selected tools: read_file".to_string(),
                            success_target: Some(
                                "Use a side-effect-capable tool to make concrete forward progress before doing any more broad research.".to_string()
                            ),
                            unsupported_items: Vec::new(),
                            assumptions: Vec::new(),
                            allowed_tools: Some(ContinuationToolScope::MutationOnly),
                        }
                    ))
                );
        }
        other => panic!("expected continuation, got {other:?}"),
    }
}

#[tokio::test]
async fn mutation_tool_continuation_without_pending_contract_finishes_terminally() {
    let mut engine = run_command_observation_engine(BudgetConfig::default());
    let decision = Decision::UseTools(vec![ToolCall {
        id: "call-1".to_string(),
        name: "run_command".to_string(),
        arguments: serde_json::json!({"command":"git pull"}),
    }]);
    let llm = SequentialMockLlm::new(vec![text_response(
        "The repo is updated; I'll continue with the milestone.",
    )]);

    let action = engine
        .act(
            &decision,
            &llm,
            &[Message::user(
                "Switch to dev, pull, and continue working on the milestone.",
            )],
            CycleStream::disabled(),
        )
        .await
        .expect("act should succeed");

    match action.next_step {
        ActionNextStep::Finish(ActionTerminal::Complete { response }) => {
            assert_eq!(
                response,
                "The repo is updated; I'll continue with the milestone."
            );
        }
        other => panic!("expected terminal completion, got {other:?}"),
    }
}

#[tokio::test]
async fn mutation_follow_up_without_pending_contract_does_not_reenter_reasoning() {
    let mut engine = run_command_observation_engine(BudgetConfig::default());
    let llm = RecordingLlm::ok(vec![
        tool_use_response(vec![ToolCall {
            id: "cmd-1".to_string(),
            name: "run_command".to_string(),
            arguments: serde_json::json!({"command":"git pull"}),
        }]),
        text_response("The repo is updated; I'll continue with the milestone."),
    ]);

    let result = engine
        .run_cycle(
            test_snapshot("Switch to dev, pull, and continue working on the milestone."),
            &llm,
        )
        .await
        .expect("run_cycle");

    assert_eq!(
        complete_response(result),
        "The repo is updated; I'll continue with the milestone."
    );
    let requests = llm.requests();
    assert_eq!(
        requests.len(),
        2,
        "answer-shaped tool continuation text should not force a synthetic follow-up reasoning pass"
    );
}

#[tokio::test]
async fn synthesizing_task_contract_removes_tools_from_next_reasoning_pass() {
    let mut engine = mixed_tool_engine(BudgetConfig::default());
    engine.task_contract = Some(TaskContract {
        inputs: vec![
            InputRequirement {
                description: "ENGINEERING.md".to_string(),
                normalized_description: normalize_contract_label("ENGINEERING.md"),
                satisfied: true,
            },
            InputRequirement {
                description: "TASTE.md".to_string(),
                normalized_description: normalize_contract_label("TASTE.md"),
                satisfied: true,
            },
        ],
        phase: TaskPhase::Synthesizing,
    });
    let llm = RecordingLlm::ok(vec![text_response("done")]);

    let processed = engine
        .perceive(&test_snapshot("Continue with the fix."))
        .await
        .expect("perceive");
    let _ = engine
        .reason(&processed, &llm, CycleStream::disabled())
        .await
        .expect("reason");

    let requests = llm.requests();
    assert_eq!(requests.len(), 1);
    assert!(
        requests[0].tools.is_empty(),
        "synthesizing phase should not expose more tools"
    );
    let system_prompt = requests[0].system_prompt.as_deref().expect("system prompt");
    assert!(system_prompt.contains("Task lifecycle contract:"));
    assert!(system_prompt.contains("Phase: synthesizing."));
    assert!(system_prompt.contains("Do not call more tools."));
}

#[tokio::test]
async fn initial_reasoning_prompt_requests_a_task_plan_before_tool_gathering() {
    let mut engine = mixed_tool_engine(BudgetConfig::default());
    let llm = RecordingLlm::ok(vec![tool_use_response(vec![ToolCall {
        id: "read-1".to_string(),
        name: "read_file".to_string(),
        arguments: serde_json::json!({"path":"ENGINEERING.md"}),
    }])]);

    let processed = engine
        .perceive(&test_snapshot(
            "Read ENGINEERING.md and TASTE.md before fixing the bug.",
        ))
        .await
        .expect("perceive");
    let _ = engine
        .reason(&processed, &llm, CycleStream::disabled())
        .await
        .expect("reason");

    let request = llm.requests().into_iter().next().expect("request");
    let system_prompt = request.system_prompt.expect("system prompt");
    assert!(system_prompt.contains("Task lifecycle declaration:"));
    assert!(system_prompt.contains("Task plan:"));
    assert!(system_prompt.contains("List only the concrete inputs"));
}

#[tokio::test]
async fn single_input_reasoning_prompt_skips_task_plan_declaration() {
    let mut engine = mixed_tool_engine(BudgetConfig::default());
    let llm = RecordingLlm::ok(vec![tool_use_response(vec![ToolCall {
        id: "read-1".to_string(),
        name: "read_file".to_string(),
        arguments: serde_json::json!({"path":"README.md"}),
    }])]);

    let processed = engine
        .perceive(&test_snapshot("Read README.md before answering."))
        .await
        .expect("perceive");
    let _ = engine
        .reason(&processed, &llm, CycleStream::disabled())
        .await
        .expect("reason");

    let request = llm.requests().into_iter().next().expect("request");
    let system_prompt = request.system_prompt.expect("system prompt");
    assert!(!system_prompt.contains("Task lifecycle declaration:"));
    assert!(!system_prompt.contains("Task plan:"));
}

#[test]
fn declaration_heuristic_rejects_period_terminated_abbreviations() {
    assert!(!should_request_task_contract_declaration(
        "Check the U.S. and E.U. policies"
    ));
}

#[tokio::test]
#[ignore = "legacy harness behavior replaced by simple agent loop"]
async fn observation_follow_up_still_injects_mutation_commitment_into_next_reasoning_pass() {
    let mut engine = mixed_tool_engine(BudgetConfig::default());
    let llm = RecordingLlm::ok(vec![
        tool_use_response(vec![ToolCall {
            id: "cmd-1".to_string(),
            name: "read_file".to_string(),
            arguments: serde_json::json!({"path":"README.md"}),
        }]),
        text_response("I have enough context to implement the next step."),
        text_response("done"),
    ]);

    let _ = engine
        .run_cycle(
            test_snapshot("Inspect README.md, then write to src/lib.rs."),
            &llm,
        )
        .await
        .expect("run_cycle");

    let requests = llm.requests();
    let committed_request = requests
        .iter()
        .find(|request| {
            request
                .system_prompt
                .as_deref()
                .is_some_and(|prompt| prompt.contains("Turn commitment:"))
        })
        .expect("expected a committed mutation-only follow-up request");
    let system_prompt = committed_request
        .system_prompt
        .as_deref()
        .expect("system prompt");
    assert!(system_prompt.contains("Turn commitment:"));
    assert!(system_prompt.contains(
        "Use a side-effect-capable tool to make concrete forward progress before doing any more broad research."
    ));
    assert!(
        committed_request
            .tools
            .iter()
            .any(|tool| tool.name == "write_file"),
        "mutation tools should stay available under the carried commitment"
    );
    assert!(
        !committed_request
            .tools
            .iter()
            .any(|tool| tool.name == "read_file"),
        "read-only inspection should stay hidden under the carried mutation-only commitment"
    );
}

#[tokio::test]
#[ignore = "legacy harness behavior replaced by simple agent loop"]
async fn explicit_deliverables_block_progress_only_terminal_response_after_tool_continuation() {
    let mut engine = run_command_observation_engine(BudgetConfig::default());
    let final_response = "\
**Plan**
- Implement the root-turn completion gate.

**Decision Log**
- Persist an explicit root-turn contract.

**Delegation Log If Any**
- None.

**Verification Report**
- Focused regression coverage passed.

**Final Milestone Judgment**
- fixed";
    let llm = RecordingLlm::ok(vec![
        tool_use_response(vec![ToolCall {
            id: "cmd-1".to_string(),
            name: "run_command".to_string(),
            arguments: serde_json::json!({"command":"git pull"}),
        }]),
        text_response("The repo is updated; I'll continue with the milestone."),
        text_response(final_response),
    ]);

    let result = engine
        .run_cycle(
            test_snapshot(
                "Switch to dev, pull, and continue the milestone.\n\nDeliverables:\n- Plan\n- Decision Log\n- Delegation Log If Any\n- Verification Report\n- Final Milestone Judgment",
            ),
            &llm,
        )
        .await
        .expect("run_cycle");

    assert_eq!(complete_response(result), final_response);
    let requests = llm.requests();
    assert_eq!(
        requests.len(),
        3,
        "the progress-only summary should not terminate the turn"
    );
    let system_prompt = requests[2].system_prompt.as_deref().expect("system prompt");
    assert!(system_prompt.contains("Root turn completion contract:"));
    assert!(system_prompt.contains("final-response phase"));
    assert!(system_prompt.contains("Plan"));
    assert!(system_prompt.contains("Final Milestone Judgment"));
    assert!(
        requests[2].tools.is_empty(),
        "root-contract retry must be final-response-only so stale tool commitments cannot loop"
    );
}

#[tokio::test]
#[ignore = "legacy harness behavior replaced by simple agent loop"]
async fn valid_tool_transaction_continues_beyond_outer_iteration_cap() {
    let mut engine = LoopEngine::builder()
        .budget(BudgetTracker::new(
            BudgetConfig::default(),
            current_time_ms(),
            0,
        ))
        .context(ContextCompactor::new(2048, 256))
        .max_iterations(1)
        .tool_executor(Arc::new(StubToolExecutor))
        .synthesis_instruction("Summarize tool output".to_string())
        .build()
        .expect("test engine build");

    let llm = RecordingLlm::ok(vec![
        tool_use_response(vec![ToolCall {
            id: "call-1".to_string(),
            name: "read_file".to_string(),
            arguments: serde_json::json!({"path":"README.md"}),
        }]),
        tool_use_response(vec![ToolCall {
            id: "call-2".to_string(),
            name: "read_file".to_string(),
            arguments: serde_json::json!({"path":"Cargo.toml"}),
        }]),
        text_response("final from committed evidence"),
    ]);

    let result = engine
        .run_cycle(test_snapshot("inspect the repo"), &llm)
        .await
        .expect("run_cycle");

    match result {
        LoopResult::Complete { response, .. } => {
            assert_eq!(response, "final from committed evidence");
        }
        other => panic!("expected complete result after valid follow-up tool, got {other:?}"),
    }
    let requests = llm.requests();
    assert_eq!(
        requests.len(),
        3,
        "expected initial tool call plus two tool-continuation requests"
    );
}

#[tokio::test]
#[ignore = "legacy harness behavior replaced by simple agent loop"]
async fn root_turn_contract_retry_cap_ends_incomplete_after_limit() {
    let mut engine = run_command_observation_engine(BudgetConfig::default());
    let incomplete_response = "Still working on it.";
    let llm = RecordingLlm::ok(vec![
        text_response(incomplete_response),
        text_response(incomplete_response),
        text_response(incomplete_response),
    ]);

    let result = engine
        .run_cycle(
            test_snapshot(
                "Continue the milestone.\n\nDeliverables:\n- Plan\n- Verification Report",
            ),
            &llm,
        )
        .await
        .expect("run_cycle");

    let reason = match result {
        LoopResult::Incomplete {
            partial_response,
            reason,
            signals,
            ..
        } => {
            assert_eq!(
                partial_response, None,
                "contract-incomplete responses must not be returned as successful final text"
            );
            assert!(
                signals.iter().any(|signal| {
                    signal.step == LoopStep::Act
                        && signal.kind == SignalKind::Friction
                        && signal.message
                            == "root turn completion retry cap reached; ending incomplete"
                        && signal.metadata["blocked_attempts"] == 2
                }),
                "the second blocked terminal attempt should end immediately at the default cap"
            );
            assert!(
                signals.iter().all(|signal| {
                    signal.message
                        != "blocked terminal completion until root turn deliverables are satisfied"
                        || signal.metadata["retries_remaining"] != 0
                }),
                "the gate must not schedule a continuation with zero retries remaining"
            );
            reason
        }
        other => panic!("expected incomplete result after retry cap, got {other:?}"),
    };
    assert!(reason.contains("Plan"));
    assert!(reason.contains("Verification Report"));
    let requests = llm.requests();
    assert_eq!(
        requests.len(),
        2,
        "the kernel should stop retrying after the contract retry cap is exhausted"
    );
}

#[tokio::test]
#[ignore = "legacy harness behavior replaced by simple agent loop"]
async fn mutation_contract_blocks_terminal_response_after_observation_only_round() {
    let mut engine = mixed_tool_engine(BudgetConfig::default());
    let unable_to_patch = "I can't resolve these in the current workspace because the relevant symbols aren't present.";
    let llm = RecordingLlm::ok(vec![
        tool_use_response(vec![ToolCall {
            id: "read-1".to_string(),
            name: "read_file".to_string(),
            arguments: serde_json::json!({"path":"README.md"}),
        }]),
        text_response(unable_to_patch),
        text_response(unable_to_patch),
    ]);

    let result = engine
        .run_cycle(
            test_snapshot(
                "please resolve these issues:\n\n1. discarded_field_note is too broad.\n2. ResultKind::Error regressed headless callers.",
            ),
            &llm,
        )
        .await
        .expect("run_cycle");

    match result {
        LoopResult::Incomplete {
            partial_response,
            reason,
            signals,
            ..
        } => {
            let partial_response = partial_response.expect("kernel-owned incomplete response");
            assert!(
                partial_response
                    .contains("local mutation work `Complete the requested code or file changes`"),
                "kernel-owned incomplete response should name the pending mutation work: {partial_response}"
            );
            assert!(
                reason.contains("Complete the requested code or file changes"),
                "incomplete reason should name the unsatisfied mutation work: {reason}"
            );
            assert!(
                signals.iter().any(|signal| {
                    signal.message
                        == "blocked terminal completion until root turn deliverables are satisfied"
                        && signal.metadata["pending_mutation_work"]
                            .as_array()
                            .is_some_and(|items| !items.is_empty())
                }),
                "the root-turn mutation contract should block final completion before retry cap"
            );
        }
        other => panic!("expected incomplete mutation contract result, got {other:?}"),
    }
}

#[tokio::test]
#[ignore = "legacy harness behavior replaced by simple agent loop"]
async fn explicit_deliverables_block_blocks_partial_terminal_response() {
    let mut engine = run_command_observation_engine(BudgetConfig::default());
    let partial_response = "**What you inspected**\n- `tui/src/app.rs`\n\n**Current architecture**\n- The UI is rendered from a single terminal app loop.";
    let final_response = "**What you inspected**\n- `tui/src/app.rs`\n- `tui/src/render.rs`\n\n**Current architecture**\n- The UI is rendered from a terminal app loop with transcript rendering delegated to helper functions.\n\n**3 concrete recommendations**\n- Make transcript phases explicit.\n- Keep tool chunks collapsed by default.\n- Add reducer-level ordering tests.";
    let llm = RecordingLlm::ok(vec![
        tool_use_response(vec![ToolCall {
            id: "cmd-1".to_string(),
            name: "run_command".to_string(),
            arguments: serde_json::json!({"command":"rg -n transcript tui/src"}),
        }]),
        text_response(partial_response),
        text_response(final_response),
    ]);

    let result = engine
        .run_cycle(
            test_snapshot(
                "Please perform a small real task that requires several tool calls and narrate your work.\n\nDeliverables:\n- What you inspected\n- Current architecture\n- 3 concrete recommendations",
            ),
            &llm,
        )
        .await
        .expect("run_cycle");

    assert_eq!(complete_response(result), final_response);
    let requests = llm.requests();
    assert_eq!(
        requests.len(),
        3,
        "the partial terminal answer must be retried until the explicit Deliverables contract is satisfied"
    );
    assert!(
        requests[2].tools.is_empty(),
        "deliverables-contract retry must be final-response-only"
    );
    let system_prompt = requests[2].system_prompt.as_deref().expect("system prompt");
    assert!(system_prompt.contains("final-response phase"));
}

#[tokio::test]
#[ignore = "legacy harness behavior replaced by simple agent loop"]
async fn explicit_deliverables_block_blocks_raw_evidence_dump_terminal_response() {
    let mut engine = run_command_observation_engine(BudgetConfig::default());
    let raw_dump = "```rust\nfn render_entry(entry: Entry) {\n    render_tool(entry);\n}\n```\n\nThis appears to be the core render path.";
    let final_response = "**What you inspected**\n- `tui/src/app.rs`\n\n**Current architecture**\n- Transcript entries are rendered from a terminal event loop.\n\n**3 concrete recommendations**\n- Separate live narration from final answer text.\n- Persist completed activity chunks as typed transcript nodes.\n- Test collapsed tool-call rendering end to end.";
    let llm = RecordingLlm::ok(vec![
        tool_use_response(vec![ToolCall {
            id: "cmd-1".to_string(),
            name: "run_command".to_string(),
            arguments: serde_json::json!({"command":"sed -n '1000,1100p' tui/src/app.rs"}),
        }]),
        text_response(raw_dump),
        text_response(final_response),
    ]);

    let result = engine
        .run_cycle(
            test_snapshot(
                "Please perform a small real task that requires several tool calls and narrate your work.\n\nDeliverables:\n- What you inspected\n- Current architecture\n- 3 concrete recommendations",
            ),
            &llm,
        )
        .await
        .expect("run_cycle");

    assert_eq!(complete_response(result), final_response);
    assert_eq!(
        llm.requests().len(),
        3,
        "a raw evidence/code dump must not satisfy the explicit Deliverables contract"
    );
}

#[tokio::test]
#[ignore = "legacy harness behavior replaced by simple agent loop"]
async fn root_turn_contract_retry_limit_uses_budget_config() {
    let config = BudgetConfig {
        termination: TerminationConfig {
            root_turn_completion_retry_limit: 1,
            ..TerminationConfig::default()
        },
        ..BudgetConfig::default()
    };
    let mut engine = run_command_observation_engine(config);
    let incomplete_response = "Still working on it.";
    let llm = RecordingLlm::ok(vec![
        text_response(incomplete_response),
        text_response(incomplete_response),
    ]);

    let result = engine
        .run_cycle(
            test_snapshot(
                "Continue the milestone.\n\nDeliverables:\n- Plan\n- Verification Report",
            ),
            &llm,
        )
        .await
        .expect("run_cycle");

    let reason = match result {
        LoopResult::Incomplete {
            partial_response,
            reason,
            ..
        } => {
            assert_eq!(
                partial_response, None,
                "contract-incomplete responses must not be returned as successful final text"
            );
            reason
        }
        other => panic!("expected incomplete result after configured retry cap, got {other:?}"),
    };
    assert!(reason.contains("Plan"));
    assert!(reason.contains("Verification Report"));
    assert_eq!(
        llm.requests().len(),
        1,
        "a configured retry limit of 1 should end at the first incomplete terminal response"
    );
}

#[tokio::test]
async fn multiple_deliverables_blocks_emit_friction_signal_and_use_first_block_only() {
    let mut engine = mixed_tool_engine(BudgetConfig::default());

    let _ = engine
        .perceive(&test_snapshot(
            "Deliverables:\n- Plan\n\nDeliverables:\n- Verification Report",
        ))
        .await
        .expect("perceive");

    let contract = engine
        .root_turn_contract
        .as_ref()
        .expect("root turn contract");
    let response_sections = contract
        .deliverables
        .iter()
        .filter_map(|deliverable| match deliverable {
            RootTurnDeliverable::ResponseSection { label, .. } => Some(label.as_str()),
            RootTurnDeliverable::MutationWork { .. } => None,
            RootTurnDeliverable::ArtifactWrite { .. } => None,
            RootTurnDeliverable::ExternalAction { .. } => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(response_sections, vec!["Plan"]);

    let signals = engine.signals.drain_all();
    let multiple_blocks = signals
        .iter()
        .find(|signal| {
            signal.step == LoopStep::Perceive
                && signal.kind == SignalKind::Friction
                && signal
                    .message
                    .contains("multiple Deliverables blocks detected")
        })
        .expect("multiple deliverables blocks signal");
    assert_eq!(
        multiple_blocks.metadata["deliverable_block_count"]
            .as_u64()
            .expect("block count"),
        2
    );
}

#[test]
fn recent_write_verb_matcher_ignores_substring_hits() {
    assert!(prefix_context_contains_recent_write_verb(
        "please write a short note"
    ));
    assert!(prefix_context_contains_recent_write_verb(
        "please save the note"
    ));
    assert!(!prefix_context_contains_recent_write_verb(
        "please rewrite the note"
    ));
    assert!(!prefix_context_contains_recent_write_verb("this is unsafe"));
}

#[test]
fn extract_requested_write_target_requires_a_standalone_write_verb() {
    assert_eq!(
        extract_requested_write_target("Write a short note to ~/.fawx/test-note.md"),
        Some("~/.fawx/test-note.md".to_string())
    );
    assert_eq!(
        extract_requested_write_target("This is unsafe to ~/.fawx/test-note.md"),
        None
    );
}

#[test]
fn root_turn_contract_extracts_pr_comment_external_action() {
    let contract = extract_root_turn_contract("Review PR 1842 and post findings in a comment.")
        .contract
        .expect("root turn contract");

    assert!(contract.deliverables.iter().any(|deliverable| {
        matches!(
            deliverable,
            RootTurnDeliverable::ExternalAction {
                kind: RootTurnExternalActionKind::GitHubPrComment,
                label,
                satisfied: false,
            } if label == "Post a comment on the GitHub pull request"
        )
    }));
}

#[test]
fn root_turn_contract_progress_marks_successful_pr_comment_command() {
    let mut engine = mixed_tool_engine(BudgetConfig::default());
    engine.root_turn_contract = Some(RootTurnContract {
        deliverables: vec![RootTurnDeliverable::ExternalAction {
            kind: RootTurnExternalActionKind::GitHubPrComment,
            label: external_action_label(RootTurnExternalActionKind::GitHubPrComment).to_string(),
            satisfied: false,
        }],
        blocked_terminal_attempts: 0,
    });

    let calls = vec![ToolCall {
        id: "comment-1".to_string(),
        name: "run_command".to_string(),
        arguments: serde_json::json!({
            "command": "gh pr comment 1842 --body-file /tmp/review.md"
        }),
    }];
    let results = vec![ToolResult {
        tool_call_id: "comment-1".to_string(),
        tool_name: "run_command".to_string(),
        success: true,
        output: "https://github.com/fawxai/fawx/pull/1842#issuecomment-1".to_string(),
        failure_class: None,
    }];

    engine.record_root_turn_contract_progress(&calls, &results);

    let contract = engine
        .root_turn_contract
        .as_ref()
        .expect("root turn contract");
    assert!(contract.deliverables.iter().any(|deliverable| {
        matches!(
            deliverable,
            RootTurnDeliverable::ExternalAction {
                kind: RootTurnExternalActionKind::GitHubPrComment,
                satisfied: true,
                ..
            }
        )
    }));
}

#[test]
fn root_turn_contract_progress_trusts_successful_call_id_pairing_for_pr_comment() {
    let mut engine = mixed_tool_engine(BudgetConfig::default());
    engine.root_turn_contract = Some(RootTurnContract {
        deliverables: vec![RootTurnDeliverable::ExternalAction {
            kind: RootTurnExternalActionKind::GitHubPrComment,
            label: external_action_label(RootTurnExternalActionKind::GitHubPrComment).to_string(),
            satisfied: false,
        }],
        blocked_terminal_attempts: 0,
    });

    let calls = vec![ToolCall {
        id: "comment-1".to_string(),
        name: "run_command".to_string(),
        arguments: serde_json::json!({
            "command": "gh pr comment 1842 --body-file /tmp/review.md"
        }),
    }];
    let results = vec![ToolResult {
        tool_call_id: "comment-1".to_string(),
        // Some provider/tool bridges preserve the call id but return the
        // adapter name instead of echoing the requested tool name. The call id
        // is the durable pairing contract.
        tool_name: "shell".to_string(),
        success: true,
        output: "https://github.com/fawxai/fawx/pull/1842#issuecomment-1".to_string(),
        failure_class: None,
    }];

    engine.record_root_turn_contract_progress(&calls, &results);

    let contract = engine
        .root_turn_contract
        .as_ref()
        .expect("root turn contract");
    assert!(contract.deliverables.iter().any(|deliverable| {
        matches!(
            deliverable,
            RootTurnDeliverable::ExternalAction {
                kind: RootTurnExternalActionKind::GitHubPrComment,
                satisfied: true,
                ..
            }
        )
    }));
}

#[test]
fn root_turn_contract_progress_marks_successful_pr_comment_argv_command() {
    let mut engine = mixed_tool_engine(BudgetConfig::default());
    engine.root_turn_contract = Some(RootTurnContract {
        deliverables: vec![RootTurnDeliverable::ExternalAction {
            kind: RootTurnExternalActionKind::GitHubPrComment,
            label: external_action_label(RootTurnExternalActionKind::GitHubPrComment).to_string(),
            satisfied: false,
        }],
        blocked_terminal_attempts: 0,
    });

    let calls = vec![ToolCall {
        id: "comment-1".to_string(),
        name: "run_command".to_string(),
        arguments: serde_json::json!({
            "argv": [
                "gh",
                "pr",
                "comment",
                "1858",
                "--repo",
                "fawxai/fawx",
                "--body",
                "Review posted"
            ]
        }),
    }];
    let results = vec![ToolResult {
        tool_call_id: "comment-1".to_string(),
        tool_name: "run_command".to_string(),
        success: true,
        output: "https://github.com/fawxai/fawx/pull/1858#issuecomment-1".to_string(),
        failure_class: None,
    }];

    engine.record_root_turn_contract_progress(&calls, &results);

    let contract = engine
        .root_turn_contract
        .as_ref()
        .expect("root turn contract");
    assert!(contract.deliverables.iter().any(|deliverable| {
        matches!(
            deliverable,
            RootTurnDeliverable::ExternalAction {
                kind: RootTurnExternalActionKind::GitHubPrComment,
                satisfied: true,
                ..
            }
        )
    }));
}

#[test]
fn root_turn_contract_progress_prefers_typed_external_action_evidence() {
    let mut engine = mixed_tool_engine(BudgetConfig::default());
    engine.root_turn_contract = Some(RootTurnContract {
        deliverables: vec![RootTurnDeliverable::ExternalAction {
            kind: RootTurnExternalActionKind::GitHubPrComment,
            label: external_action_label(RootTurnExternalActionKind::GitHubPrComment).to_string(),
            satisfied: false,
        }],
        blocked_terminal_attempts: 0,
    });

    let calls = vec![ToolCall {
        id: "comment-1".to_string(),
        name: "run_command".to_string(),
        arguments: serde_json::json!({
            "command": "custom-github-commenter --pr 1842 --body /tmp/review.md"
        }),
    }];
    let results = vec![ToolResult {
        tool_call_id: "comment-1".to_string(),
        tool_name: "run_command".to_string(),
        success: true,
        output: "comment posted".to_string(),
        failure_class: None,
    }];
    engine.pending_tool_result_diagnostics.insert(
        "comment-1".to_string(),
        ToolExecutionDiagnostics::RunCommand(RunCommandDiagnostics {
            exit_code: Some(0),
            stderr_snippet: None,
            duration_ms: 12,
            shell: true,
            timed_out: false,
            external_actions: vec![ExternalActionEvidence::github_pr_comment(Some(
                "https://github.com/fawxai/fawx/pull/1842#issuecomment-1".to_string(),
            ))],
        }),
    );

    engine.record_root_turn_contract_progress(&calls, &results);

    let contract = engine
        .root_turn_contract
        .as_ref()
        .expect("root turn contract");
    assert!(contract.deliverables.iter().any(|deliverable| {
        matches!(
            deliverable,
            RootTurnDeliverable::ExternalAction {
                kind: RootTurnExternalActionKind::GitHubPrComment,
                satisfied: true,
                ..
            }
        )
    }));
}

#[test]
fn root_turn_contract_progress_accepts_structured_skill_external_action_evidence() {
    let mut engine = mixed_tool_engine(BudgetConfig::default());
    engine.root_turn_contract = Some(RootTurnContract {
        deliverables: vec![RootTurnDeliverable::ExternalAction {
            kind: RootTurnExternalActionKind::GitHubPrComment,
            label: external_action_label(RootTurnExternalActionKind::GitHubPrComment).to_string(),
            satisfied: false,
        }],
        blocked_terminal_attempts: 0,
    });

    let calls = vec![ToolCall {
        id: "comment-1".to_string(),
        name: "comment_pr".to_string(),
        arguments: serde_json::json!({
            "owner": "abbudjoe",
            "repo": "fawx",
            "pr_number": 1842,
            "body": "Review posted"
        }),
    }];
    let results = vec![ToolResult {
        tool_call_id: "comment-1".to_string(),
        tool_name: "comment_pr".to_string(),
        success: true,
        output: r#"{"success":true,"comment_id":12345}"#.to_string(),
        failure_class: None,
    }];
    engine.pending_tool_result_diagnostics.insert(
        "comment-1".to_string(),
        ToolExecutionDiagnostics::Tool(ToolDiagnostics {
            external_actions: vec![ExternalActionEvidence::github_pr_comment(Some(
                "https://github.com/fawxai/fawx/pull/1842#issuecomment-12345".to_string(),
            ))],
        }),
    );

    engine.record_root_turn_contract_progress(&calls, &results);

    let contract = engine
        .root_turn_contract
        .as_ref()
        .expect("root turn contract");
    assert!(contract.deliverables.iter().any(|deliverable| {
        matches!(
            deliverable,
            RootTurnDeliverable::ExternalAction {
                kind: RootTurnExternalActionKind::GitHubPrComment,
                satisfied: true,
                ..
            }
        )
    }));
}

#[test]
fn root_turn_contract_progress_accepts_successful_typed_comment_tool_without_diagnostics() {
    let mut engine = mixed_tool_engine(BudgetConfig::default());
    engine.root_turn_contract = Some(RootTurnContract {
        deliverables: vec![RootTurnDeliverable::ExternalAction {
            kind: RootTurnExternalActionKind::GitHubPrComment,
            label: external_action_label(RootTurnExternalActionKind::GitHubPrComment).to_string(),
            satisfied: false,
        }],
        blocked_terminal_attempts: 0,
    });

    let calls = vec![ToolCall {
        id: "comment-1".to_string(),
        name: "comment_pr".to_string(),
        arguments: serde_json::json!({
            "owner": "abbudjoe",
            "repo": "fawx",
            "pr_number": 1842,
            "body": "Review posted"
        }),
    }];
    let results = vec![ToolResult {
        tool_call_id: "comment-1".to_string(),
        tool_name: "comment_pr".to_string(),
        success: true,
        output: r#"{"success":true,"comment_id":12345}"#.to_string(),
        failure_class: None,
    }];

    engine.record_root_turn_contract_progress(&calls, &results);

    let contract = engine
        .root_turn_contract
        .as_ref()
        .expect("root turn contract");
    assert!(contract.deliverables.iter().any(|deliverable| {
        matches!(
            deliverable,
            RootTurnDeliverable::ExternalAction {
                kind: RootTurnExternalActionKind::GitHubPrComment,
                satisfied: true,
                ..
            }
        )
    }));
}

#[test]
fn root_turn_contract_progress_ignores_typed_external_action_on_failed_result() {
    let mut engine = mixed_tool_engine(BudgetConfig::default());
    engine.root_turn_contract = Some(RootTurnContract {
        deliverables: vec![RootTurnDeliverable::ExternalAction {
            kind: RootTurnExternalActionKind::GitHubPrComment,
            label: external_action_label(RootTurnExternalActionKind::GitHubPrComment).to_string(),
            satisfied: false,
        }],
        blocked_terminal_attempts: 0,
    });

    let calls = vec![ToolCall {
        id: "comment-1".to_string(),
        name: "run_command".to_string(),
        arguments: serde_json::json!({
            "command": "custom-github-commenter --pr 1842 --body /tmp/review.md"
        }),
    }];
    let results = vec![ToolResult {
        tool_call_id: "comment-1".to_string(),
        tool_name: "run_command".to_string(),
        success: false,
        output: "comment failed".to_string(),
        failure_class: Some(FailureClass::Unknown),
    }];
    engine.pending_tool_result_diagnostics.insert(
        "comment-1".to_string(),
        ToolExecutionDiagnostics::RunCommand(RunCommandDiagnostics {
            exit_code: Some(1),
            stderr_snippet: Some("failed".to_string()),
            duration_ms: 12,
            shell: true,
            timed_out: false,
            external_actions: vec![ExternalActionEvidence::github_pr_comment(Some(
                "https://github.com/fawxai/fawx/pull/1842#issuecomment-1".to_string(),
            ))],
        }),
    );

    engine.record_root_turn_contract_progress(&calls, &results);

    let contract = engine
        .root_turn_contract
        .as_ref()
        .expect("root turn contract");
    assert!(contract.deliverables.iter().any(|deliverable| {
        matches!(
            deliverable,
            RootTurnDeliverable::ExternalAction {
                kind: RootTurnExternalActionKind::GitHubPrComment,
                satisfied: false,
                ..
            }
        )
    }));
}

#[test]
fn github_pr_comment_detection_accepts_common_gh_comment_forms() {
    assert!(command_posts_github_pr_comment(
        "gh pr comment 1847 --body-file /tmp/review.md"
    ));
    assert!(command_posts_github_pr_comment(
        "gh pr review 1847 --comment --body-file /tmp/review.md"
    ));
    assert!(command_posts_github_pr_comment(
        "gh issue comment 1847 --body-file /tmp/review.md"
    ));
    assert!(command_posts_github_pr_comment(
        "gh api repos/fawxai/fawx/issues/1847/comments -f body=@/tmp/review.md"
    ));
    assert!(command_posts_github_pr_comment(
        "gh api repos/fawxai/fawx/issues/1847/comments --method POST --field body=@/tmp/review.md"
    ));
    assert!(command_posts_github_pr_comment_with_output(
        "gh api graphql -f query='mutation AddComment($subjectId:ID!,$body:String!){addComment(input:{subjectId:$subjectId,body:$body}){commentEdge{node{url}}}}' -f subjectId=PR_kwDO -f body=@/tmp/review.md",
        "https://github.com/fawxai/fawx/pull/1847#issuecomment-1"
    ));
    assert!(!command_posts_github_pr_comment(
        "gh pr view 1847 --comments"
    ));
    assert!(!command_posts_github_pr_comment(
        "gh api repos/fawxai/fawx/issues/1847/comments"
    ));
    assert!(!command_posts_github_pr_comment_with_output(
        "gh pr view 1847 --comments",
        "https://github.com/fawxai/fawx/pull/1847#issuecomment-1"
    ));
}

#[tokio::test]
#[ignore = "legacy harness behavior replaced by simple agent loop"]
async fn pending_external_action_retry_cap_surfaces_visible_partial_response() {
    let config = BudgetConfig {
        termination: TerminationConfig {
            root_turn_completion_retry_limit: 1,
            ..TerminationConfig::default()
        },
        ..BudgetConfig::default()
    };
    let mut engine = run_command_observation_engine(config);
    let llm = RecordingLlm::ok(vec![text_response(
        "I reviewed the PR but did not post the comment.",
    )]);

    let result = engine
        .run_cycle(
            test_snapshot("Review PR 1847 and post findings in a comment."),
            &llm,
        )
        .await
        .expect("run_cycle");

    match result {
        LoopResult::Incomplete {
            partial_response,
            reason,
            ..
        } => {
            let partial_response = partial_response.expect("visible incomplete response");
            assert!(partial_response.contains(
                "I could not complete this turn because required root-turn deliverable(s) are still pending"
            ));
            assert!(partial_response
                .contains("external action `Post a comment on the GitHub pull request`"));
            assert!(partial_response.contains(
                "Model response before the turn ended:\nI reviewed the PR but did not post the comment."
            ));
            assert!(reason.contains("pending external actions"));
        }
        other => panic!("expected incomplete result after retry cap, got {other:?}"),
    }
}

#[test]
fn root_turn_contract_incomplete_response_without_candidate_is_kernel_owned_note() {
    let block = RootTurnCompletionBlock {
        missing_response_sections: Vec::new(),
        pending_mutation_work: Vec::new(),
        pending_artifact_paths: Vec::new(),
        pending_external_actions: vec!["Open the GitHub pull request".to_string()],
    };

    let response = LoopEngine::root_turn_contract_incomplete_response(&block, None)
        .expect("kernel-owned incomplete response");

    assert_eq!(
        response,
        "I could not complete this turn because required root-turn deliverable(s) are still pending: external action `Open the GitHub pull request`."
    );
    assert!(!response.contains("Model response before the turn ended"));
}

#[tokio::test]
#[ignore = "legacy harness behavior replaced by simple agent loop"]
async fn requested_pr_comment_blocks_terminal_response_until_comment_is_posted() {
    let mut engine = run_command_observation_engine(BudgetConfig::default());
    let final_response = "Reviewed PR 1842 and posted the findings as a PR comment.";
    let llm = RecordingLlm::ok(vec![
        text_response("I reviewed the PR but did not post a comment."),
        tool_use_response(vec![ToolCall {
            id: "comment-1".to_string(),
            name: "run_command".to_string(),
            arguments: serde_json::json!({
                "command": "gh pr comment 1842 --body-file /tmp/review.md"
            }),
        }]),
        text_response(final_response),
    ]);

    let result = engine
        .run_cycle(
            test_snapshot("Review PR 1842 and post findings in a comment."),
            &llm,
        )
        .await
        .expect("run_cycle");

    assert_eq!(complete_response(result), final_response);
    let requests = llm.requests();
    assert_eq!(
        requests.len(),
        3,
        "the turn should continue until the requested PR comment side effect is completed"
    );
    assert!(requests[1]
        .tools
        .iter()
        .any(|tool| tool.name == "run_command"));
    assert!(requests[1]
        .system_prompt
        .as_deref()
        .expect("system prompt")
        .contains("Post a comment on the GitHub pull request"));
}

#[test]
fn root_turn_contract_progress_handles_partial_multi_artifact_writes() {
    let mut engine = mixed_tool_engine(BudgetConfig::default());
    engine.root_turn_contract = Some(RootTurnContract {
        deliverables: vec![
            RootTurnDeliverable::ArtifactWrite {
                path: "~/.fawx/first.md".to_string(),
                satisfied: false,
            },
            RootTurnDeliverable::ArtifactWrite {
                path: "~/.fawx/second.md".to_string(),
                satisfied: false,
            },
        ],
        blocked_terminal_attempts: 0,
    });

    engine.record_root_turn_contract_progress(
        &[],
        &[
            ToolResult {
                tool_call_id: "write-1".to_string(),
                tool_name: "write_file".to_string(),
                success: true,
                output: "wrote 14 bytes to ~/.fawx/first.md".to_string(),
                failure_class: None,
            },
            ToolResult {
                tool_call_id: "write-2".to_string(),
                tool_name: "write_file".to_string(),
                success: true,
                output: "wrote 7 bytes to ./unrelated.txt".to_string(),
                failure_class: None,
            },
        ],
    );

    let contract = engine
        .root_turn_contract
        .as_ref()
        .expect("root turn contract");
    assert_eq!(
        contract.deliverables,
        vec![
            RootTurnDeliverable::ArtifactWrite {
                path: "~/.fawx/first.md".to_string(),
                satisfied: true,
            },
            RootTurnDeliverable::ArtifactWrite {
                path: "~/.fawx/second.md".to_string(),
                satisfied: false,
            },
        ]
    );
}

#[tokio::test]
#[ignore = "legacy harness behavior replaced by simple agent loop"]
async fn requested_artifact_write_blocks_terminal_response_until_file_is_written() {
    #[derive(Debug, Default)]
    struct ArtifactAwareWriteExecutor;

    #[async_trait]
    impl ToolExecutor for ArtifactAwareWriteExecutor {
        async fn execute_tools(
            &self,
            calls: &[ToolCall],
            _cancel: Option<&CancellationToken>,
        ) -> Result<Vec<ToolResult>, crate::act::ToolExecutorError> {
            Ok(calls
                .iter()
                .map(|call| {
                    let output = match call.name.as_str() {
                        "write_file" => {
                            let path = call
                                .arguments
                                .get("path")
                                .and_then(serde_json::Value::as_str)
                                .expect("write_file path");
                            format!("wrote 14 bytes to {path}")
                        }
                        other => format!("unsupported tool: {other}"),
                    };
                    ToolResult {
                        tool_call_id: call.id.clone(),
                        tool_name: call.name.clone(),
                        success: true,
                        output,
                        failure_class: None,
                    }
                })
                .collect())
        }

        fn tool_definitions(&self) -> Vec<ToolDefinition> {
            vec![ToolDefinition {
                name: "write_file".to_string(),
                description: "Write a file".to_string(),
                parameters: serde_json::json!({"type":"object"}),
            }]
        }

        fn cacheability(&self, tool_name: &str) -> crate::act::ToolCacheability {
            match tool_name {
                "write_file" => crate::act::ToolCacheability::SideEffect,
                _ => crate::act::ToolCacheability::NeverCache,
            }
        }
    }

    let mut engine = mixed_tool_engine_with_executor(
        BudgetConfig::default(),
        Arc::new(ArtifactAwareWriteExecutor),
    );
    let llm = RecordingLlm::ok(vec![
        text_response("I'll write the note next."),
        tool_use_response(vec![ToolCall {
            id: "write-1".to_string(),
            name: "write_file".to_string(),
            arguments: serde_json::json!({
                "path":"~/.fawx/test-note.md",
                "content":"artifact note",
            }),
        }]),
        text_response("Wrote the note."),
        text_response("Wrote the note."),
    ]);

    let result = engine
        .run_cycle(
            test_snapshot("Write a short note to ~/.fawx/test-note.md and tell me when it's done."),
            &llm,
        )
        .await
        .expect("run_cycle");

    assert_eq!(complete_response(result), "Wrote the note.");
    let requests = llm.requests();
    assert_eq!(
        requests.len(),
        3,
        "the turn should continue until the requested artifact is written, then finish without a synthetic extra reasoning pass"
    );
    assert!(requests[1]
        .system_prompt
        .as_deref()
        .expect("system prompt")
        .contains("~/.fawx/test-note.md"));
}

#[tokio::test]
async fn read_only_follow_up_uses_structured_tool_evidence_without_root_detour() {
    let baseline = "README intro\nACTUAL FINAL LINE";
    let executor = Arc::new(StatefulReadWriteExecutor::new(baseline));
    let mut engine = stateful_mixed_tool_engine(executor.clone());
    let llm = ReadEvidenceLlm::new(baseline);

    let result = engine
        .run_cycle(
            test_snapshot("Read README.md again and tell me the current final line."),
            &llm,
        )
        .await
        .expect("run_cycle");

    let response = complete_response(result);
    assert_eq!(response, "ACTUAL FINAL LINE");
    assert_eq!(executor.readme_contents(), baseline);
}

#[tokio::test]
async fn append_follow_up_uses_actual_file_body_in_tool_continuation() {
    let baseline = "README intro\nACTUAL FINAL LINE";
    let verification = "[verification] appended in place";
    let executor = Arc::new(StatefulReadWriteExecutor::new(baseline));
    let mut engine = stateful_mixed_tool_engine(executor.clone());
    let llm = AppendEvidenceLlm::new(baseline, verification);

    let result = engine
            .run_cycle(
                test_snapshot(
                    "Read README.md, append one clearly marked verification line to it, then tell me exactly what changed.",
                ),
                &llm,
            )
            .await
            .expect("run_cycle");

    let response = complete_response(result);
    assert_eq!(response, "Appended the verification line.");
    assert_eq!(
        executor.readme_contents(),
        format!("{baseline}\n{verification}")
    );
}

#[tokio::test]
#[ignore = "legacy harness behavior replaced by simple agent loop"]
async fn mixed_observation_and_mutation_batch_defers_mutation_until_review() {
    let baseline = "README intro\nACTUAL FINAL LINE";
    let verification = "[verification] appended after review";
    let executor = Arc::new(StatefulReadWriteExecutor::new(baseline));
    let mut engine = stateful_mixed_tool_engine(executor.clone());
    let llm = MixedBatchMutationBarrierLlm::new(baseline, verification);

    let result = engine
        .run_cycle(
            test_snapshot(
                "Read README.md and then update it, but only after reviewing what you found.",
            ),
            &llm,
        )
        .await
        .expect("run_cycle");

    assert_eq!(
        complete_response(result),
        "Applied the write after reviewing the file."
    );
    assert_eq!(
        executor.readme_contents(),
        format!("{baseline}\n{verification}")
    );
}

#[tokio::test]
#[ignore = "legacy harness behavior replaced by simple agent loop"]
async fn mutation_batches_execute_one_call_per_round() {
    let baseline = "README intro\nACTUAL FINAL LINE";
    let executor = Arc::new(StatefulReadWriteExecutor::new(baseline));
    let mut engine = stateful_mixed_tool_engine(executor.clone());
    let llm = SequentialMutationBarrierLlm::new();

    let result = engine
        .run_cycle(
            test_snapshot("Apply two sequential writes to README.md."),
            &llm,
        )
        .await
        .expect("run_cycle");

    assert_eq!(complete_response(result), "Applied both writes safely.");
    assert_eq!(executor.readme_contents(), "second write");
}

#[tokio::test]
async fn mutation_first_mixed_batch_preserves_original_order() {
    let executor = Arc::new(StatefulReadWriteExecutor::new(
        "README intro\nACTUAL FINAL LINE",
    ));
    let mut engine = stateful_mixed_tool_engine(executor.clone());
    let decision = Decision::UseTools(vec![
        ToolCall {
            id: "write-1".to_string(),
            name: "write_file".to_string(),
            arguments: serde_json::json!({"path":"README.md","content":"first write"}),
        },
        ToolCall {
            id: "read-1".to_string(),
            name: "read_file".to_string(),
            arguments: serde_json::json!({"path":"README.md"}),
        },
    ]);
    let llm = RecordingLlm::ok(vec![
        tool_use_response(vec![ToolCall {
            id: "read-2".to_string(),
            name: "read_file".to_string(),
            arguments: serde_json::json!({"path":"README.md"}),
        }]),
        text_response("done after ordered read"),
    ]);

    // Use `act()` directly so we can assert the barrier's intermediate continuation requests.
    let action = engine
        .act(
            &decision,
            &llm,
            &[Message::user(
                "Update README.md, then verify the final contents.",
            )],
            CycleStream::disabled(),
        )
        .await
        .expect("act should succeed");

    assert_eq!(action.response_text, "done after ordered read");
    assert_eq!(executor.readme_contents(), "first write");

    let requests = llm.requests();
    assert_eq!(
        requests.len(),
        2,
        "expected one follow-up read and one final synthesis"
    );
    assert_eq!(
        request_tool_use_names(&requests[0]),
        vec!["write_file".to_string()],
        "the trailing read should not execute before the leading mutation"
    );
    assert!(
        request_contains_text(&requests[0], "Calls after the first mutation were deferred"),
        "the continuation should explain why later calls were deferred"
    );
    assert!(
        request_contains_tool_result_text(&requests[1], "first write"),
        "the follow-up read should observe the post-mutation contents"
    );
}

#[tokio::test]
async fn deferred_tool_guardrail_continues_when_final_response_missing() {
    let executor = Arc::new(StatefulReadWriteExecutor::new("README intro"));
    let mut engine = stateful_mixed_tool_engine(executor.clone());
    let decision = Decision::UseTools(vec![
        ToolCall {
            id: "write-1".to_string(),
            name: "write_file".to_string(),
            arguments: serde_json::json!({"path":"README.md","content":"first write"}),
        },
        ToolCall {
            id: "memory-1".to_string(),
            name: "memory_search".to_string(),
            arguments: serde_json::json!({"query":"prior context"}),
        },
    ]);
    let llm = RecordingLlm::ok(vec![CompletionResponse {
        content: Vec::new(),
        tool_calls: Vec::new(),
        usage: None,
        stop_reason: None,
    }]);

    let action = engine
        .act(
            &decision,
            &llm,
            &[Message::user(
                "Update README.md, then check memory before answering.",
            )],
            CycleStream::disabled(),
        )
        .await
        .expect("act should succeed");

    let continuation = match &action.next_step {
        ActionNextStep::Continue(continuation) => continuation,
        other => panic!("expected continuation after policy-deferred work, got {other:?}"),
    };
    assert!(
        continuation.partial_response.is_none(),
        "policy-deferred work should not invent a user-visible partial response: {:?}",
        continuation.partial_response
    );
    for banned in ["latest blocker", "follow-up tool work was paused"] {
        assert!(
            !action.response_text.contains(banned),
            "policy-deferred control state leaked into user-visible response text `{banned}`: {}",
            action.response_text
        );
    }
    assert!(
        continuation.context_messages.iter().any(|message| {
            message.content.iter().any(|block| match block {
                ContentBlock::Text { text } => {
                    text.contains("Calls after the first mutation were deferred")
                }
                _ => false,
            })
        }),
        "model-facing continuation should preserve the typed guardrail context"
    );

    let deferred_result = action
        .tool_results
        .iter()
        .find(|result| result.tool_name == "memory_search")
        .expect("deferred memory_search result should be preserved");
    assert_eq!(
        deferred_result.failure_class,
        Some(FailureClass::PolicyDeferred),
        "deferred calls should carry typed policy state"
    );
    assert!(
        deferred_result
            .output
            .contains("Calls after the first mutation were deferred"),
        "internal deferred context should remain available to follow-up reasoning"
    );

    let requests = llm.requests();
    assert_eq!(requests.len(), 1, "expected one empty continuation request");
    assert!(
        request_contains_text(&requests[0], "Calls after the first mutation were deferred"),
        "the model-facing continuation should still receive the deferred guardrail"
    );
}

#[tokio::test]
async fn deferred_tool_guardrail_does_not_expose_tool_continuation_text_as_partial() {
    let executor = Arc::new(StatefulReadWriteExecutor::new("README intro"));
    let mut engine = stateful_mixed_tool_engine(executor);
    let decision = Decision::UseTools(vec![
        ToolCall {
            id: "write-1".to_string(),
            name: "write_file".to_string(),
            arguments: serde_json::json!({"path":"README.md","content":"updated"}),
        },
        ToolCall {
            id: "memory-1".to_string(),
            name: "memory_search".to_string(),
            arguments: serde_json::json!({"query":"context"}),
        },
    ]);
    let llm = RecordingLlm::with_generated_summary(
        vec![Ok(text_response(
            "completed tool work: write_file. Some follow-up tool work was paused.",
        ))],
        "synthesis should not be used".to_string(),
    );

    let action = engine
        .act(
            &decision,
            &llm,
            &[Message::user("update README, then check memory")],
            CycleStream::disabled(),
        )
        .await
        .expect("act should succeed");

    assert!(
        action.response_text.is_empty(),
        "policy-deferred tool state must not become response text: {}",
        action.response_text
    );
    let continuation = match action.next_step {
        ActionNextStep::Continue(continuation) => continuation,
        other => panic!("expected policy-deferred continuation, got {other:?}"),
    };
    assert_eq!(
        continuation.partial_response, None,
        "tool continuation text must not become user-visible partial output"
    );
    assert!(
        continuation.context_messages.iter().any(|message| {
            message.content.iter().any(|block| match block {
                ContentBlock::Text { text } => {
                    text.contains("Calls after the first mutation were deferred")
                }
                _ => false,
            })
        }),
        "model-facing continuation should still receive the deferral contract"
    );
}

#[tokio::test]
#[ignore = "legacy harness behavior replaced by simple agent loop"]
async fn policy_deferred_empty_continuation_reasons_again_instead_of_finishing_with_progress() {
    let executor = Arc::new(StatefulReadWriteExecutor::new("README intro"));
    let mut engine = stateful_mixed_tool_engine(executor.clone());
    let llm = RecordingLlm::ok(vec![
        tool_use_response(vec![
            ToolCall {
                id: "write-1".to_string(),
                name: "write_file".to_string(),
                arguments: serde_json::json!({"path":"README.md","content":"first write"}),
            },
            ToolCall {
                id: "memory-1".to_string(),
                name: "memory_search".to_string(),
                arguments: serde_json::json!({"query":"prior context"}),
            },
        ]),
        CompletionResponse {
            content: Vec::new(),
            tool_calls: Vec::new(),
            usage: None,
            stop_reason: None,
        },
        text_response("Finished after continuing from the deferred guardrail."),
    ]);

    let result = engine
        .run_cycle(
            test_snapshot("Update README.md, then check memory before answering."),
            &llm,
        )
        .await
        .expect("run_cycle");

    assert_eq!(
        complete_response(result),
        "Finished after continuing from the deferred guardrail."
    );
    assert_eq!(executor.readme_contents(), "first write");

    let requests = llm.requests();
    assert!(
        requests.len() >= 3,
        "expected initial reason, empty tool continuation, and follow-up reason: {requests:?}"
    );
    assert!(
        request_contains_text(
            requests.last().expect("follow-up request"),
            "Calls after the first mutation were deferred"
        ),
        "follow-up reasoning should retain the policy-deferred guardrail context"
    );
}

#[test]
fn tool_progress_summary_omits_policy_deferred_control_state() {
    let summary = summarize_tool_progress(&[
        ToolResult {
            tool_call_id: "ok-1".to_string(),
            tool_name: "run_command".to_string(),
            success: true,
            output: "ok".to_string(),
            failure_class: None,
        },
        ToolResult {
            tool_call_id: "read-1".to_string(),
            tool_name: "read_file".to_string(),
            success: false,
            output: "file is missing".to_string(),
            failure_class: Some(FailureClass::NotFound),
        },
        ToolResult {
            tool_call_id: "memory-1".to_string(),
            tool_name: "memory_search".to_string(),
            success: false,
            output: "Calls after the first mutation were deferred until after you review the latest mutation result: memory_search. Re-request in your next turn if still needed.".to_string(),
            failure_class: Some(FailureClass::PolicyDeferred),
        },
    ])
    .expect("summary");

    assert!(
        summary.contains("latest blocker: file is missing"),
        "real failures should still dominate as blockers: {summary}"
    );
    assert!(
        !summary.contains("follow-up tool work was paused"),
        "policy-deferred control state should not become user-visible fallback prose: {summary}"
    );
    assert!(
        !summary.contains("completed tool work"),
        "successful tool progress belongs to activity events, not fallback prose: {summary}"
    );
    for banned in [
        "Calls after the first mutation",
        "Deferred until",
        "Re-request in your next turn",
    ] {
        assert!(
            !summary.contains(banned),
            "summary leaked internal deferred guardrail phrase `{banned}`: {summary}"
        );
    }
}

#[tokio::test]
async fn pending_mutation_only_scope_limits_next_reasoning_pass() {
    let mut engine = mixed_tool_engine(BudgetConfig::default());
    engine.apply_pending_turn_commitment(
            &ActionContinuation::new(
                Some("I have enough context to implement now.".to_string()),
                Some("Proceed with implementation.".to_string()),
            )
            .with_tool_scope(ContinuationToolScope::MutationOnly)
            .with_turn_commitment(TurnCommitment::ProceedUnderConstraints(
                ProceedUnderConstraints {
                    goal: "Implement the committed local skill changes.".to_string(),
                    success_target: Some(
                        "Use a side-effect-capable tool to make concrete forward progress before doing any more broad research.".to_string(),
                    ),
                    unsupported_items: vec!["Do not reopen X API rate-limit research.".to_string()],
                    assumptions: vec!["Current research is sufficient to begin implementation.".to_string()],
                    allowed_tools: Some(ContinuationToolScope::MutationOnly),
                },
            )),
            &[],
        );

    let llm = RecordingLlm::ok(vec![CompletionResponse {
        content: vec![ContentBlock::Text {
            text: "I'll implement it now.".to_string(),
        }],
        tool_calls: Vec::new(),
        usage: None,
        stop_reason: None,
    }]);

    let processed = engine
        .perceive(&test_snapshot("Keep going"))
        .await
        .expect("perceive");
    let _ = engine
        .reason(&processed, &llm, CycleStream::disabled())
        .await
        .expect("reason");

    let requests = llm.requests();
    assert_eq!(requests.len(), 1);
    assert!(
        requests[0]
            .tools
            .iter()
            .any(|tool| tool.name == "write_file"),
        "mutation tools should remain available under continuation scope"
    );
    assert!(
        !requests[0]
            .tools
            .iter()
            .any(|tool| tool.name == "read_file"),
        "observation tools should be hidden under continuation scope"
    );
    let system_prompt = requests[0].system_prompt.as_deref().expect("system prompt");
    assert!(system_prompt.contains("Turn commitment:"));
    assert!(system_prompt.contains("committed constrained execution plan"));
    assert!(system_prompt.contains("Implement the committed local skill changes."));
    assert!(system_prompt.contains("Do not reopen X API rate-limit research."));
}

#[tokio::test]
async fn pending_turn_commitment_persists_when_later_continuation_omits_replacement() {
    let mut engine = mixed_tool_engine(BudgetConfig::default());
    engine.apply_pending_turn_commitment(
            &ActionContinuation::new(
                Some("Spec written.".to_string()),
                Some("Proceed with local implementation.".to_string()),
            )
            .with_tool_scope(ContinuationToolScope::MutationOnly)
            .with_turn_commitment(TurnCommitment::ProceedUnderConstraints(
                ProceedUnderConstraints {
                    goal: "Implement the committed local skill changes.".to_string(),
                    success_target: Some(
                        "Use a side-effect-capable tool to make concrete forward progress before doing any more broad research.".to_string(),
                    ),
                    unsupported_items: vec!["Do not reopen web research.".to_string()],
                    assumptions: vec!["The spec file already exists.".to_string()],
                    allowed_tools: Some(ContinuationToolScope::MutationOnly),
                },
            )),
            &[],
        );

    engine.apply_pending_turn_commitment(
        &ActionContinuation::new(
            Some("Wrote the spec file.".to_string()),
            Some("Continuing into implementation.".to_string()),
        ),
        &[],
    );

    assert_eq!(
        engine.pending_tool_scope,
        Some(ContinuationToolScope::MutationOnly)
    );
    assert_eq!(
            engine.pending_turn_commitment,
            Some(TurnCommitment::ProceedUnderConstraints(
                ProceedUnderConstraints {
                    goal: "Implement the committed local skill changes.".to_string(),
                    success_target: Some(
                        "Use a side-effect-capable tool to make concrete forward progress before doing any more broad research.".to_string(),
                    ),
                    unsupported_items: vec!["Do not reopen web research.".to_string()],
                    assumptions: vec!["The spec file already exists.".to_string()],
                    allowed_tools: Some(ContinuationToolScope::MutationOnly),
                }
            ))
        );

    let llm = RecordingLlm::ok(vec![CompletionResponse {
        content: vec![ContentBlock::Text {
            text: "Continuing implementation.".to_string(),
        }],
        tool_calls: Vec::new(),
        usage: None,
        stop_reason: None,
    }]);

    let processed = engine
        .perceive(&test_snapshot("Keep going"))
        .await
        .expect("perceive");
    let _ = engine
        .reason(&processed, &llm, CycleStream::disabled())
        .await
        .expect("reason");

    let requests = llm.requests();
    assert_eq!(requests.len(), 1);
    assert!(
        requests[0]
            .tools
            .iter()
            .any(|tool| tool.name == "write_file"),
        "mutation tools should still be available"
    );
    assert!(
        !requests[0]
            .tools
            .iter()
            .any(|tool| tool.name == "read_file"),
        "observation tools should stay hidden while commitment is active"
    );
    let system_prompt = requests[0].system_prompt.as_deref().expect("system prompt");
    assert!(system_prompt.contains("Implement the committed local skill changes."));
    assert!(system_prompt.contains("Do not reopen web research."));
}

#[tokio::test]
async fn artifact_gate_limits_next_reasoning_pass_to_write_file() {
    let mut engine = run_command_observation_engine(BudgetConfig::default());
    engine.apply_pending_turn_commitment(
            &ActionContinuation::new(
                Some("The X skill spec is ready to materialize.".to_string()),
                Some("Write the requested spec file next.".to_string()),
            )
            .with_tool_scope(ContinuationToolScope::MutationOnly)
            .with_turn_commitment(TurnCommitment::ProceedUnderConstraints(
                ProceedUnderConstraints {
                    goal: "Write the requested X skill spec, then continue local implementation."
                        .to_string(),
                    success_target: Some(
                        "Materialize the requested ~/.fawx/x.md spec before broader implementation work."
                            .to_string(),
                    ),
                    unsupported_items: vec!["Do not reopen web research before writing the spec."
                        .to_string()],
                    assumptions: vec!["Current research is sufficient to write the spec artifact."
                        .to_string()],
                    allowed_tools: Some(ContinuationToolScope::MutationOnly),
                },
            ))
            .with_artifact_write_target("~/.fawx/x.md".to_string()),
            &[],
        );

    let llm = RecordingLlm::ok(vec![CompletionResponse {
        content: vec![ContentBlock::Text {
            text: "Writing the spec now.".to_string(),
        }],
        tool_calls: Vec::new(),
        usage: None,
        stop_reason: None,
    }]);

    let processed = engine
        .perceive(&test_snapshot("Keep going"))
        .await
        .expect("perceive");
    let _ = engine
        .reason(&processed, &llm, CycleStream::disabled())
        .await
        .expect("reason");

    let requests = llm.requests();
    assert_eq!(requests.len(), 1);
    let tool_names: Vec<&str> = requests[0]
        .tools
        .iter()
        .map(|tool| tool.name.as_str())
        .collect();
    assert_eq!(
        tool_names,
        vec!["write_file"],
        "artifact gate should collapse the next public tool surface to write_file"
    );
    let system_prompt = requests[0].system_prompt.as_deref().expect("system prompt");
    assert!(system_prompt.contains("Turn commitment:"));
    assert!(system_prompt.contains("Artifact gate:"));
    assert!(system_prompt.contains("~/.fawx/x.md"));
    assert!(system_prompt.contains("Do not reopen web research before writing the spec."));
}

#[tokio::test]
async fn artifact_gate_clears_after_successful_write_and_preserves_broader_commitment() {
    let mut engine = run_command_observation_engine(BudgetConfig::default());
    let home = std::env::var("HOME").expect("HOME");
    engine.apply_pending_turn_commitment(
            &ActionContinuation::new(
                Some("The X skill spec is ready to materialize.".to_string()),
                Some("Write the requested spec file next.".to_string()),
            )
            .with_tool_scope(ContinuationToolScope::MutationOnly)
            .with_turn_commitment(TurnCommitment::ProceedUnderConstraints(
                ProceedUnderConstraints {
                    goal: "Write the requested X skill spec, then continue local implementation."
                        .to_string(),
                    success_target: Some(
                        "Materialize the requested ~/.fawx/x.md spec before broader implementation work."
                            .to_string(),
                    ),
                    unsupported_items: vec!["Do not reopen web research before writing the spec."
                        .to_string()],
                    assumptions: vec!["Current research is sufficient to write the spec artifact."
                        .to_string()],
                    allowed_tools: Some(ContinuationToolScope::MutationOnly),
                },
            ))
            .with_artifact_write_target("~/.fawx/x.md".to_string()),
            &[],
        );

    engine.apply_pending_turn_commitment(
        &ActionContinuation::new(
            Some("Spec written.".to_string()),
            Some("Continue with local implementation.".to_string()),
        ),
        &[ToolResult {
            tool_call_id: "call-1".to_string(),
            tool_name: "write_file".to_string(),
            success: true,
            output: format!("wrote 64 bytes to {home}/.fawx/x.md"),
            failure_class: None,
        }],
    );

    assert!(engine.pending_artifact_write_target.is_none());
    assert_eq!(
        engine.pending_tool_scope,
        Some(ContinuationToolScope::MutationOnly)
    );
    assert_eq!(
            engine.pending_turn_commitment,
            Some(TurnCommitment::ProceedUnderConstraints(
                ProceedUnderConstraints {
                    goal: "Write the requested X skill spec, then continue local implementation."
                        .to_string(),
                    success_target: Some(
                        "Materialize the requested ~/.fawx/x.md spec before broader implementation work."
                            .to_string(),
                    ),
                    unsupported_items: vec!["Do not reopen web research before writing the spec."
                        .to_string()],
                    assumptions: vec!["Current research is sufficient to write the spec artifact."
                        .to_string()],
                    allowed_tools: Some(ContinuationToolScope::MutationOnly),
                }
            ))
        );

    let llm = RecordingLlm::ok(vec![CompletionResponse {
        content: vec![ContentBlock::Text {
            text: "Continuing with local implementation.".to_string(),
        }],
        tool_calls: Vec::new(),
        usage: None,
        stop_reason: None,
    }]);

    let processed = engine
        .perceive(&test_snapshot("Keep going"))
        .await
        .expect("perceive");
    let _ = engine
        .reason(&processed, &llm, CycleStream::disabled())
        .await
        .expect("reason");

    let requests = llm.requests();
    assert_eq!(requests.len(), 1);
    assert!(
        requests[0]
            .tools
            .iter()
            .any(|tool| tool.name == "write_file"),
        "mutation tools should remain available after the artifact gate clears"
    );
    assert!(
        requests[0]
            .tools
            .iter()
            .any(|tool| tool.name == "run_command"),
        "the broader mutation-only commitment should survive after the artifact write"
    );
    let system_prompt = requests[0].system_prompt.as_deref().expect("system prompt");
    assert!(system_prompt.contains("Turn commitment:"));
    assert!(!system_prompt.contains("Artifact gate:"));
}

#[tokio::test]
async fn tools_not_stripped_before_grace() {
    let config = BudgetConfig {
        termination: TerminationConfig {
            nudge_after_tool_turns: 3,
            strip_tools_after_nudge: 2,
            ..TerminationConfig::default()
        },
        ..BudgetConfig::default()
    };
    let mut engine = LoopEngine::builder()
        .budget(BudgetTracker::new(config, 0, 0))
        .context(ContextCompactor::new(2048, 256))
        .max_iterations(3)
        .tool_executor(Arc::new(StubToolExecutor))
        .synthesis_instruction("Summarize".to_string())
        .build()
        .expect("build");
    // At turn 4 (below 3+2=5), tools should NOT be stripped
    engine.consecutive_tool_turns = 4;

    let llm = RecordingLlm::ok(vec![CompletionResponse {
        content: vec![ContentBlock::Text {
            text: "still working".to_string(),
        }],
        tool_calls: Vec::new(),
        usage: None,
        stop_reason: None,
    }]);

    let processed = engine
        .perceive(&test_snapshot("hello"))
        .await
        .expect("perceive");
    let _ = engine
        .reason(&processed, &llm, CycleStream::disabled())
        .await
        .expect("reason");

    let requests = llm.requests();
    assert_eq!(requests.len(), 1);
    assert!(
        !requests[0].tools.is_empty(),
        "tools should still be present at turn 4, threshold 5"
    );
}

#[path = "../loop_resilience_tests/direct_inspection_tests.rs"]
mod direct_inspection_tests;

#[path = "../loop_resilience_tests/bounded_local_tests.rs"]
mod bounded_local_tests;

#[path = "../loop_resilience_tests/profile_boundary_tests.rs"]
mod profile_boundary_tests;

#[path = "../loop_resilience_tests/preflight_route_tests.rs"]
mod preflight_route_tests;

#[path = "../loop_resilience_tests/external_action_tests.rs"]
mod external_action_tests;

#[tokio::test]
async fn synthesis_skipped_when_disabled() {
    let config = BudgetConfig {
        max_llm_calls: 1,
        termination: TerminationConfig {
            synthesize_on_exhaustion: false,
            ..TerminationConfig::default()
        },
        ..BudgetConfig::default()
    };
    let mut budget = BudgetTracker::new(config, 0, 0);
    budget.record(&ActionCost {
        llm_calls: 1,
        ..ActionCost::default()
    });

    let mut engine = engine_with_tracker(budget);
    let llm = RecordingLlm::ok(vec![CompletionResponse {
        content: vec![ContentBlock::Text {
            text: "synthesized".to_string(),
        }],
        tool_calls: Vec::new(),
        usage: None,
        stop_reason: None,
    }]);
    let messages = vec![Message::user("hello")];

    let result = engine
        .forced_synthesis_turn(&llm, &messages, CycleStream::disabled())
        .await;

    assert_eq!(result, None);
    assert!(llm.requests().is_empty());
}

fn tool_action(response_text: &str) -> ActionResult {
    let normalized = normalize_response_text(response_text);
    let partial_response = (!normalized.is_empty()).then_some(normalized.clone());
    let context_message = partial_response
        .clone()
        .or_else(|| Some("Tool execution completed: read_file".to_string()));
    ActionResult {
        decision: Decision::UseTools(Vec::new()),
        tool_results: vec![ToolResult {
            tool_call_id: "call-1".to_string(),
            tool_name: "read_file".to_string(),
            success: true,
            output: "ok".to_string(),
            failure_class: None,
        }],
        response_text: response_text.to_string(),
        tokens_used: TokenUsage::default(),
        next_step: ActionNextStep::Continue(ActionContinuation::new(
            partial_response,
            context_message,
        )),
    }
}

fn tool_continuation_without_results_action(response_text: &str) -> ActionResult {
    let normalized = normalize_response_text(response_text);
    let partial_response = (!normalized.is_empty()).then_some(normalized.clone());
    let context_message = partial_response
        .clone()
        .or_else(|| Some("Tool execution continues".to_string()));
    ActionResult {
        decision: Decision::UseTools(Vec::new()),
        tool_results: Vec::new(),
        response_text: response_text.to_string(),
        tokens_used: TokenUsage::default(),
        next_step: ActionNextStep::Continue(ActionContinuation::new(
            partial_response,
            context_message,
        )),
    }
}

fn decomposition_continue_action() -> ActionResult {
    ActionResult {
        decision: Decision::Decompose(fx_decompose::DecompositionPlan {
            sub_goals: Vec::new(),
            strategy: fx_decompose::AggregationStrategy::Sequential,
            reasoning_mode: fx_decompose::ReasoningMode::Standard,
            truncated_from: None,
        }),
        tool_results: Vec::new(),
        response_text: "Task decomposition results: none".to_string(),
        tokens_used: TokenUsage::default(),
        next_step: ActionNextStep::Continue(ActionContinuation::new(
            None,
            Some("Task decomposition results: none".to_string()),
        )),
    }
}

fn text_only_action(response_text: &str) -> ActionResult {
    ActionResult {
        decision: Decision::Respond(response_text.to_string()),
        tool_results: Vec::new(),
        response_text: response_text.to_string(),
        tokens_used: TokenUsage::default(),
        next_step: ActionNextStep::Finish(ActionTerminal::Complete {
            response: response_text.to_string(),
        }),
    }
}

#[test]
fn default_termination_config_matches_current_behavior() {
    let config = TerminationConfig::default();
    assert!(config.synthesize_on_exhaustion);
    assert_eq!(config.nudge_after_tool_turns, 6);
    assert_eq!(
        config.strip_tools_after_nudge,
        crate::budget::DISABLE_TOOL_STRIPPING_AFTER_NUDGE
    );
    assert_eq!(config.tool_round_nudge_after, 4);
    assert_eq!(
        config.tool_round_strip_after_nudge,
        crate::budget::DISABLE_TOOL_STRIPPING_AFTER_NUDGE
    );
    assert_eq!(config.observation_only_round_nudge_after, 2);
    assert_eq!(
        config.observation_only_round_strip_after_nudge,
        crate::budget::DISABLE_TOOL_STRIPPING_AFTER_NUDGE
    );
}

#[test]
fn user_message_evidence_extraction_seeds_pr_issue_and_file_references() {
    let references = extract_user_evidence_references(
        "Review PR #1834, compare issue 1704, and inspect app/Fawx/ViewModels/ChatViewModel.swift.",
    );

    assert!(references.iter().any(|reference| reference == "pr 1834"));
    assert!(references.iter().any(|reference| reference == "#1834"));
    assert!(references.iter().any(|reference| reference == "issue 1704"));
    assert!(references
        .iter()
        .any(|reference| reference == "app/Fawx/ViewModels/ChatViewModel.swift"));
}

#[test]
fn user_message_evidence_extraction_ignores_numbered_issue_list_items() {
    let references = extract_user_evidence_references(
        "please resolve these issues:\n\n1. discarded_field_note is too broad.\n2. ResultKind::Error regressed headless callers.",
    );

    assert!(
        !references.iter().any(|reference| reference == "issue 1"),
        "plural issue prose plus a numbered list item must not become a fake issue reference"
    );
    assert!(
        !references.iter().any(|reference| reference == "issue 2"),
        "plural issue prose plus a numbered list item must not become a fake issue reference"
    );
}

#[test]
fn pr_diff_satisfies_explicit_evidence_and_seeds_discovered_file_slots() {
    let mut engine = fingerprint_run_command_engine(BudgetConfig::default());
    engine
        .turn_progress_ledger
        .seed_explicit_evidence_slot("pr 1813");
    let diff_call = ToolCall {
        id: "call-1".to_string(),
        name: "run_command".to_string(),
        arguments: serde_json::json!({
            "command": "gh pr diff 1813",
            "cwd": "/Users/joseph/fawx",
        }),
    };

    engine.record_tool_round_progress(
        std::slice::from_ref(&diff_call),
        &[ToolResult::success(
            "call-1",
            "run_command",
            "diff --git a/app/Fawx/ViewModels/ChatViewModel.swift b/app/Fawx/ViewModels/ChatViewModel.swift\n--- a/app/Fawx/ViewModels/ChatViewModel.swift\n+++ b/app/Fawx/ViewModels/ChatViewModel.swift",
        )],
    );

    let explicit_slot = engine
        .turn_progress_ledger
        .evidence_slots
        .values()
        .find(|slot| slot.explicit && slot.label == "pr 1813")
        .expect("explicit PR slot");
    assert_eq!(explicit_slot.status, ProgressSlotStatus::Satisfied);
    assert!(engine
        .turn_progress_ledger
        .evidence_slots
        .values()
        .any(|slot| !slot.explicit
            && slot.label == "app/Fawx/ViewModels/ChatViewModel.swift"
            && slot.status == ProgressSlotStatus::Open));

    let read_discovered_file = ToolCall {
        id: "call-2".to_string(),
        name: "run_command".to_string(),
        arguments: serde_json::json!({
            "command": "sed -n '1,120p' app/Fawx/ViewModels/ChatViewModel.swift",
            "cwd": "/Users/joseph/fawx",
        }),
    };
    engine.record_tool_round_progress(
        std::slice::from_ref(&read_discovered_file),
        &[ToolResult::success(
            "call-2",
            "run_command",
            "view model source",
        )],
    );

    assert_eq!(
        engine.observation_round_tracker.repetitive_rounds, 0,
        "following a discovered diff path should be productive evidence"
    );
    let discovered_slot = engine
        .turn_progress_ledger
        .evidence_slots
        .values()
        .find(|slot| slot.label == "app/Fawx/ViewModels/ChatViewModel.swift")
        .expect("discovered file slot");
    assert_eq!(discovered_slot.status, ProgressSlotStatus::Satisfied);
}

#[test]
fn search_hits_surface_discovered_follow_up_contract_without_explicit_slots() {
    let mut engine = mixed_tool_engine(BudgetConfig::default());
    let search_call = ToolCall {
        id: "call-1".to_string(),
        name: "search_text".to_string(),
        arguments: serde_json::json!({
            "pattern": "ChatTranscriptItem",
            "path": "app/Fawx",
        }),
    };

    engine.record_tool_round_progress(
        std::slice::from_ref(&search_call),
        &[ToolResult::success(
            "call-1",
            "search_text",
            "\
app/Fawx/Models/ChatTranscript.swift:64:enum ChatTranscriptItem
app/Fawx/Models/ChatTranscript.swift:80:case toolActivityGroup
app/Fawx/ViewModels/ChatViewModel.swift:1024:func makeTranscriptItems()",
        )],
    );

    assert!(
        !engine.turn_progress_ledger.has_explicit_evidence_slots(),
        "this regression covers organic inspection tasks with no declared contract"
    );
    let directive = engine
        .task_contract_state_directive()
        .expect("discovered search hits should create a follow-up evidence directive");
    assert!(directive.contains("Discovered follow-up evidence"));
    assert!(directive.contains("app/Fawx/Models/ChatTranscript.swift"));
    assert!(directive.contains("app/Fawx/ViewModels/ChatViewModel.swift"));
    assert_eq!(
        engine
            .turn_progress_ledger
            .evidence_slots
            .values()
            .filter(|slot| slot.label == "app/Fawx/Models/ChatTranscript.swift")
            .count(),
        1,
        "multiple line hits in the same file should collapse into one follow-up slot"
    );
    assert!(directive.contains("read_file before answering"));
    assert!(
        directive.contains("file:line search hits"),
        "the model must not treat pointer-only search output as complete evidence"
    );
}

#[test]
fn arbitrary_observation_output_does_not_seed_discovered_file_slots() {
    let mut engine = fingerprint_run_command_engine(BudgetConfig::default());
    let cat_call = ToolCall {
        id: "call-1".to_string(),
        name: "run_command".to_string(),
        arguments: serde_json::json!({
            "command": "cat README.md",
            "cwd": "/Users/joseph/fawx",
        }),
    };

    engine.record_tool_round_progress(
        std::slice::from_ref(&cat_call),
        &[ToolResult::success(
            "call-1",
            "run_command",
            "A prose mention of app/Fawx/Models/ChatTranscript.swift:64 should not create a follow-up slot.",
        )],
    );

    assert!(
        !engine
            .turn_progress_ledger
            .evidence_slots
            .values()
            .any(|slot| slot.label == "app/Fawx/Models/ChatTranscript.swift"),
        "non-discovery command output should not create noisy discovered evidence"
    );
    assert!(
        engine.task_contract_state_directive().is_none(),
        "without scoped discovered evidence, arbitrary output should not force a follow-up directive"
    );
}

#[test]
fn unrelated_observation_after_required_evidence_is_unproductive_pressure() {
    let mut engine = fingerprint_run_command_engine(BudgetConfig::default());
    engine
        .turn_progress_ledger
        .seed_explicit_evidence_slot("pr 1813");
    engine.record_tool_round_progress(
        &[ToolCall {
            id: "call-1".to_string(),
            name: "run_command".to_string(),
            arguments: serde_json::json!({
                "command": "gh pr view 1813",
                "cwd": "/Users/joseph/fawx",
            }),
        }],
        &[ToolResult::success("call-1", "run_command", "PR 1813")],
    );
    assert_eq!(engine.observation_round_tracker.repetitive_rounds, 0);

    engine.record_tool_round_progress(
        &[ToolCall {
            id: "call-2".to_string(),
            name: "run_command".to_string(),
            arguments: serde_json::json!({
                "command": "git status",
                "cwd": "/Users/joseph/fawx",
            }),
        }],
        &[ToolResult::success(
            "call-2",
            "run_command",
            "On branch dev",
        )],
    );

    assert_eq!(
        engine.observation_round_tracker.repetitive_rounds, 1,
        "once required evidence is satisfied, unrelated observation is pressure"
    );
    assert_eq!(
        engine
            .turn_progress_ledger
            .tool_entries
            .last()
            .map(|entry| entry.outcome),
        Some(ToolProgressOutcome::Duplicate)
    );
}

#[test]
fn distinct_read_file_rounds_advance_evidence_without_observation_pressure() {
    let config = BudgetConfig {
        termination: TerminationConfig {
            observation_only_round_nudge_after: 1,
            observation_only_round_strip_after_nudge: 1,
            ..TerminationConfig::default()
        },
        ..BudgetConfig::default()
    };
    let mut engine = mixed_tool_engine(config);
    let first_round = vec![ToolCall {
        id: "call-1".to_string(),
        name: "read_file".to_string(),
        arguments: serde_json::json!({"path":"README.md"}),
    }];
    let second_round = vec![ToolCall {
        id: "call-2".to_string(),
        name: "read_file".to_string(),
        arguments: serde_json::json!({"path":"Cargo.toml"}),
    }];

    engine.record_tool_round_kind(&first_round);
    let mut continuation_messages = Vec::new();
    let first_tools = engine.apply_tool_round_progress_policy(0, &mut continuation_messages);
    assert_eq!(
        engine
            .observation_round_tracker
            .consecutive_observation_only_rounds,
        1
    );
    assert_eq!(engine.observation_round_tracker.repetitive_rounds, 0);
    assert_eq!(
        first_tools.len(),
        2,
        "first observation-only round should keep the full surface"
    );
    assert!(
        continuation_messages.is_empty(),
        "productive evidence gathering should not trigger the observation nudge"
    );

    engine.record_tool_round_kind(&second_round);
    let mut continuation_messages = Vec::new();
    let second_tools = engine.apply_tool_round_progress_policy(1, &mut continuation_messages);
    assert_eq!(
        engine
            .observation_round_tracker
            .consecutive_observation_only_rounds,
        2
    );
    assert_eq!(
        engine.observation_round_tracker.repetitive_rounds, 0,
        "distinct read fingerprints are not exact repeats"
    );

    assert_eq!(
        second_tools.len(),
        2,
        "distinct evidence reads should keep the full tool surface"
    );
    assert!(
        continuation_messages.is_empty(),
        "distinct evidence reads should not emit the wrap-up nudge"
    );
    assert_eq!(engine.turn_progress_ledger.evidence_slots.len(), 2);
    assert!(engine
        .turn_progress_ledger
        .tool_entries
        .iter()
        .all(|entry| entry.outcome == ToolProgressOutcome::Advanced));
}

#[test]
fn repeated_read_file_rounds_nudge_then_strip_observation_tools() {
    let config = BudgetConfig {
        termination: TerminationConfig {
            observation_only_round_nudge_after: 1,
            observation_only_round_strip_after_nudge: 1,
            ..TerminationConfig::default()
        },
        ..BudgetConfig::default()
    };
    let mut engine = mixed_tool_engine(config);

    let repeated_round = vec![ToolCall {
        id: "call-1".to_string(),
        name: "read_file".to_string(),
        arguments: serde_json::json!({"path":"README.md"}),
    }];

    engine.record_tool_round_kind(&repeated_round);
    let mut continuation_messages = Vec::new();
    let nudged_tools = engine.apply_tool_round_progress_policy(0, &mut continuation_messages);
    assert_eq!(
        engine
            .observation_round_tracker
            .consecutive_observation_only_rounds,
        1
    );
    assert_eq!(engine.observation_round_tracker.repetitive_rounds, 0);
    assert_eq!(
        nudged_tools.len(),
        2,
        "the first observation-only round should still keep tools"
    );
    assert!(
        continuation_messages.is_empty(),
        "first observation should not warn before duplicate pressure exists"
    );

    engine.record_tool_round_kind(&repeated_round);
    let mut continuation_messages = Vec::new();
    let nudged_again_tools = engine.apply_tool_round_progress_policy(1, &mut continuation_messages);
    assert_eq!(
        engine
            .observation_round_tracker
            .consecutive_observation_only_rounds,
        2
    );
    assert_eq!(engine.observation_round_tracker.repetitive_rounds, 1);

    assert_eq!(
        nudged_again_tools.len(),
        2,
        "the first duplicate observation round should warn before stripping"
    );
    assert!(continuation_messages.iter().any(|msg| {
        msg.content.iter().any(|block| match block {
            ContentBlock::Text { text } => text.contains("Stop doing more read-only research"),
            _ => false,
        })
    }));

    engine.record_tool_round_kind(&repeated_round);
    let mut continuation_messages = Vec::new();
    let stripped_tools = engine.apply_tool_round_progress_policy(2, &mut continuation_messages);
    assert_eq!(engine.observation_round_tracker.repetitive_rounds, 2);
    assert_eq!(
        stripped_tools.len(),
        1,
        "repeated duplicate reads should strip to side-effect tools"
    );
    assert_eq!(stripped_tools[0].name, "write_file");
}

#[test]
fn retryable_failed_observation_keeps_follow_up_reads_available() {
    let config = BudgetConfig {
        termination: TerminationConfig {
            observation_only_round_nudge_after: 1,
            observation_only_round_strip_after_nudge: 1,
            ..TerminationConfig::default()
        },
        ..BudgetConfig::default()
    };
    let mut engine = mixed_tool_engine(config);
    let failed_call = ToolCall {
        id: "call-1".to_string(),
        name: "read_file".to_string(),
        arguments: serde_json::json!({"path":"engine/src/missing.rs"}),
    };
    let corrected_call = ToolCall {
        id: "call-2".to_string(),
        name: "read_file".to_string(),
        arguments: serde_json::json!({"path":"engine/crates/fx-kernel/src/lib.rs"}),
    };

    engine.record_tool_round_progress(
        std::slice::from_ref(&failed_call),
        &[ToolResult::failure(
            "call-1",
            "read_file",
            "No such file or directory",
            FailureClass::NotFound,
        )],
    );
    let mut continuation_messages = Vec::new();
    let tools_after_failure =
        engine.apply_tool_round_progress_policy(0, &mut continuation_messages);
    assert_eq!(
        engine.observation_round_tracker.repetitive_rounds, 0,
        "a first failed path is retryable evidence, not duplicate pressure"
    );
    assert_eq!(
        tools_after_failure.len(),
        2,
        "corrected read attempts must remain available after a path miss"
    );

    engine.record_tool_round_progress(
        std::slice::from_ref(&corrected_call),
        &[ToolResult::success("call-2", "read_file", "kernel source")],
    );
    let mut continuation_messages = Vec::new();
    let tools_after_correction =
        engine.apply_tool_round_progress_policy(1, &mut continuation_messages);
    assert_eq!(engine.observation_round_tracker.repetitive_rounds, 0);
    assert_eq!(
        tools_after_correction.len(),
        2,
        "a corrected successful read should still be treated as productive progress"
    );
    assert_eq!(
        engine
            .turn_progress_ledger
            .tool_entries
            .iter()
            .map(|entry| entry.outcome)
            .collect::<Vec<_>>(),
        vec![
            ToolProgressOutcome::RetryableFailure,
            ToolProgressOutcome::Advanced
        ]
    );
}

#[test]
fn run_command_progress_target_uses_display_command_for_argv() {
    let mut engine = mixed_tool_engine(BudgetConfig::default());
    let call = ToolCall {
        id: "call-1".to_string(),
        name: "run_command".to_string(),
        arguments: serde_json::json!({
            "argv": [
                "gh",
                "pr",
                "comment",
                "1858",
                "--repo",
                "fawxai/fawx",
                "--body",
                "Review posted"
            ]
        }),
    };

    let entries = engine.record_tool_round_progress(
        std::slice::from_ref(&call),
        &[ToolResult::success(
            "call-1",
            "run_command",
            "https://github.com/fawxai/fawx/pull/1858#issuecomment-1",
        )],
    );

    assert_eq!(entries.len(), 1);
    assert_eq!(
        entries[0].target.as_deref(),
        Some("gh pr comment 1858 --repo fawxai/fawx --body Review posted")
    );
    assert!(
        !entries[0]
            .target
            .as_deref()
            .unwrap_or_default()
            .contains("{\"argv\""),
        "stream progress targets should not expose raw tool-call JSON"
    );
}

#[test]
fn repeated_failed_observation_accumulates_duplicate_pressure() {
    let config = BudgetConfig {
        termination: TerminationConfig {
            observation_only_round_nudge_after: 1,
            observation_only_round_strip_after_nudge: 1,
            ..TerminationConfig::default()
        },
        ..BudgetConfig::default()
    };
    let mut engine = mixed_tool_engine(config);
    let failed_call = ToolCall {
        id: "call-1".to_string(),
        name: "read_file".to_string(),
        arguments: serde_json::json!({"path":"engine/src/missing.rs"}),
    };

    engine.record_tool_round_progress(
        std::slice::from_ref(&failed_call),
        &[ToolResult::failure(
            "call-1",
            "read_file",
            "No such file or directory",
            FailureClass::NotFound,
        )],
    );
    let repeated_failed_call = ToolCall {
        id: "call-2".to_string(),
        ..failed_call
    };
    engine.record_tool_round_progress(
        std::slice::from_ref(&repeated_failed_call),
        &[ToolResult::failure(
            "call-2",
            "read_file",
            "No such file or directory",
            FailureClass::NotFound,
        )],
    );

    assert_eq!(
        engine.observation_round_tracker.repetitive_rounds, 1,
        "repeating the same failed path is duplicate pressure"
    );
    assert_eq!(
        engine
            .turn_progress_ledger
            .tool_entries
            .iter()
            .map(|entry| entry.outcome)
            .collect::<Vec<_>>(),
        vec![
            ToolProgressOutcome::RetryableFailure,
            ToolProgressOutcome::Duplicate
        ]
    );
}

#[test]
fn completed_work_summary_counts_tool_progress_by_kind() {
    let summary = completed_work_summary_from_progress_entries(&[
        ToolProgressEntry {
            call_id: "call-1".to_string(),
            tool_name: "read_file".to_string(),
            class: ToolProgressClass::Observation,
            target: Some("src/app.rs".to_string()),
            advances_slot: None,
            outcome: ToolProgressOutcome::Advanced,
        },
        ToolProgressEntry {
            call_id: "call-2".to_string(),
            tool_name: "search_text".to_string(),
            class: ToolProgressClass::Observation,
            target: Some("ChatTranscript".to_string()),
            advances_slot: None,
            outcome: ToolProgressOutcome::Duplicate,
        },
        ToolProgressEntry {
            call_id: "call-3".to_string(),
            tool_name: "run_command".to_string(),
            class: ToolProgressClass::Mutation,
            target: Some("gh pr comment".to_string()),
            advances_slot: None,
            outcome: ToolProgressOutcome::RetryableFailure,
        },
    ])
    .expect("summary");

    assert_eq!(
        summary,
        "Worked this turn: 1 command, 1 file read, 1 search, 1 failed, 1 repeated."
    );
}

#[test]
fn tool_round_strip_preserves_mutation_tools_when_available() {
    let config = BudgetConfig {
        termination: TerminationConfig {
            tool_round_nudge_after: 1,
            tool_round_strip_after_nudge: 0,
            ..TerminationConfig::default()
        },
        ..BudgetConfig::default()
    };
    let engine = mixed_tool_engine(config);
    let mut continuation_messages = Vec::new();

    let tools = engine.apply_tool_round_progress_policy(1, &mut continuation_messages);

    assert_eq!(tools.len(), 1, "progress strip should keep mutation tools");
    assert_eq!(tools[0].name, "write_file");
}

#[test]
fn record_tool_round_kind_resets_after_side_effect_round() {
    let mut engine = mixed_tool_engine(BudgetConfig::default());
    let repeated_read_round = [ToolCall {
        id: "call-1".to_string(),
        name: "read_file".to_string(),
        arguments: serde_json::json!({"path":"README.md"}),
    }];

    engine.record_tool_round_kind(&repeated_read_round);
    engine.record_tool_round_kind(&repeated_read_round);
    assert_eq!(
        engine
            .observation_round_tracker
            .consecutive_observation_only_rounds,
        2
    );
    assert_eq!(engine.observation_round_tracker.repetitive_rounds, 1);

    engine.record_tool_round_kind(&[ToolCall {
        id: "call-2".to_string(),
        name: "write_file".to_string(),
        arguments: serde_json::json!({"path":"/tmp/out.txt","content":"hi"}),
    }]);

    assert_eq!(
        engine
            .observation_round_tracker
            .consecutive_observation_only_rounds,
        0
    );
    assert_eq!(engine.observation_round_tracker.repetitive_rounds, 0);
}

#[test]
fn record_tool_round_kind_treats_distinct_observation_run_command_arguments_as_new() {
    let mut engine = fingerprint_run_command_engine(BudgetConfig::default());

    engine.record_tool_round_kind(&[ToolCall {
        id: "call-1".to_string(),
        name: "run_command".to_string(),
        arguments: serde_json::json!({
            "command": "cat README.md",
            "cwd": "/Users/joseph/fawx",
        }),
    }]);
    assert_eq!(
        engine
            .observation_round_tracker
            .consecutive_observation_only_rounds,
        1
    );
    assert_eq!(engine.observation_round_tracker.repetitive_rounds, 0);

    engine.record_tool_round_kind(&[ToolCall {
        id: "call-2".to_string(),
        name: "run_command".to_string(),
        arguments: serde_json::json!({
            "command": "cat Cargo.toml",
            "cwd": "/Users/joseph/fawx",
        }),
    }]);

    assert_eq!(
        engine
            .observation_round_tracker
            .consecutive_observation_only_rounds,
        2
    );
    assert_eq!(engine.observation_round_tracker.repetitive_rounds, 0);
}

#[test]
fn progress_evidence_references_canonicalize_grep_line_hits() {
    let references = extract_progress_evidence_references(
        "\
/Users/joseph/fawx/tui/src/app.rs:1668:fn render_tool_use_entry
/Users/joseph/fawx/tui/src/app.rs:2670:fn render_tool_result_entry
app/Fawx/Views/Shared/ChatDetailView.swift:64:12: var body: some View
",
    );

    assert_eq!(
        references,
        vec![
            "/Users/joseph/fawx/tui/src/app.rs".to_string(),
            "app/Fawx/Views/Shared/ChatDetailView.swift".to_string(),
        ],
        "line-numbered grep/ripgrep hits must canonicalize to file evidence"
    );
}

#[test]
fn same_file_observation_variants_accumulate_pressure_after_bounded_progress() {
    let config = BudgetConfig {
        termination: TerminationConfig {
            observation_only_round_nudge_after: 1,
            observation_only_round_strip_after_nudge: 1,
            ..TerminationConfig::default()
        },
        ..BudgetConfig::default()
    };
    let mut engine = fingerprint_run_command_engine(config);

    for (index, (command, output)) in [
        (
            r#"grep -n "render_tool_use_entry" /Users/joseph/fawx/tui/src/app.rs"#,
            "/Users/joseph/fawx/tui/src/app.rs:1668:fn render_tool_use_entry",
        ),
        (
            r#"sed -n '900,1040p' /Users/joseph/fawx/tui/src/app.rs"#,
            "/Users/joseph/fawx/tui/src/app.rs:920:fn transcript_entries",
        ),
        (
            r#"grep -n "render_tool_result_entry" /Users/joseph/fawx/tui/src/app.rs"#,
            "/Users/joseph/fawx/tui/src/app.rs:2670:fn render_tool_result_entry",
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let call_id = format!("call-{}", index + 1);
        engine.record_tool_round_progress(
            &[ToolCall {
                id: call_id.clone(),
                name: "run_command".to_string(),
                arguments: serde_json::json!({
                    "command": command,
                    "cwd": "/Users/joseph/fawx",
                }),
            }],
            &[ToolResult::success(&call_id, "run_command", output)],
        );
    }

    assert_eq!(
        engine.observation_round_tracker.repetitive_rounds, 1,
        "changing grep patterns or sed ranges on the same file must not reset observation pressure forever"
    );
    assert_eq!(
        engine
            .turn_progress_ledger
            .tool_entries
            .last()
            .map(|entry| entry.outcome),
        Some(ToolProgressOutcome::Duplicate)
    );

    let mut continuation_messages = Vec::new();
    let tools_after_semantic_repeat =
        engine.apply_tool_round_progress_policy(2, &mut continuation_messages);
    assert_eq!(
        tools_after_semantic_repeat.len(),
        2,
        "the first same-scope pressure round should warn before stripping"
    );
    assert!(continuation_messages.iter().any(|msg| {
        msg.content.iter().any(|block| match block {
            ContentBlock::Text { text } => text.contains("Stop doing more read-only research"),
            _ => false,
        })
    }));

    engine.record_tool_round_progress(
        &[ToolCall {
            id: "call-4".to_string(),
            name: "run_command".to_string(),
            arguments: serde_json::json!({
                "command": r#"rg -n "ToolUse|ToolResult" /Users/joseph/fawx/tui/src/app.rs"#,
                "cwd": "/Users/joseph/fawx",
            }),
        }],
        &[ToolResult::success(
            "call-4",
            "run_command",
            "/Users/joseph/fawx/tui/src/app.rs:3104:fn render_transcript_line",
        )],
    );

    assert_eq!(
        engine.observation_round_tracker.repetitive_rounds, 2,
        "continued same-scope command variation should keep increasing pressure"
    );
}

#[test]
fn record_tool_round_kind_treats_identical_canonicalized_run_command_arguments_as_repetitive() {
    let mut engine = fingerprint_run_command_engine(BudgetConfig::default());

    engine.record_tool_round_kind(&[ToolCall {
        id: "call-1".to_string(),
        name: "run_command".to_string(),
        arguments: serde_json::json!({
            "command": "cat README.md",
            "cwd": "/Users/joseph/fawx",
        }),
    }]);
    engine.record_tool_round_kind(&[ToolCall {
        id: "call-2".to_string(),
        name: "run_command".to_string(),
        arguments: serde_json::json!({
            "cwd": "/Users/joseph/fawx",
            "command": "cat README.md",
        }),
    }]);

    assert_eq!(engine.observation_round_tracker.repetitive_rounds, 1);
}

#[test]
fn observation_only_round_tracker_clears_on_cycle_reset() {
    let mut engine = fingerprint_run_command_engine(BudgetConfig::default());

    engine.record_tool_round_kind(&[ToolCall {
        id: "call-1".to_string(),
        name: "run_command".to_string(),
        arguments: serde_json::json!({
            "command": "cat README.md",
            "cwd": "/Users/joseph/fawx",
        }),
    }]);
    engine.record_tool_round_kind(&[ToolCall {
        id: "call-2".to_string(),
        name: "run_command".to_string(),
        arguments: serde_json::json!({
            "cwd": "/Users/joseph/fawx",
            "command": "cat README.md",
        }),
    }]);

    assert_eq!(engine.observation_round_tracker.repetitive_rounds, 1);
    assert_eq!(
        engine
            .observation_round_tracker
            .consecutive_observation_only_rounds,
        2
    );
    assert!(!engine
        .observation_round_tracker
        .seen_observation_fingerprints
        .is_empty());

    engine.prepare_cycle();

    assert_eq!(engine.observation_round_tracker.repetitive_rounds, 0);
    assert_eq!(
        engine
            .observation_round_tracker
            .consecutive_observation_only_rounds,
        0
    );
    assert!(engine
        .observation_round_tracker
        .seen_observation_fingerprints
        .is_empty());
}

#[test]
fn observation_only_round_tracker_caps_fingerprint_memory() {
    let mut engine = mixed_tool_engine(BudgetConfig::default());
    engine.observation_round_tracker.repetitive_rounds = 1;
    for index in 0..256 {
        engine
            .observation_round_tracker
            .seen_observation_fingerprints
            .insert(format!("read_file:{{\"path\":\"file-{index}.md\"}}"));
    }

    engine.record_tool_round_kind(&[ToolCall {
        id: "call-1".to_string(),
        name: "read_file".to_string(),
        arguments: serde_json::json!({
            "path": "overflow.md",
        }),
    }]);

    assert_eq!(engine.observation_round_tracker.repetitive_rounds, 2);
    assert_eq!(
        engine
            .observation_round_tracker
            .seen_observation_fingerprints
            .len(),
        256
    );
}

#[tokio::test]
async fn observation_only_restriction_blocks_read_only_run_command_calls() {
    let mut engine = run_command_observation_engine(observation_restriction_config());
    engine.observation_round_tracker.repetitive_rounds = 3;

    let results = engine
        .execute_tool_calls(&[
            ToolCall {
                id: "call-1".to_string(),
                name: "run_command".to_string(),
                arguments: serde_json::json!({"command":"cat README.md"}),
            },
            ToolCall {
                id: "call-2".to_string(),
                name: "write_file".to_string(),
                arguments: serde_json::json!({"path":"/tmp/out.txt","content":"hi"}),
            },
        ])
        .await
        .expect("results");

    assert_eq!(results.len(), 2);
    assert!(!results[0].success);
    assert!(results[0]
        .output
        .contains("read-only inspection is disabled"));
    assert!(results[1].success);
}

#[tokio::test]
async fn observation_only_restriction_accepts_bounded_summary_when_no_write_is_pending() {
    let mut engine = mixed_tool_engine(observation_restriction_config());
    engine.observation_round_tracker.repetitive_rounds = 3;
    let decision = Decision::UseTools(vec![ToolCall {
        id: "call-1".to_string(),
        name: "read_file".to_string(),
        arguments: serde_json::json!({"path":"README.md"}),
    }]);
    let llm = SequentialMockLlm::new(vec![text_response(
        "Current findings are enough to begin implementation.",
    )]);

    let action = engine
        .act(
            &decision,
            &llm,
            &[Message::user(
                "Research the API and summarize what you found",
            )],
            CycleStream::disabled(),
        )
        .await
        .expect("act should succeed");

    assert_eq!(
        action.response_text,
        "Current findings are enough to begin implementation."
    );
    assert_eq!(action.tool_results.len(), 1);
    assert!(!action.tool_results[0].success);
    assert!(action.tool_results[0]
        .output
        .contains("read-only inspection is disabled"));
    match action.next_step {
        ActionNextStep::Finish(ActionTerminal::Complete { response }) => {
            assert_eq!(
                response,
                "Current findings are enough to begin implementation."
            );
        }
        other => panic!("expected bounded synthesis terminal, got {other:?}"),
    }
}

#[tokio::test]
async fn observation_only_restriction_replans_with_mutation_only_tools() {
    let mut engine = mixed_tool_engine(observation_restriction_config());
    engine.observation_round_tracker.repetitive_rounds = 3;
    let decision = Decision::UseTools(vec![ToolCall {
        id: "call-1".to_string(),
        name: "read_file".to_string(),
        arguments: serde_json::json!({"path":"README.md"}),
    }]);
    let llm = RecordingLlm::ok(vec![
        tool_use_response(vec![ToolCall {
            id: "call-2".to_string(),
            name: "write_file".to_string(),
            arguments: serde_json::json!({"path":"x-post/README.md","content":"spec"}),
        }]),
        text_response("done after write"),
    ]);

    let action = engine
        .act(
            &decision,
            &llm,
            &[Message::user(
                "Research, then implement once you know enough.",
            )],
            CycleStream::disabled(),
        )
        .await
        .expect("act should succeed");

    assert_eq!(action.response_text, "done after write");
    assert_eq!(action.tool_results.len(), 2);
    assert_eq!(action.tool_results[0].tool_name, "read_file");
    assert!(!action.tool_results[0].success);
    assert_eq!(action.tool_results[1].tool_name, "write_file");
    assert!(action.tool_results[1].success);

    let requests = llm.requests();
    assert!(!requests.is_empty());
    assert!(requests.iter().any(|request| {
        request.tools.iter().any(|tool| tool.name == "write_file")
            && !request.tools.iter().any(|tool| tool.name == "read_file")
    }));
}

#[tokio::test]
async fn observation_only_replan_intercepts_follow_up_decompose_before_executor() {
    let mut engine = mixed_tool_engine_with_executor(
        observation_restriction_config(),
        Arc::new(ObservationMixedNoDecomposeExecutor),
    );
    engine.observation_round_tracker.repetitive_rounds = 3;
    let decision = Decision::UseTools(vec![ToolCall {
        id: "call-1".to_string(),
        name: "read_file".to_string(),
        arguments: serde_json::json!({"path":"README.md"}),
    }]);
    let llm = RecordingLlm::ok(vec![
        tool_use_response(vec![ToolCall {
            id: "decompose-1".to_string(),
            name: DECOMPOSE_TOOL_NAME.to_string(),
            arguments: serde_json::json!({
                "sub_goals": [{
                    "description": "implement the skill",
                }],
                "strategy": "Sequential"
            }),
        }]),
        text_response("implementation ready"),
    ]);

    let action = engine
        .act(
            &decision,
            &llm,
            &[Message::user(
                "Research, then break implementation into sub-goals.",
            )],
            CycleStream::disabled(),
        )
        .await
        .expect("act should succeed");

    assert_eq!(action.tool_results.len(), 1);
    assert_eq!(action.tool_results[0].tool_name, "read_file");
    assert!(!action.tool_results[0].success);
    assert!(action
        .tool_results
        .iter()
        .all(|result| result.tool_name != DECOMPOSE_TOOL_NAME));
    assert!(
        action
            .response_text
            .contains("implement the skill => skipped (below floor)"),
        "{}",
        action.response_text
    );
}

#[test]
fn update_tool_turns_increments_on_tools_with_text() {
    let mut engine = high_budget_engine();

    engine.update_tool_turns(&tool_action("still working"));

    assert_eq!(engine.consecutive_tool_turns, 1);
}

#[test]
fn update_tool_turns_resets_on_text_only() {
    let mut engine = high_budget_engine();
    engine.consecutive_tool_turns = 2;

    engine.update_tool_turns(&text_only_action("done"));

    assert_eq!(engine.consecutive_tool_turns, 0);
}

#[test]
fn update_tool_turns_increments_on_tools_only() {
    let mut engine = high_budget_engine();

    engine.update_tool_turns(&tool_action(""));

    assert_eq!(engine.consecutive_tool_turns, 1);
}

#[test]
fn update_tool_turns_increments_on_tool_continuation_without_results() {
    let mut engine = high_budget_engine();

    engine.update_tool_turns(&tool_continuation_without_results_action("still working"));

    assert_eq!(engine.consecutive_tool_turns, 1);
}

#[test]
fn update_tool_turns_resets_on_decomposition_continuation() {
    let mut engine = high_budget_engine();
    engine.consecutive_tool_turns = 2;

    engine.update_tool_turns(&decomposition_continue_action());

    assert_eq!(engine.consecutive_tool_turns, 0);
}

#[test]
fn update_tool_turns_saturating_add() {
    let mut engine = high_budget_engine();
    engine.consecutive_tool_turns = u16::MAX;

    engine.update_tool_turns(&tool_action("still working"));

    assert_eq!(engine.consecutive_tool_turns, u16::MAX);
}

#[test]
fn action_cost_from_result_charges_empty_tool_continuation() {
    let engine = high_budget_engine();
    let cost =
        engine.action_cost_from_result(&tool_continuation_without_results_action("still working"));

    assert_eq!(cost.llm_calls, 0);
    assert_eq!(cost.tool_invocations, 0);
    assert_eq!(cost.tokens, 0);
    assert_eq!(cost.cost_cents, 1);
}

#[test]
fn action_cost_from_result_keeps_decomposition_continuation_free() {
    let engine = high_budget_engine();
    let cost = engine.action_cost_from_result(&decomposition_continue_action());

    assert_eq!(cost.cost_cents, 0);
}

// --- Test 9: 3 tool calls with cap=4 → all 3 execute ---
#[tokio::test]
async fn fan_out_3_calls_within_cap_all_execute() {
    let mut engine = fan_out_engine(4);
    let calls: Vec<ToolCall> = (0..3)
        .map(|i| ToolCall {
            id: format!("call-{i}"),
            name: "read_file".to_string(),
            arguments: serde_json::json!({"path": format!("file{i}.txt")}),
        })
        .collect();
    let decision = Decision::UseTools(calls.clone());
    let context = vec![Message::user("read files")];
    let llm = SequentialMockLlm::new(vec![CompletionResponse {
        content: vec![ContentBlock::Text {
            text: "done reading".to_string(),
        }],
        tool_calls: Vec::new(),
        usage: None,
        stop_reason: None,
    }]);

    let result = engine
        .act(&decision, &llm, &context, CycleStream::disabled())
        .await
        .expect("act");

    assert_eq!(result.tool_results.len(), 3, "all 3 should execute");
}

// --- Test 10: 6 tool calls with cap=4 → first 4 execute, last 2 deferred ---
#[tokio::test]
async fn fan_out_6_calls_cap_4_defers_2() {
    let mut engine = fan_out_engine(4);
    let calls: Vec<ToolCall> = (0..6)
        .map(|i| ToolCall {
            id: format!("call-{i}"),
            name: format!("tool_{i}"),
            arguments: serde_json::json!({}),
        })
        .collect();
    let decision = Decision::UseTools(calls.clone());
    let context = vec![Message::user("do stuff")];
    let llm = SequentialMockLlm::new(vec![CompletionResponse {
        content: vec![ContentBlock::Text {
            text: "completed".to_string(),
        }],
        tool_calls: Vec::new(),
        usage: None,
        stop_reason: None,
    }]);

    let result = engine
        .act(&decision, &llm, &context, CycleStream::disabled())
        .await
        .expect("act");

    let executed: Vec<_> = result.tool_results.iter().filter(|r| r.success).collect();
    assert_eq!(executed.len(), 4, "only first 4 should execute");
    let deferred_results: Vec<_> = result
        .tool_results
        .iter()
        .filter(|r| !r.success && r.output.contains("deferred"))
        .collect();
    assert_eq!(deferred_results.len(), 2, "2 deferred as synthetic results");
    // Check that deferred signal was emitted
    let signals = engine.signals.drain_all();
    let friction: Vec<_> = signals
        .iter()
        .filter(|s| s.kind == SignalKind::Friction && s.message.contains("fan-out cap"))
        .collect();
    assert_eq!(friction.len(), 1, "fan-out friction signal emitted");
    assert_eq!(
        friction[0].metadata["decision_kind"],
        "tool_batch_guardrail"
    );
    assert_eq!(friction[0].metadata["decision"], "deferred");
    assert_eq!(
        friction[0].metadata["deferred_tools"],
        serde_json::json!(["tool_4", "tool_5"])
    );
}

// --- Test 11: Deferred message lists correct tool names ---
#[tokio::test]
async fn fan_out_deferred_message_lists_tool_names() {
    let mut engine = fan_out_engine(2);
    let calls = vec![
        ToolCall {
            id: "a".to_string(),
            name: "alpha".to_string(),
            arguments: serde_json::json!({}),
        },
        ToolCall {
            id: "b".to_string(),
            name: "beta".to_string(),
            arguments: serde_json::json!({}),
        },
        ToolCall {
            id: "c".to_string(),
            name: "gamma".to_string(),
            arguments: serde_json::json!({}),
        },
        ToolCall {
            id: "d".to_string(),
            name: "delta".to_string(),
            arguments: serde_json::json!({}),
        },
    ];

    let (execute, deferred) = engine.apply_fan_out_cap(&calls);
    assert_eq!(execute.len(), 2);
    assert_eq!(deferred.len(), 2);
    assert_eq!(deferred[0].name, "gamma");
    assert_eq!(deferred[1].name, "delta");

    let signals = engine.signals.drain_all();
    let friction = signals
        .iter()
        .find(|s| s.kind == SignalKind::Friction)
        .expect("friction signal");
    assert!(
        friction.message.contains("gamma"),
        "deferred message should list gamma: {}",
        friction.message
    );
    assert!(
        friction.message.contains("delta"),
        "deferred message should list delta: {}",
        friction.message
    );
}

// --- Test 12: Cap=1 forces strictly sequential tool execution ---
#[tokio::test]
async fn fan_out_cap_1_forces_sequential() {
    let mut engine = fan_out_engine(1);
    let calls: Vec<ToolCall> = (0..3)
        .map(|i| ToolCall {
            id: format!("call-{i}"),
            name: format!("tool_{i}"),
            arguments: serde_json::json!({}),
        })
        .collect();
    let decision = Decision::UseTools(calls.clone());
    let context = vec![Message::user("do stuff")];
    let llm = SequentialMockLlm::new(vec![CompletionResponse {
        content: vec![ContentBlock::Text {
            text: "done".to_string(),
        }],
        tool_calls: Vec::new(),
        usage: None,
        stop_reason: None,
    }]);

    let result = engine
        .act(&decision, &llm, &context, CycleStream::disabled())
        .await
        .expect("act");

    let executed: Vec<_> = result.tool_results.iter().filter(|r| r.success).collect();
    assert_eq!(executed.len(), 1, "cap=1 should execute exactly 1 tool");
    let deferred_results: Vec<_> = result
        .tool_results
        .iter()
        .filter(|r| !r.success && r.output.contains("deferred"))
        .collect();
    assert_eq!(
        deferred_results.len(),
        2,
        "cap=1 with 3 calls should defer 2"
    );
}

// --- Test 11b: Deferred tools injected as synthetic tool results ---
#[tokio::test]
async fn deferred_tools_appear_in_synthesis_results() {
    let mut engine = fan_out_engine(1);
    let calls = vec![
        ToolCall {
            id: "a".to_string(),
            name: "alpha".to_string(),
            arguments: serde_json::json!({}),
        },
        ToolCall {
            id: "b".to_string(),
            name: "beta".to_string(),
            arguments: serde_json::json!({}),
        },
    ];

    // LLM returns empty so we fall through to synthesize_tool_fallback
    let llm = SequentialMockLlm::new(vec![CompletionResponse {
        content: vec![ContentBlock::Text {
            text: "summary".to_string(),
        }],
        tool_calls: Vec::new(),
        usage: None,
        stop_reason: None,
    }]);

    let decision = Decision::UseTools(calls);
    let context = vec![Message::user("do things")];
    let result = engine
        .act(&decision, &llm, &context, CycleStream::disabled())
        .await
        .expect("act");

    // Should have 1 executed + 1 deferred-as-synthetic = 2 tool results
    assert_eq!(
        result.tool_results.len(),
        2,
        "deferred tool should appear as synthetic tool result"
    );
    let deferred_result = result
        .tool_results
        .iter()
        .find(|r| r.tool_name == "beta")
        .expect("beta should be in results");
    assert!(
        !deferred_result.success,
        "deferred result should be marked as not successful"
    );
    assert!(
        deferred_result.output.contains("deferred"),
        "deferred result should mention deferral: {}",
        deferred_result.output
    );
}

// --- Test 12b: Continuation tool calls also capped by fan-out ---
#[tokio::test]
async fn continuation_tool_calls_capped_by_fan_out() {
    let mut engine = fan_out_engine(2);

    // Initial: 2 calls (within cap). Continuation response has 4 more calls.
    let initial_calls: Vec<ToolCall> = (0..2)
        .map(|i| ToolCall {
            id: format!("init-{i}"),
            name: "read_file".to_string(),
            arguments: serde_json::json!({"path": format!("f{i}.txt")}),
        })
        .collect();

    // Mock LLM: first call returns 4 tool calls (should be capped to 2),
    // second call returns 2 more (capped to 2), third returns final text.
    let continuation_calls: Vec<ToolCall> = (0..4)
        .map(|i| ToolCall {
            id: format!("cont-{i}"),
            name: "read_file".to_string(),
            arguments: serde_json::json!({"path": format!("c{i}.txt")}),
        })
        .collect();
    let llm = SequentialMockLlm::new(vec![
        // First continuation: returns 4 tool calls
        CompletionResponse {
            content: Vec::new(),
            tool_calls: continuation_calls,
            usage: None,
            stop_reason: Some("tool_use".to_string()),
        },
        // Second continuation: returns text (done)
        CompletionResponse {
            content: vec![ContentBlock::Text {
                text: "all done".to_string(),
            }],
            tool_calls: Vec::new(),
            usage: None,
            stop_reason: None,
        },
    ]);

    let decision = Decision::UseTools(initial_calls);
    let context = vec![Message::user("read files")];
    let result = engine
        .act(&decision, &llm, &context, CycleStream::disabled())
        .await
        .expect("act");

    // Initial 2 + capped 2 executed + 2 deferred (synthetic) = 6 total
    assert_eq!(
        result.tool_results.len(),
        6,
        "continuation tool calls should include capped + deferred: got {}",
        result.tool_results.len()
    );

    // The last 2 entries are synthetic deferred results (not successfully executed)
    let deferred_results: Vec<_> = result.tool_results.iter().filter(|r| !r.success).collect();
    assert_eq!(
        deferred_results.len(),
        2,
        "expected 2 deferred tool results, got {}",
        deferred_results.len()
    );
    for r in &deferred_results {
        assert!(
            r.output.contains("deferred"),
            "deferred result should mention deferral: {}",
            r.output
        );
    }
}

// --- Tool result truncation via execute_tool_calls ---
#[tokio::test]
async fn tool_results_truncated_by_execute_tool_calls() {
    let config = BudgetConfig {
        max_tool_result_bytes: 100,
        ..BudgetConfig::default()
    };
    let mut engine = LoopEngine::builder()
        .budget(BudgetTracker::new(config, 0, 0))
        .context(ContextCompactor::new(2048, 256))
        .max_iterations(3)
        .tool_executor(Arc::new(LargeOutputToolExecutor { output_size: 500 }))
        .synthesis_instruction("Summarize".to_string())
        .build()
        .expect("build");

    let calls = vec![ToolCall {
        id: "1".to_string(),
        name: "read_file".to_string(),
        arguments: serde_json::json!({"path": "big.txt"}),
    }];
    let results = engine.execute_tool_calls(&calls).await.expect("execute");
    assert_eq!(results.len(), 1);
    assert!(
        results[0].output.contains("[truncated"),
        "output should be truncated: {}",
        &results[0].output[..100.min(results[0].output.len())]
    );
}

#[tokio::test]
async fn tool_results_not_truncated_within_limit() {
    let config = BudgetConfig {
        max_tool_result_bytes: 1000,
        ..BudgetConfig::default()
    };
    let mut engine = LoopEngine::builder()
        .budget(BudgetTracker::new(config, 0, 0))
        .context(ContextCompactor::new(2048, 256))
        .max_iterations(3)
        .tool_executor(Arc::new(LargeOutputToolExecutor { output_size: 500 }))
        .synthesis_instruction("Summarize".to_string())
        .build()
        .expect("build");

    let calls = vec![ToolCall {
        id: "1".to_string(),
        name: "read_file".to_string(),
        arguments: serde_json::json!({"path": "small.txt"}),
    }];
    let results = engine.execute_tool_calls(&calls).await.expect("execute");
    assert_eq!(results.len(), 1);
    assert!(
        !results[0].output.contains("[truncated"),
        "output within limit should NOT be truncated"
    );
    assert_eq!(results[0].output.len(), 500);
}
