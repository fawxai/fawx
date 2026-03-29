use super::{parse_args, to_tool_result, ToolRegistry};
use crate::tool_trait::{Tool, ToolContext};
use async_trait::async_trait;
use fx_kernel::act::{ToolCacheability, ToolResult};
use fx_kernel::cancellation::CancellationToken;
use fx_llm::{ToolCall, ToolDefinition};
use serde::Deserialize;
use std::sync::Arc;

const DEFAULT_MEMORY_SEARCH_RESULTS: usize = 5;

pub(super) fn register_tools(registry: &mut ToolRegistry, context: &Arc<ToolContext>) {
    registry.register(MemoryWriteTool::new(context));
    registry.register(MemoryReadTool::new(context));
    registry.register(MemoryListTool::new(context));
    registry.register(MemoryDeleteTool::new(context));
    registry.register(MemorySearchTool::new(context));
}

struct MemoryWriteTool {
    context: Arc<ToolContext>,
}

struct MemoryReadTool {
    context: Arc<ToolContext>,
}

struct MemoryListTool {
    context: Arc<ToolContext>,
}

struct MemoryDeleteTool {
    context: Arc<ToolContext>,
}

struct MemorySearchTool {
    context: Arc<ToolContext>,
}

impl MemoryWriteTool {
    fn new(context: &Arc<ToolContext>) -> Self {
        Self {
            context: Arc::clone(context),
        }
    }
}

impl MemoryReadTool {
    fn new(context: &Arc<ToolContext>) -> Self {
        Self {
            context: Arc::clone(context),
        }
    }
}

impl MemoryListTool {
    fn new(context: &Arc<ToolContext>) -> Self {
        Self {
            context: Arc::clone(context),
        }
    }
}

impl MemoryDeleteTool {
    fn new(context: &Arc<ToolContext>) -> Self {
        Self {
            context: Arc::clone(context),
        }
    }
}

impl MemorySearchTool {
    fn new(context: &Arc<ToolContext>) -> Self {
        Self {
            context: Arc::clone(context),
        }
    }
}

#[async_trait]
impl Tool for MemoryWriteTool {
    fn name(&self) -> &'static str {
        "memory_write"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "Store a fact in persistent memory. Use for user preferences, project context, important decisions, or anything worth remembering across sessions."
                .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "key": { "type": "string" },
                    "value": { "type": "string" }
                },
                "required": ["key", "value"]
            }),
        }
    }

    async fn execute(&self, call: &ToolCall, _cancel: Option<&CancellationToken>) -> ToolResult {
        to_tool_result(
            &call.id,
            self.name(),
            self.context.handle_memory_write(&call.arguments),
        )
    }

    fn is_available(&self) -> bool {
        self.context.memory.is_some()
    }

    fn cacheability(&self) -> ToolCacheability {
        ToolCacheability::SideEffect
    }

    fn action_category(&self) -> &'static str {
        "tool_call"
    }
}

#[async_trait]
impl Tool for MemoryReadTool {
    fn name(&self) -> &'static str {
        "memory_read"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "Retrieve a stored fact from persistent memory.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "key": { "type": "string" }
                },
                "required": ["key"]
            }),
        }
    }

    async fn execute(&self, call: &ToolCall, _cancel: Option<&CancellationToken>) -> ToolResult {
        to_tool_result(
            &call.id,
            self.name(),
            self.context.handle_memory_read(&call.arguments),
        )
    }

    fn is_available(&self) -> bool {
        self.context.memory.is_some()
    }

    fn cacheability(&self) -> ToolCacheability {
        ToolCacheability::Cacheable
    }

    fn action_category(&self) -> &'static str {
        "tool_call"
    }
}

#[async_trait]
impl Tool for MemoryListTool {
    fn name(&self) -> &'static str {
        "memory_list"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "List all stored memory keys with value previews.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
        }
    }

    async fn execute(&self, call: &ToolCall, _cancel: Option<&CancellationToken>) -> ToolResult {
        to_tool_result(&call.id, self.name(), self.context.handle_memory_list())
    }

    fn is_available(&self) -> bool {
        self.context.memory.is_some()
    }

    fn cacheability(&self) -> ToolCacheability {
        ToolCacheability::Cacheable
    }

    fn action_category(&self) -> &'static str {
        "tool_call"
    }
}

