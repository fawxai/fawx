# Bug: Tool Spam Regression in Fawx Server

## Problem
When sending a simple message like "hey fawx" to the HTTP server, the model makes excessive tool calls (current_time x3, fawx_status x3) before responding, hitting the per-tool retry budget (2 per tool per cycle). This was NOT happening before ~72 hours ago. The bug affects both Claude and GPT models, so it's not model-specific.

## Repo
~/fawx on this machine, branch: fix/fx-api-test-failures (or dev — same code minus 3 small test fixes).

## What to investigate
Something changed in the last 72 hours in the code path between "user sends message" and "model receives CompletionRequest" that causes the model to reflexively call tools for simple greetings. The model gets tool definitions + system prompt + conversation history. If any of those changed in a way that encourages tool use, that's the bug.

## How to reproduce
1. Build: `export PATH="$HOME/.cargo/bin:$PATH" && cargo build --release`
2. Start server: `./target/release/fawx serve --http` (runs on port 8400, config at ~/.fawx/config.toml)
3. Get bearer token: `grep bearer ~/.fawx/config.toml`
4. Send test message:
   ```
   curl -s http://127.0.0.1:8400/v1/sessions/test-debug/message \
     -H "Authorization: Bearer <token>" \
     -H "Content-Type: application/json" \
     -d '{"message":"hey fawx"}' | head -50
   ```
5. Check server terminal output for "[warning] [tool_execution] Tool 'current_time' blocked" lines
6. If those warnings appear → bug is present at this commit. If the model just responds with text → bug is NOT present.

## Key files in the execution path
- `engine/crates/fx-kernel/src/loop_engine.rs` — The agentic loop. Builds CompletionRequest with system prompt, tools, and conversation. Look at `build_reasoning_request()` (~line 4783), `reasoning_user_prompt()`, `REASONING_SYSTEM_PROMPT`, and `build_continuation_request()`.
- `engine/crates/fx-cli/src/startup.rs` — Builds the executor chain: PermissionGateExecutor → TripwireEvaluator → ProposalGateExecutor → CachingExecutor → SkillRegistry. Look at `build_loop_engine_with_options()` (~line 475).
- `engine/crates/fx-kernel/src/permission_gate.rs` — Permission checks before tool execution. `tool_to_action_category()` maps tool names to categories. `current_time` and `fawx_status` map to "unknown" which is allowed in capability mode.
- `engine/crates/fx-tools/src/tools.rs` — Tool definitions and implementations. The tool definitions list is what the model sees.
- `engine/crates/fx-cli/src/headless.rs` — HeadlessApp orchestrates cycles. `run_cycle_streaming_http()` (~line 866) is the HTTP entry point.

## Bisect strategy
The regression was introduced between ~March 14 and March 17. Key commits to test:

**Likely good (before the regression window):**
- `794c44d3` (March 15 00:34 UTC) — tool continuation retry fix
- `31fd8898` (before that) — error surfacing

**Likely bad (after the regression window):**
- `fba01818` (March 17, current dev HEAD)

Commits that touched the execution path in the last 72 hours:
1. `c8a2ac5b` — Wire PermissionGateExecutor into startup (CHANGED THE EXECUTOR CHAIN)
2. `de0ff460` — Wire PermissionGateExecutor SSE callback per-cycle
3. `a87d6f9c` — Ripcord API + TripwireEvaluator wiring (ADDED ANOTHER EXECUTOR WRAPPER)
4. `30196b5a` — Capability-based enforcement mode
5. `fba01818` — Fix: capability mode allows unknown tools by default
6. `a73ec747` — Wire experiment registry into startup
7. `52f991ad` — Wire experiment tool to ExperimentRegistry
8. `3430dc1d` — Yield primitive in loop engine

Start by testing `794c44d3` (should be good) and `c8a2ac5b` (first executor chain change). If 794c44d3 is good and c8a2ac5b is bad, the PermissionGateExecutor wiring introduced it. Then narrow from there.

## What to look for
The model decides to call tools based on what it sees in the CompletionRequest. Compare requests between a good and bad commit. Specifically:
- Did the number of tool definitions change? (More tools = model more likely to call them)
- Did the system prompt change?
- Did the conversation message format change?
- Is the PermissionGateExecutor or TripwireEvaluator modifying/wrapping tool results in a way that confuses the model?
- Are tool results being returned differently (e.g., wrapped in extra metadata)?

## Instrumentation approach
If bisect is inconclusive, add temporary logging to `build_reasoning_request()` in loop_engine.rs to print:
- Number of tool definitions
- System prompt length and first 200 chars
- Number of messages in context
This will show exactly what the model sees at the first iteration.

## Goal
Identify which commit introduced the regression, understand WHY it causes tool spam, and fix it. The fix should go on branch `fix/fx-api-test-failures`.
