# Spec: Post-hoc Build Verification for Experiment Subagents

## Problem

The experiment subagent generates patches that don't compile. The subagent claims "all tests pass" but never actually runs `cargo build`. The evaluator catches this (build_ok=false, score 0.00), but by then the experiment round is wasted.

## Solution

After extracting a patch from the subagent (either via `<PATCH>` tags or git diff fallback), verify the build in the generator workspace before submitting the candidate. If the build fails, give the subagent one retry with the error output.

## Files to modify

- `engine/crates/fx-consensus/src/subagent_source.rs` — main changes

## Design

### In `SubagentPatchSource::generate_patch`:

After successfully extracting a patch (either from parse or git diff fallback), add a build verification step:

1. Run `cargo build` in `self.working_dir` (the generator's cloned workspace where the subagent made changes)
2. If build succeeds → return the patch as-is (happy path)
3. If build fails → capture stderr, attempt one retry:
   a. Send a follow-up message to the subagent session with the build errors
   b. Wait for the subagent to respond with fixes
   c. Re-extract the patch via git diff (the subagent will have edited files again)
   d. Return whatever we get (don't retry infinitely)

### Retry mechanism

The current subagent flow uses `SpawnMode::Run` which is one-shot. For retry:
- Change `SpawnMode::Run` to `SpawnMode::Session` so the subagent session stays alive
- After initial response, if build fails, send a second message: "Your changes failed to build:\n```\n{stderr}\n```\nFix the build errors. Do not explain — just use the tools to fix the files."
- Poll for the second response with the same timeout logic
- After getting the second response, extract patch via git diff
- Clean up the session

### Build verification function

Add a private method or free function:

```rust
/// Runs `cargo build` in the given directory.
/// Returns Ok(()) on success, Err(stderr) on failure.
fn verify_build(working_dir: &Path) -> Result<(), String> {
    let output = std::process::Command::new("cargo")
        .arg("build")
        .current_dir(working_dir)
        .output()
        .map_err(|e| format!("failed to run cargo build: {e}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).to_string())
    }
}
```

### Important constraints

- The retry message MUST be short and directive. Don't re-explain the experiment. Just say "build failed, here are errors, fix them."
- Timeout for retry: use half the remaining experiment timeout, or 120 seconds, whichever is less
- If the retry also fails to build, return the patch anyway — let the evaluator score it. Don't throw away the candidate.
- Log build verification results at `info!` level
- The `verify_build` should use async (tokio::process::Command) to not block the runtime

### Tests

1. `verify_build_succeeds_for_valid_project` — set up a temp Cargo project, verify_build returns Ok
2. `verify_build_fails_for_broken_project` — introduce a syntax error, verify_build returns Err with the error message
3. `generate_patch_retries_on_build_failure` — mock subagent that produces a non-compiling patch first, then a compiling patch on retry. Verify the retry happens and the final patch is from the second attempt.

### Edge cases

- If `cargo` is not in PATH: return an error string, don't panic
- If the working dir doesn't exist: return error (shouldn't happen since we own the workspace, but defensive)
- If the subagent session dies during retry: return the original patch from the first attempt
