use super::{to_tool_result, ToolRegistry};
use crate::tool_trait::{Tool, ToolContext};
use async_trait::async_trait;
use fx_kernel::act::ToolResult;
use fx_kernel::cancellation::CancellationToken;
use fx_llm::{ToolCall, ToolDefinition};
use std::sync::Arc;

pub(super) fn register_tools(registry: &mut ToolRegistry, context: &Arc<ToolContext>) {
    registry.register(AnalyzeSignalsTool::new(context));
    registry.register(ProposeImprovementTool::new(context));
}

struct AnalyzeSignalsTool {
    context: Arc<ToolContext>,
}

struct ProposeImprovementTool {
    context: Arc<ToolContext>,
}

impl AnalyzeSignalsTool {
    fn new(context: &Arc<ToolContext>) -> Self {
        Self {
            context: Arc::clone(context),
        }
    }
}

impl ProposeImprovementTool {
    fn new(context: &Arc<ToolContext>) -> Self {
        Self {
            context: Arc::clone(context),
        }
    }
}

#[async_trait]
impl Tool for AnalyzeSignalsTool {
    fn name(&self) -> &'static str {
        "analyze_signals"
    }

    fn definition(&self) -> ToolDefinition {
        crate::improvement_tools::improvement_tool_definitions()
            .into_iter()
            .find(|definition| definition.name == self.name())
            .unwrap_or_else(|| ToolDefinition {
                name: self.name().to_string(),
                description: "Analyze system signals for potential improvements.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {},
                    "required": []
                }),
            })
    }

    async fn execute(&self, call: &ToolCall, _cancel: Option<&CancellationToken>) -> ToolResult {
        let output = match &self.context.improvement {
            Some(state) if state.config.enabled => {
                crate::improvement_tools::handle_analyze_signals(state, &call.arguments).await
            }
            _ => Err("improvement tools not enabled".to_string()),
        };
        to_tool_result(&call.id, self.name(), output)
    }

    fn is_available(&self) -> bool {
        self.context
            .improvement
            .as_ref()
            .is_some_and(|state| state.config.enabled)
    }
}

#[async_trait]
impl Tool for ProposeImprovementTool {
    fn name(&self) -> &'static str {
        "propose_improvement"
    }

    fn definition(&self) -> ToolDefinition {
        crate::improvement_tools::improvement_tool_definitions()
            .into_iter()
            .find(|definition| definition.name == self.name())
            .unwrap_or_else(|| ToolDefinition {
                name: self.name().to_string(),
                description: "Propose a concrete improvement from system signals.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {},
                    "required": []
                }),
            })
    }

    async fn execute(&self, call: &ToolCall, _cancel: Option<&CancellationToken>) -> ToolResult {
        let output = match &self.context.improvement {
            Some(state) if state.config.enabled => {
                crate::improvement_tools::handle_propose_improvement(
                    state,
                    &call.arguments,
                    &self.context.working_dir,
                )
                .await
            }
            _ => Err("improvement tools not enabled".to_string()),
        };
        to_tool_result(&call.id, self.name(), output)
    }

    fn is_available(&self) -> bool {
        self.context
            .improvement
            .as_ref()
            .is_some_and(|state| state.config.enabled)
    }
}
