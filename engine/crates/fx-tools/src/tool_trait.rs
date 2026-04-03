use crate::{ExperimentRegistrar, ExperimentToolState, NodeRunState};
use async_trait::async_trait;
use fx_config::manager::ConfigManager;
use fx_consensus::ProgressCallback;
use fx_core::memory::MemoryStore;
use fx_core::runtime_info::RuntimeInfo;
use fx_core::self_modify::SelfModifyConfig;
use fx_kernel::act::{
    JournalAction, SubGoalToolRoutingRequest, ToolCacheability, ToolCallClassification, ToolResult,
};
use fx_kernel::budget::BudgetConfig as KernelBudgetConfig;
use fx_kernel::cancellation::CancellationToken;
use fx_kernel::ProcessRegistry;
use fx_kernel::ToolAuthoritySurface;
use fx_llm::{ToolCall, ToolDefinition};
use fx_memory::embedding_index::EmbeddingIndex;
use fx_subagent::SubagentControl;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

#[cfg(feature = "improvement")]
use crate::ImprovementToolsState;

const DEFAULT_MAX_FILE_SIZE: u64 = 10 * 1024 * 1024;
const DEFAULT_MAX_READ_SIZE: u64 = 1024 * 1024;
const DEFAULT_COMMAND_TIMEOUT_SECS: u64 = 30;

#[derive(Debug, Clone)]
pub struct ToolConfig {
    pub max_file_size: u64,
    pub max_read_size: u64,
    pub search_exclude: Vec<String>,
    pub command_timeout: Duration,
    pub jail_to_working_dir: bool,
    pub allow_outside_workspace_reads: bool,
}

impl Default for ToolConfig {
    fn default() -> Self {
        Self {
            max_file_size: DEFAULT_MAX_FILE_SIZE,
            max_read_size: DEFAULT_MAX_READ_SIZE,
            search_exclude: Vec::new(),
            command_timeout: Duration::from_secs(DEFAULT_COMMAND_TIMEOUT_SECS),
            jail_to_working_dir: true,
            allow_outside_workspace_reads: false,
        }
    }
}

#[derive(Clone)]
pub struct ToolContext {
    pub(crate) working_dir: PathBuf,
    pub(crate) config: ToolConfig,
    pub(crate) process_registry: Arc<ProcessRegistry>,
    pub(crate) memory: Option<Arc<Mutex<dyn MemoryStore>>>,
    pub(crate) embedding_index: Option<Arc<Mutex<EmbeddingIndex>>>,
    pub(crate) runtime_info: Option<Arc<RwLock<RuntimeInfo>>>,
    pub(crate) self_modify: Option<SelfModifyConfig>,
    pub(crate) config_manager: Option<Arc<Mutex<ConfigManager>>>,
    pub(crate) protected_branches: Vec<String>,
    pub(crate) kernel_budget: KernelBudgetConfig,
    pub(crate) start_time: Instant,
    pub(crate) subagent_control: Option<Arc<dyn SubagentControl>>,
    pub(crate) experiment: Option<ExperimentToolState>,
    pub(crate) experiment_progress: Option<ProgressCallback>,
    pub(crate) experiment_registrar: Option<Arc<dyn ExperimentRegistrar>>,
    pub(crate) background_experiments: bool,
    pub(crate) node_run: Option<NodeRunState>,
    #[cfg(feature = "improvement")]
    pub(crate) improvement: Option<ImprovementToolsState>,
}

#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &'static str;

    fn definition(&self) -> ToolDefinition;

    async fn execute(&self, call: &ToolCall, cancel: Option<&CancellationToken>) -> ToolResult;

    fn is_available(&self) -> bool {
        true
    }

    fn cacheability(&self) -> ToolCacheability {
        ToolCacheability::NeverCache
    }

    fn classify_call(&self, call: &ToolCall) -> ToolCallClassification {
        let _ = call;
        match self.cacheability() {
            ToolCacheability::SideEffect => ToolCallClassification::Mutation,
            ToolCacheability::Cacheable | ToolCacheability::NeverCache => {
                ToolCallClassification::Observation
            }
        }
    }

    fn journal_action(&self, _call: &ToolCall, _result: &ToolResult) -> Option<JournalAction> {
        None
    }

    fn action_category(&self) -> &'static str {
        "unknown"
    }

    fn authority_surface(&self, call: &ToolCall) -> ToolAuthoritySurface {
        let _ = call;
        ToolAuthoritySurface::Other
    }

    fn route_sub_goal(
        &self,
        request: &SubGoalToolRoutingRequest,
        call_id: &str,
    ) -> Option<ToolCall> {
        let requested_name = request.required_tools.first()?;
        if requested_name != self.name() {
            return None;
        }

        let definition = self.definition();
        let required = definition
            .parameters
            .get("required")
            .and_then(serde_json::Value::as_array)?;
        if !required.is_empty() {
            return None;
        }

        Some(ToolCall {
            id: call_id.to_string(),
            name: self.name().to_string(),
            arguments: serde_json::json!({}),
        })
    }
}
