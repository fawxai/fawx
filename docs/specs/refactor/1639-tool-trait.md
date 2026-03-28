# Spec: #1639 — Tool Trait: Replace FawxToolExecutor Monolith

## Status
Foundation laid. `ToolExecutor` trait has `classify_call()`, `cacheability()`, `route_sub_goal_call()`. But these are on the monolithic executor, not on individual tools. The 27-arm dispatch match is intact.

## Goal
Decompose `FawxToolExecutor` (5,938 lines) into individual `Tool` trait objects. Each tool self-describes its name, schema, cacheability, classification, and journal hints. The executor becomes a registry that delegates to registered tools.

## Current State (codex/provider-owned-loop-refactor branch)

File: `engine/crates/fx-tools/src/tools.rs` (5,938 lines)

### The monolith

**Main dispatch (line 305, unchanged post-merge):**
```rust
let output = match call.name.as_str() {
    "read_file" => self.read_file(call, cancel).await,
    "write_file" => self.write_file(call, cancel).await,
    "edit_file" => self.edit_file(call, cancel).await,
    "search_text" => self.search_text(call, cancel).await,
    "list_directory" => self.list_directory(call, cancel).await,
    "run_command" => self.run_command(call, cancel).await,
    // ... 27 arms total
    other => Err(ToolError::NotFound(other.to_string())),
};
```

**Cacheability match (line 264):**
```rust
fn cacheability_for(tool_name: &str) -> ToolCacheability {
    match tool_name {
        "read_file" | "search_text" | "list_directory" => ToolCacheability::Cacheable,
        "write_file" | "create_file" | "edit_file" | "delete_file" | "run_command" | ... => ToolCacheability::SideEffect,
        _ => ToolCacheability::NeverCache,
    }
}
```

**Classification override (line 284):**
```rust
fn classify_call_impl(call: &ToolCall) -> ToolCallClassification {
    match call.name.as_str() {
        "run_command" => { /* checks shell syntax */ }
        _ => { /* delegates to cacheability */ }
    }
}
```

### Reference pattern: WASM Skills

WASM skills already implement a `Skill` trait pattern where each skill is a self-describing object. The weather skill manifest proves the metadata pipeline works end-to-end (`manifest.toml` → `ManifestTool` → `ToolDefinition.parameters["x-fawx-direct-utility"]`). The `Tool` trait should follow this same pattern for built-in tools.

### What ToolExecutor trait already has (fx-kernel/src/act.rs)

```rust
pub trait ToolExecutor: Send + Sync + std::fmt::Debug {
    async fn execute_tools(&self, calls: &[ToolCall], cancel: Option<&CancellationToken>) -> Result<Vec<ToolResult>, ToolExecutorError>;
    fn concurrency_policy(&self) -> ConcurrencyPolicy;
    fn tool_definitions(&self) -> Vec<ToolDefinition>;
    fn cacheability(&self, tool_name: &str) -> ToolCacheability;
    fn classify_call(&self, call: &ToolCall) -> ToolCallClassification;
    fn route_sub_goal_call(&self, request: &SubGoalToolRoutingRequest, call_id: &str) -> Option<ToolCall>;
    fn clear_cache(&self);
    fn cache_stats(&self) -> Option<ToolCacheStats>;
}
```

## Design

### New `Tool` trait

```rust
pub trait Tool: Send + Sync {
    /// Tool name as exposed to the model.
    fn name(&self) -> &str;

    /// JSON Schema tool definition.
    fn definition(&self) -> ToolDefinition;

    /// Execute the tool call.
    async fn execute(&self, call: &ToolCall, cancel: Option<&CancellationToken>) -> Result<ToolResult, ToolError>;

    /// Cache classification for this tool.
    fn cacheability(&self) -> ToolCacheability { ToolCacheability::NeverCache }

    /// Effect classification for a specific call.
    fn classify_call(&self, call: &ToolCall) -> ToolCallClassification {
        match self.cacheability() {
            ToolCacheability::SideEffect => ToolCallClassification::Mutation,
            _ => ToolCallClassification::Observation,
        }
    }

    /// Ripcord journal hint for a completed call.
    fn journal_action(&self, call: &ToolCall, result: &ToolResult) -> Option<JournalAction> { None }

    /// Ripcord action category for tripwire threshold counting.
    fn action_category(&self) -> &str { "unknown" }

    /// Concurrency policy for this tool (default: no restrictions).
    fn concurrency_policy(&self) -> Option<ConcurrencyPolicy> { None }

    /// Whether this tool can be safely routed from a sub-goal.
    fn route_sub_goal(&self, request: &SubGoalToolRoutingRequest, call_id: &str) -> Option<ToolCall> { None }
}
```

