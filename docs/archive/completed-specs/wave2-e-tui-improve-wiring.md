# Wave 2 PR E — TUI Improve Wiring

## Goal
Wire `fx-improve` into the TUI by adding a `/improve` command that triggers the improvement cycle, 
and displaying improvement signals in the TUI status area.

## Target files
- **EDIT**: `engine/crates/fx-cli/src/tui.rs` — add `/improve` command handling
- **EDIT**: `engine/crates/fx-cli/Cargo.toml` — add `fx-improve` dependency
- **NEW** (if needed): `engine/crates/fx-cli/src/improve_command.rs` — extracted command logic

## fx-improve public API
```rust
// fx-improve/src/lib.rs
pub struct CyclePaths<'a> {
    pub signal_dir: &'a Path,
    pub analysis_dir: &'a Path,  
    pub plan_dir: &'a Path,
}

pub async fn run_improvement_cycle(
    paths: CyclePaths<'_>,
    config: &ImprovementConfig,
    git_skill: &dyn Skill,
    cancel: Option<&CancellationToken>,
) -> Result<ImprovementResult, ImprovementError>;
```

Check the actual public types in `fx-improve/src/lib.rs` before implementing — the above is from memory.

## /improve command design

### Command parsing
In the TUI input handler (where other `/` commands are matched):
```
/improve           — run improvement cycle with defaults
/improve --dry-run — analyze only, don't propose changes  
```

### Integration
1. Parse `/improve` from user input in the command handler
2. Construct `CyclePaths` from the fawx config directory (`~/.fawx/`)
3. Create `ImprovementConfig` with defaults
4. Call `run_improvement_cycle()` 
5. Display result summary to the user via the TUI output area

### Signal display
When an improvement cycle runs, display a status line:
```
⚡ Analyzing signals... → ⚡ Planning improvements... → ⚡ Proposing changes...
```

### Error handling  
- If `run_improvement_cycle` returns an error, display it as a user-visible error message
- If cancelled (Ctrl+C during improvement), handle gracefully via CancellationToken

## Tests
Tests should be in `fx-cli` test module:
1. `improve_command_parses_correctly` — verify `/improve` is recognized as a command
2. `improve_dry_run_flag_parsed` — verify `--dry-run` flag
3. `unknown_improve_subcommand_shows_help` — `/improve --invalid` shows usage

Note: integration tests with actual improvement cycles are hard to unit test (need signal files, git state). Focus on command parsing and wiring correctness. The fx-improve crate itself is already thoroughly tested.

## Constraints  
- Extract command logic into a separate function/module if the handler exceeds 40 lines.
- No `.unwrap()` outside tests.
- `cargo fmt --all` before commit.
- Run `cargo test -p fx-cli` and `cargo test -p fx-improve` to verify nothing breaks.
- Do NOT refactor existing tui.rs code — only add the /improve command path.