#[async_trait]
impl Tool for MemoryDeleteTool {
    fn name(&self) -> &'static str {
        "memory_delete"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "Remove a stored fact from persistent memory.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "key": { "type": "string" }
                },
                "required": ["key"]
            }),
        }
    }

    async fn execute(&self, call: &ToolCall, _cancel: Option<&CancellationToken>) -> ToolResult {
        to_tool_result(
            &call.id,
            self.name(),
            self.context.handle_memory_delete(&call.arguments),
        )
    }

    fn is_available(&self) -> bool {
        self.context.memory.is_some()
    }

    fn cacheability(&self) -> ToolCacheability {
        ToolCacheability::SideEffect
    }

    fn action_category(&self) -> &'static str {
        "tool_call"
    }
}

#[async_trait]
impl Tool for MemorySearchTool {
    fn name(&self) -> &'static str {
        "memory_search"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "Search agent memory by meaning. Finds semantically related memories even without exact keyword matches."
                .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Natural language search query"
                    },
                    "max_results": {
                        "type": "integer",
                        "description": "Maximum results to return (default: 5)"
                    }
                },
                "required": ["query"]
            }),
        }
    }

    async fn execute(&self, call: &ToolCall, _cancel: Option<&CancellationToken>) -> ToolResult {
        to_tool_result(
            &call.id,
            self.name(),
            self.context.handle_memory_search(&call.arguments),
        )
    }

    fn is_available(&self) -> bool {
        self.context.memory.is_some()
    }

    fn cacheability(&self) -> ToolCacheability {
        ToolCacheability::Cacheable
    }

    fn action_category(&self) -> &'static str {
        "tool_call"
    }
}

#[derive(Deserialize)]
struct MemoryWriteArgs {
    key: String,
    value: String,
}

#[derive(Deserialize)]
struct MemoryReadArgs {
    key: String,
}

#[derive(Deserialize)]
struct MemoryDeleteArgs {
    key: String,
}

#[derive(Deserialize)]
struct MemorySearchArgs {
    query: String,
    max_results: Option<usize>,
}

struct MemorySearchResult {
    key: String,
    value: String,
    score: Option<f32>,
}

impl ToolContext {
    pub(crate) fn handle_memory_write(&self, args: &serde_json::Value) -> Result<String, String> {
        let parsed: MemoryWriteArgs = parse_args(args)?;
        let memory = self.memory.as_ref().ok_or("memory not configured")?;
        let mut guard = memory.lock().map_err(|error| format!("{error}"))?;
        guard.write(&parsed.key, &parsed.value)?;
        drop(guard);
        self.upsert_embedding_memory(&parsed.key, &parsed.value)?;
        Ok(format!("stored key '{}'", parsed.key))
    }

    pub(crate) fn handle_memory_read(&self, args: &serde_json::Value) -> Result<String, String> {
        let parsed: MemoryReadArgs = parse_args(args)?;
        let memory = self.memory.as_ref().ok_or("memory not configured")?;
        let mut guard = memory.lock().map_err(|error| format!("{error}"))?;
        let value = guard.read(&parsed.key);
        if value.is_some() {
            guard.touch(&parsed.key)?;
        }
        match value {
            Some(value) => Ok(value),
            None => Ok(format!("key '{}' not found", parsed.key)),
        }
    }

    pub(crate) fn handle_memory_list(&self) -> Result<String, String> {
        let memory = self.memory.as_ref().ok_or("memory not configured")?;
        let guard = memory.lock().map_err(|error| format!("{error}"))?;
        let entries = guard.list();
        if entries.is_empty() {
            return Ok("no memories stored".to_string());
        }
        Ok(format_memory_list(&entries))
    }

    pub(crate) fn handle_memory_search(&self, args: &serde_json::Value) -> Result<String, String> {
        let parsed: MemorySearchArgs = parse_args(args)?;
        let max_results = parsed.max_results.unwrap_or(DEFAULT_MEMORY_SEARCH_RESULTS);
        let results = self.memory_search_results(&parsed.query, max_results)?;
        self.touch_memory_search_results(&results)?;
        Ok(format_memory_search_results(&parsed.query, &results))
    }

