# Spec: Chain History Forward — Recursive Experiment Learning

## Problem

Each experiment starts from zero. The subagent prompt has no knowledge of previous attempts — what was tried, what worked, what failed, and why. Run 8 makes the same mistakes as Run 1 because there's no learning between generations.

## Solution

Feed recent chain entries into the experiment prompt. When generating a new candidate, include a summary of prior experiments for the same signal — what patches were tried, whether they built, whether tests passed, what the scores were, and what went wrong.

## Files to modify

- `engine/crates/fx-consensus/src/llm_source.rs` — modify `build_subagent_experiment_prompt` and `build_experiment_prompt`
- `engine/crates/fx-consensus/src/chain.rs` — add method to query recent entries by signal

## Design

### Chain query

Add a method to `ConsensusChain`:

```rust
/// Returns the most recent `limit` entries matching the given signal name.
pub fn recent_entries_for_signal(&self, signal_name: &str, limit: usize) -> Vec<&ChainEntry> {
    self.entries
        .iter()
        .rev()
        .filter(|e| e.experiment.signal.name == signal_name)
        .take(limit)
        .collect()
}
```

### History summary builder

Add a new function in `llm_source.rs`:

```rust
/// Builds a human-readable summary of prior experiment attempts.
fn format_chain_history(entries: &[&ChainEntry]) -> String
```

For each entry, summarize:
- Experiment ID (abbreviated, first 8 chars)
- Decision (Accept/Reject/Inconclusive)
- For each candidate:
  - Strategy name
  - Score
  - Whether build passed
  - Whether tests passed  
  - Key failure reason (from evaluation notes)
  - First 20 lines of the patch (if stored in candidate_patches)
- What to learn: "Avoid referencing functions that don't exist in the file" (extracted from failure patterns)

### Prompt integration

Modify `build_subagent_experiment_prompt` to accept an optional `history: &str` parameter:

```rust
pub fn build_subagent_experiment_prompt(
    experiment: &Experiment,
    strategy: &GenerationStrategy,
    history: Option<&str>,
) -> String
```

If history is Some, add a section to the prompt BEFORE the task instructions:

```
## Previous Experiment Results

The following experiments have already been run for this signal. Learn from their successes and failures.

{history}

## LESSONS FROM HISTORY
- Do NOT repeat patterns that failed in previous runs
- If a previous run failed because of undefined functions, READ the file first to find what helpers actually exist
- If a previous run's patch didn't compile, verify your changes compile before finishing
```

### Plumbing: pass chain to generator

The `SubagentPatchSource` needs access to the chain. Two options:

**Option A (preferred):** Pass the formatted history string at experiment start time, not the chain itself. The orchestrator reads the chain, formats history, and passes it through to the patch source.

Add a field to `SubagentPatchSource`:
```rust
chain_history: Option<String>,
```

Set it via a new method:
```rust
pub fn with_chain_history(mut self, history: String) -> Self {
    self.chain_history = Some(history);
    self
}
```

The orchestrator calls `format_chain_history` and passes it to each node's patch source before running.

**Option B:** Pass `Arc<ConsensusChain>` to SubagentPatchSource. Rejected because it couples the generator to chain internals.

### Orchestrator changes

In `ExperimentOrchestrator::run_experiment`, before generating candidates:
1. Load the chain
2. Query recent entries for the current signal (limit 5)
3. Format history
4. Pass to each node's patch source

This requires the orchestrator to have access to the chain path. Add it to the orchestrator constructor or config.

Actually — the `ExperimentRunner` already has the chain. The runner should format the history and pass it to the orchestrator/nodes before starting.

In `ExperimentRunner::run`:
1. After creating the experiment, before passing to orchestrator
2. Read recent chain entries for this signal
3. Format history string
4. Set chain_history on each node's patch_source

This means `NodeConfig::patch_source` needs to be mutable, or we set history before constructing the runner. Since nodes are built in the CLI command, the CLI should read the chain, format history, and set it on each SubagentPatchSource before creating the runner.

### Changes to CLI experiment command

In `build_subagent_nodes` (both `fx-cli/commands/experiment/mod.rs` and `fx-tools/experiment_tool.rs`):
1. Accept a `chain_history: Option<String>` parameter
2. Pass it to `SubagentPatchSource::with_chain_history`

In the caller (run command handler):
1. Load chain from default path
2. Query recent entries for the signal
3. Format history
4. Pass to build_subagent_nodes

### Tests

1. `recent_entries_for_signal_returns_matching_entries` — chain with mixed signals, verify filtering
2. `recent_entries_for_signal_respects_limit` — chain with 10 entries, limit 3, verify only 3 returned
3. `recent_entries_for_signal_returns_empty_for_unknown` — no matching signal
4. `format_chain_history_includes_decision_and_scores` — verify output contains expected fields
5. `format_chain_history_includes_failure_reasons` — verify build failure notes appear
6. `format_chain_history_truncates_long_patches` — verify patches are truncated to 20 lines
7. `prompt_includes_history_when_provided` — verify the prompt string contains history section
8. `prompt_excludes_history_when_none` — verify no history section when None

### Important constraints

- History limit: 5 most recent entries (configurable, but 5 is the default)
- Patch preview in history: first 20 lines only (full patches waste context)
- History section goes BEFORE task instructions (so the model sees it as context, not as the task)
- Don't include the current experiment in history (it hasn't happened yet)
- If chain file doesn't exist or is empty: no history section, no error
- `format_chain_history` must be deterministic — same inputs produce same output
