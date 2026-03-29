use super::{to_tool_result, ToolRegistry};
use crate::experiment_tool::run_experiment_tool_definition;
use crate::tool_trait::{Tool, ToolContext};
use async_trait::async_trait;
use fx_kernel::act::{ToolCacheability, ToolResult};
use fx_kernel::cancellation::CancellationToken;
use fx_llm::{ToolCall, ToolDefinition};
use std::sync::Arc;

pub(super) fn register_tools(registry: &mut ToolRegistry, context: &Arc<ToolContext>) {
    registry.register(RunExperimentTool::new(context));
}

struct RunExperimentTool {
    context: Arc<ToolContext>,
}

impl RunExperimentTool {
    fn new(context: &Arc<ToolContext>) -> Self {
        Self {
            context: Arc::clone(context),
        }
    }
}

#[async_trait]
impl Tool for RunExperimentTool {
    fn name(&self) -> &'static str {
        "run_experiment"
    }

    fn definition(&self) -> ToolDefinition {
        run_experiment_tool_definition()
    }

    async fn execute(&self, call: &ToolCall, _cancel: Option<&CancellationToken>) -> ToolResult {
        to_tool_result(
            &call.id,
            self.name(),
            self.context.handle_run_experiment(&call.arguments).await,
        )
    }

    fn is_available(&self) -> bool {
        self.context.experiment.is_some()
    }

    fn cacheability(&self) -> ToolCacheability {
        ToolCacheability::SideEffect
    }

    fn action_category(&self) -> &'static str {
        "tool_call"
    }
}
