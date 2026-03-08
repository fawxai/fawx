# Spec: Phase 0 PR 1 — TUI Feature Parity

**Gaps:** SubagentManager + ConfigManager missing from TUI mode  
**Estimated size:** ~100 lines  
**Risk:** Low — plumbing only

---

## Problem

TUI mode (`fawx tui`) uses `HeadlessLoopBuildOptions::default()` which has 
`subagent_control: None`. This means:
- `spawn_agent` and `subagent_status` tools are excluded from the tool list
- The TUI agent cannot spawn subagents

Additionally, `run_tui()` doesn't create a `ConfigManager`, so model-callable 
`config_get`/`config_set` tools lack the manager backend.

Both work in headless/HTTP mode because `build_headless_startup()` creates them.

## Solution

### SubagentManager in TUI

In `run_tui()` (`main.rs`):

```rust
// Before building the loop engine:
let improvement_provider = tui::build_improvement_provider(&auth_manager, &config);

// Create SubagentManager (same pattern as build_headless_app)
let factory = headless::HeadlessSubagentFactory::new(
    headless::HeadlessSubagentFactoryDeps {
        router: Arc::clone(&router),
        config: config.clone(),
        improvement_provider: improvement_provider.clone(),
    },
);
let subagent_manager = Arc::new(fx_subagent::SubagentManager::new(
    fx_subagent::SubagentManagerDeps {
        factory: Arc::new(factory),
        limits: fx_subagent::SubagentLimits::default(),
    },
));

// Pass to build options:
let bundle = tui::build_loop_engine_with_subagent(
    &config,
    improvement_provider,
    Arc::clone(&subagent_manager) as Arc<dyn fx_subagent::SubagentControl>,
)?;
```

This requires either:
- A new `build_loop_engine_with_subagent()` function, or
- Modifying `build_loop_engine_from_config()` to accept optional `SubagentControl`

Preferred: add an optional parameter to `build_loop_engine_from_config()`.

### ConfigManager in TUI

In `run_tui()`:
```rust
let config_manager = {
    let config_mgr = fx_config::manager::ConfigManager::from_config(config.clone());
    Some(Arc::new(std::sync::Mutex::new(config_mgr)))
};
```

Pass through `TuiAppDeps` (add field if not present) or through `LoopEngineBundle`.

### Verification

- After change: `fawx tui` → agent has `spawn_agent`, `subagent_status`, 
  `config_get`, `config_set` in tool list
- Test: spawn a subagent from TUI, verify it completes
- Existing tests pass (no behavior change for HTTP path)

## Files touched

| File | Change |
|------|--------|
| `main.rs` | Create SubagentManager + ConfigManager in `run_tui()` |
| `tui.rs` | Accept SubagentControl in build options, add ConfigManager to TuiAppDeps |
| Tests | Verify tool list includes subagent tools when control is attached |

## Implementation Gates (stop and report if triggered)

### Gate 1: Arc wrapping for ModelRouter
`run_tui()` creates `ModelRouter` as a bare value, but `HeadlessSubagentFactory` 
needs `Arc<ModelRouter>`. Check if `TuiApp` can work with `Arc<ModelRouter>` 
(read-only after init). If TuiApp needs mutable access for model switching 
(e.g., `/model` command), that requires `Arc<RwLock<ModelRouter>>` which 
touches more of TuiApp's ownership model.

**Rule:** If Arc wrapping requires >20 lines of changes to TuiApp's existing 
ModelRouter usage, **stop and report back** with what needs refactoring. 
Do not refactor TuiApp's ownership model in this PR — that's a separate scope.

### Gate 2: Subagent I/O isolation
`HeadlessSubagentFactory` spawns child `fawx serve` processes. In TUI mode, 
ratatui owns the alternate screen. If child processes inherit the parent's TTY 
file descriptors, they could corrupt terminal output.

**Rule:** Verify that `HeadlessSubagentFactory` uses explicit piped stdin/stdout 
(not inherited parent TTY). If it inherits the parent TTY, **stop and report** — 
that's a HeadlessSubagentFactory fix, not a TUI parity fix.

## Security

No new security surface. SubagentManager is the same implementation already 
running in HTTP mode. ConfigManager is read/write to the same config file.