### Refactored `FawxToolExecutor`

Becomes a registry:
```rust
pub struct FawxToolExecutor {
    tools: HashMap<String, Box<dyn Tool>>,
    // shared state: working directory, sandbox config, etc.
    context: ToolContext,
}

impl FawxToolExecutor {
    pub fn register(&mut self, tool: Box<dyn Tool>) { ... }

    // ToolExecutor trait delegates to registered tools
}
```

### Individual tool structs

Each tool becomes its own struct. Example:
```rust
pub struct ReadFileTool { context: Arc<ToolContext> }

impl Tool for ReadFileTool {
    fn name(&self) -> &str { "read_file" }
    fn cacheability(&self) -> ToolCacheability { ToolCacheability::Cacheable }
    fn definition(&self) -> ToolDefinition { /* current read_file definition */ }
    async fn execute(&self, call: &ToolCall, cancel: ...) -> Result<ToolResult, ToolError> {
        /* current self.read_file() body */
    }
}
```

### Shared context

Tools that need shared state (working directory, sandbox config, executor permissions) receive an `Arc<ToolContext>`:
```rust
pub struct ToolContext {
    pub working_dir: PathBuf,
    pub sandbox_config: SandboxConfig,
    pub permissions: PermissionsConfig,
    pub max_read_size: u64,
    // ... other shared state currently on FawxToolExecutor
}
```

## Deliverables

1. Define `Tool` trait in `engine/crates/fx-tools/src/tool_trait.rs`
2. Define `ToolContext` shared state struct
3. Create one file per tool (or per tool group for closely related tools):
   - `tools/filesystem.rs` — read_file, write_file, edit_file, create_file, delete_file, list_directory, search_text
   - `tools/shell.rs` — run_command
   - `tools/git.rs` — git operations (currently in `git_skill.rs`)
   - `tools/web.rs` — web_search, web_fetch, browser
   - `tools/session.rs` — session tools (currently in `session_tools.rs`)
   - (group remaining tools logically)
4. Refactor `FawxToolExecutor` to registry pattern — dispatch via `self.tools.get(call.name)`
5. Delete `cacheability_for()`, `classify_call_impl()` free functions — these become trait methods
6. All existing tests pass
7. `tool_definitions()` returns definitions from registered tools, not a manual Vec

## Cascade effects (handled by follow-up issues, not this spec)
- #1638: `extract_journal_action` and `tool_to_action_category` in ripcord can delegate to `tool.journal_action()` and `tool.action_category()`
- #1515: Ripcord journal coverage improves automatically since every tool now has a `journal_action()` method
- #1171: WASM skills already implement a compatible pattern; unification becomes straightforward

## Files to modify
- `engine/crates/fx-tools/src/tools.rs` (decompose)
- `engine/crates/fx-tools/src/tool_trait.rs` (new — trait + context)
- `engine/crates/fx-tools/src/tools/*.rs` (new — individual tools)
- `engine/crates/fx-kernel/src/act.rs` (may need `JournalAction` re-export or shared type)
- `engine/crates/fx-ripcord/src/evaluator.rs` (update to use trait methods — or defer to #1638)

## Not in scope
- WASM skill / built-in tool unification (follow-up)
- Dynamic tool registration at runtime
- Tool permission system changes
- Changes to ToolExecutor trait on the kernel side (it continues to work through the executor, which now delegates)