    pub(crate) fn handle_memory_delete(&self, args: &serde_json::Value) -> Result<String, String> {
        let parsed: MemoryDeleteArgs = parse_args(args)?;
        let memory = self.memory.as_ref().ok_or("memory not configured")?;
        let mut guard = memory.lock().map_err(|error| format!("{error}"))?;
        let deleted = guard.delete(&parsed.key);
        drop(guard);
        if deleted {
            self.remove_embedding_memory(&parsed.key)?;
            Ok(format!("deleted key '{}'", parsed.key))
        } else {
            Ok(format!("key '{}' not found", parsed.key))
        }
    }

    fn memory_search_results(
        &self,
        query: &str,
        max_results: usize,
    ) -> Result<Vec<MemorySearchResult>, String> {
        if let Some(index) = &self.embedding_index {
            match self.semantic_memory_search(index, query, max_results) {
                Ok(results) => return Ok(results),
                Err(error) => {
                    tracing::warn!(
                        error = %error,
                        "semantic search failed; falling back to keyword search"
                    );
                }
            }
        }
        self.keyword_memory_search(query, max_results)
    }

    fn touch_memory_search_results(&self, results: &[MemorySearchResult]) -> Result<(), String> {
        let memory = self.memory.as_ref().ok_or("memory not configured")?;
        let mut guard = memory.lock().map_err(|error| format!("{error}"))?;
        results
            .iter()
            .try_for_each(|result| guard.touch(&result.key))
    }

    fn semantic_memory_search(
        &self,
        index: &Arc<std::sync::Mutex<fx_memory::embedding_index::EmbeddingIndex>>,
        query: &str,
        max_results: usize,
    ) -> Result<Vec<MemorySearchResult>, String> {
        let hits = index
            .lock()
            .map_err(|error| format!("{error}"))?
            .search(query, max_results)
            .map_err(|error| error.to_string())?;
        let memory = self.memory.as_ref().ok_or("memory not configured")?;
        let guard = memory.lock().map_err(|error| format!("{error}"))?;
        Ok(hits
            .into_iter()
            .filter_map(|(key, score)| {
                guard.read(&key).map(|value| MemorySearchResult {
                    key,
                    value,
                    score: Some(score),
                })
            })
            .collect())
    }

    fn keyword_memory_search(
        &self,
        query: &str,
        max_results: usize,
    ) -> Result<Vec<MemorySearchResult>, String> {
        let memory = self.memory.as_ref().ok_or("memory not configured")?;
        let guard = memory.lock().map_err(|error| format!("{error}"))?;
        Ok(guard
            .search_relevant(query, max_results)
            .into_iter()
            .map(|(key, value)| MemorySearchResult {
                key,
                value,
                score: None,
            })
            .collect())
    }

    fn upsert_embedding_memory(&self, key: &str, value: &str) -> Result<(), String> {
        let Some(index) = &self.embedding_index else {
            return Ok(());
        };
        index
            .lock()
            .map_err(|error| format!("{error}"))?
            .upsert(key, value)
            .map_err(|error| error.to_string())
    }

    fn remove_embedding_memory(&self, key: &str) -> Result<(), String> {
        let Some(index) = &self.embedding_index else {
            return Ok(());
        };
        index
            .lock()
            .map_err(|error| format!("{error}"))?
            .remove(key);
        Ok(())
    }
}

fn format_memory_list(entries: &[(String, String)]) -> String {
    entries
        .iter()
        .map(|(key, value)| format!("- {key}: {}", truncate_preview(value, 80)))
        .collect::<Vec<_>>()
        .join("\n")
}

fn format_memory_search_results(query: &str, results: &[MemorySearchResult]) -> String {
    if results.is_empty() {
        return format!("No relevant memories found for: {query}");
    }
    let items = results
        .iter()
        .enumerate()
        .map(|(index, result)| format_memory_search_item(index + 1, result))
        .collect::<Vec<_>>()
        .join("\n\n");
    format!("Found {} relevant memories:\n\n{items}", results.len())
}

fn format_memory_search_item(index: usize, result: &MemorySearchResult) -> String {
    let header = match result.score {
        Some(score) => format!("{index}. [{}] (score: {score:.2})", result.key),
        None => format!("{index}. [{}]", result.key),
    };
    let value = indent_memory_value(&result.value);
    format!("{header}\n{value}")
}

fn indent_memory_value(value: &str) -> String {
    value
        .lines()
        .map(|line| format!("   {line}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn truncate_preview(value: &str, max_len: usize) -> String {
    if value.len() <= max_len {
        return value.to_string();
    }
    let mut end = max_len;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}...", &value[..end])
}
