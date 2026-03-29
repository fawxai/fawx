use super::{to_tool_result, ToolRegistry};
use crate::tool_trait::{Tool, ToolContext};
use async_trait::async_trait;
use fx_kernel::act::{ToolCacheability, ToolResult};
use fx_kernel::cancellation::CancellationToken;
use fx_llm::{ToolCall, ToolDefinition};
use std::sync::Arc;

pub(super) fn register_tools(registry: &mut ToolRegistry, context: &Arc<ToolContext>) {
    registry.register(NodeRunTool::new(context));
}

struct NodeRunTool {
    context: Arc<ToolContext>,
}

impl NodeRunTool {
    fn new(context: &Arc<ToolContext>) -> Self {
        Self {
            context: Arc::clone(context),
        }
    }
}

#[async_trait]
impl Tool for NodeRunTool {
    fn name(&self) -> &'static str {
        "node_run"
    }

    fn definition(&self) -> ToolDefinition {
        crate::node_run::node_run_tool_definition()
    }

    async fn execute(&self, call: &ToolCall, _cancel: Option<&CancellationToken>) -> ToolResult {
        let output = match &self.context.node_run {
            Some(state) => crate::node_run::handle_node_run(state, &call.arguments).await,
            None => Err("node_run not configured".to_string()),
        };
        to_tool_result(&call.id, self.name(), output)
    }

    fn is_available(&self) -> bool {
        self.context.node_run.is_some()
    }

    fn cacheability(&self) -> ToolCacheability {
        ToolCacheability::SideEffect
    }

    fn action_category(&self) -> &'static str {
        "tool_call"
    }
}
