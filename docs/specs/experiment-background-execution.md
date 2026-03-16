# Experiment Background Execution — Ship Blocker Fix

**Ship blockers addressed:** #1 (server lock), #2 (TUI experiments invisible to GUI), #3 (session lost on crash)  
**Root cause:** Experiment tool runs synchronously in the agentic loop, holding the app Mutex for the duration  

---

## Problem

When the TUI runs an experiment via the `run_experiment` tool:
1. The tool call blocks inside `handle_run_experiment()` for minutes
2. The agentic loop holds the app Mutex during tool execution
3. All HTTP endpoints (`state.app.lock().await`) are blocked
4. The GUI shows "Disconnected/Offline"
5. If the server restarts, the in-memory session is lost
6. The experiment runs through fx-consensus but never registers in the HTTP ExperimentRegistry

## Fix

Make the experiment tool **spawn-and-return** instead of **run-and-wait**.

### Current flow:
```
Tool call → handle_run_experiment() → [blocks for minutes] → returns result
```

### New flow:
```
Tool call → spawn experiment in tokio::spawn → register in ExperimentRegistry → return immediately
             ↓ (background)
             ExperimentRunner::run_loop() runs independently
             → updates ExperimentRegistry on completion/failure
             → GUI polls registry for status
```

### Changes to `fx-tools/src/experiment_tool.rs`:

Add a new function `handle_run_experiment_background` that:
1. Accepts an `Arc<tokio::sync::Mutex<ExperimentRegistry>>` 
2. Creates an experiment entry with status `Running`
3. Spawns `tokio::spawn` with the actual experiment execution
4. On completion, updates the registry entry with results
5. Returns immediately with the experiment ID

### Changes to `fx-tools/src/tools.rs`:

The `FawxToolExecutor` needs access to the experiment registry. Add it as an optional field (same way `experiment_progress` works).

### Changes to `fx-cli/src/headless.rs` or startup:

Wire the experiment registry into the tool executor.

## What this gives us:
- Server stays responsive during experiments (no app lock held)
- GUI can see experiments in the Experiment Monitor (registered in registry)
- Session isn't at risk (experiment runs independently of the session)
- User gets immediate feedback: "Experiment started" instead of waiting minutes for a response
