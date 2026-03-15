# Experiment Tool Spec

Wire `run_experiment` as a tool callable from Fawx's agentic loop (TUI chat, Telegram, any channel).

## Tool Definition

```json
{
  "name": "run_experiment",
  "description": "Run a proof-of-fitness experiment. Spawns competing subagent nodes that generate patches, evaluates them against fitness criteria, and records the result to the consensus chain. Use this when asked to improve, research, or experiment with any aspect of the codebase.",
  "parameters": {
    "type": "object",
    "properties": {
      "signal": {
        "type": "string",
        "description": "Signal or topic that triggered this experiment (e.g., 'error handling', 'performance', 'test coverage')"
      },
      "hypothesis": {
        "type": "string",
        "description": "What improvement or research hypothesis to test"
      },
      "scope": {
        "type": "string",
        "description": "File patterns to modify (glob, comma-separated). Default: src/**/*.rs"
      },
      "nodes": {
        "type": "integer",
        "description": "Number of competing nodes (default: 3)"
      },
      "mode": {
        "type": "string",
        "enum": ["placeholder", "direct", "subagent"],
        "description": "Experiment mode: placeholder (mock), direct (raw LLM), subagent (full agent). Default: subagent"
      },
      "timeout": {
        "type": "integer",
        "description": "Timeout per node in seconds (default: 120)"
      }
    },
    "required": ["signal", "hypothesis"]
  }
}
```

## Implementation

### New file: `engine/crates/fx-tools/src/experiment_tool.rs`

The tool handler lives in fx-tools, not fx-cli. It wraps the experiment runner.

#### Dependencies needed
- `fx-consensus` (ExperimentRunner, types)
- `fx-subagent` (SubagentManager — already available via FawxToolExecutor.subagent_control)
- `fx-auth` and `fx-config` (for building the router in direct/subagent mode)

#### State needed on FawxToolExecutor
```rust
pub struct ExperimentToolState {
    pub project_dir: PathBuf,       // working directory as project default
    pub chain_path: PathBuf,        // consensus chain storage path
    pub router: Option<Arc<ModelRouter>>,  // shared router for direct/subagent modes
    pub config: Option<FawxConfig>,
}
```

Add `experiment: Option<ExperimentToolState>` to `FawxToolExecutor`.

#### Handler: `handle_run_experiment`

1. Parse args into `RunExperimentArgs`
2. Default `mode` to `subagent` (not placeholder — this is an agent tool, use real agents)
3. Default `project` to `self.working_dir` (the agent's working directory)
4. Call `run_experiment_with_path(args, chain_path)` — reuse existing CLI logic
5. Return the formatted report string

**Critical: progress streaming.** The tool must emit progress updates back to the conversation as the experiment runs. This is the verbose logging Joe wants.

### Progress Callback

Add a `ProgressCallback` trait or closure to `ExperimentRunner::run()`:

```rust
pub type ProgressCallback = Arc<dyn Fn(ExperimentProgress) + Send + Sync>;

pub enum ExperimentProgress {
    NodeSpawned { node_id: String, strategy: String },
    NodeGenerating { node_id: String },
    NodePatchReady { node_id: String, files_changed: usize },
    NodeBuilding { node_id: String },
    NodeBuildResult { node_id: String, success: bool, error: Option<String> },
    NodeTesting { node_id: String, baseline_count: usize },
    NodeTestResult { node_id: String, passed: usize, failed: usize, new_tests: usize },
    CrossEvalStarting,
    Scoring { scores: Vec<(String, f64)> },
    Decision { accepted: bool, winner: Option<String> },
}
```

The tool handler converts these to streaming text updates in the conversation.

### Wiring

In `engine/crates/fx-tools/src/tools.rs`:
1. Add `"run_experiment"` to the dispatch match
2. Add tool definition to `builtin_tool_definitions()` (gated on experiment feature or always-on)
3. Add cacheability: `SideEffect`

In `engine/crates/fx-tools/src/skill_bridge.rs`:
1. `BuiltinToolsSkill` includes the experiment tool definition

In `engine/crates/fx-cli/src/startup.rs`:
1. When building `FawxToolExecutor`, populate `ExperimentToolState` with working dir, chain path, router, config

### Feature gate

Use `#[cfg(feature = "experiment")]` or always include (since fx-consensus is already a dependency). Prefer always-on since this is a core capability.

## Tests

1. Tool definition is present in builtin tools
2. Parse experiment args from JSON
3. Mock experiment runner returns formatted report
4. Invalid args (missing signal/hypothesis) return clear error
5. Progress callback fires in correct order

## NOT in scope
- Chain format stabilization (separate spec)
- Custom fitness criteria from tool args (use defaults for now)
- Experiment history tool (use `fawx experiment chain` CLI for now)
