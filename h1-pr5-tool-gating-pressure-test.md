# H1 PR 5: Tool Gating — Pressure Test

## Feature
Extract inline accessibility check from AgentExecutor into `AccessibilityGateCheck` as a proper `BoundaryCheck`. Make `BoundaryCheck.check` a suspend function.

## Reference: OpenClaw's 3-Layer Tool Gating

OpenClaw uses a three-layer approach to tool gating:

### Layer 1: `wrapToolWithBeforeToolCallHook`
- Runs BEFORE each tool call
- Can block execution and return an error result
- Used for permission checks, content policy, etc.
- Returns `{ resultForModel }` or `undefined` (proceed)

### Layer 2: `wrapToolWithAfterToolCallHook`  
- Runs AFTER each tool call
- Can modify/replace the result
- Used for output filtering, logging, telemetry

### Layer 3: Boundary Checks (loop-level)
- Evaluated at tool boundaries (between tool calls)
- Control loop flow: continue, inject, stop
- Used for cancellation, step limits, stuck detection

### Key Pattern: Binary Availability
OpenClaw treats tool availability as **binary** — when a capability is lost (e.g., browser tab closed), ALL tools gated on that capability fail, not just specific ones. The agent is effectively dead until the capability is restored.

## Our Design

### Current State (Before PR)
- Inline `if (toolCall.name in ACCESSIBILITY_TOOLS && !delegate.isScreenReaderAvailable())` block in the for loop
- Checks BEFORE tool execution
- Only gates tools in `ACCESSIBILITY_TOOLS` set
- Waits for reconnection inline, returns `accessibility_lost` if timeout

### After PR: AccessibilityGateCheck as BoundaryCheck
- `BoundaryCheck.check` becomes `suspend fun` (backward compatible)
- `AccessibilityGateCheck` runs at POST-TOOL boundary checkpoint
- Gates ALL tools (binary availability — consistent with OpenClaw)
- Accepts lambdas for availability/reconnection (no delegate coupling)

### Comparison

| Aspect | OpenClaw | Citros (Before) | Citros (After) |
|--------|----------|-----------------|----------------|
| Timing | Before tool (Layer 1) | Before tool (inline) | After tool (boundary) |
| Scope | All tools (binary) | ACCESSIBILITY_TOOLS only | All tools (binary) |
| Wait/retry | Tool hook blocks | Inline wait block | Suspend in check |
| Loop control | Hook returns error | Direct return from loop | CheckResult.Stop |
| Extensibility | Hook system | Hard-coded | BoundaryCheck interface |

### Gap Analysis

**Timing difference (INTENTIONAL):**
OpenClaw gates BEFORE tool execution. Our boundary checks run AFTER. This means one wasted tool call max per accessibility loss event. The tool fails naturally (accessibility service is detached → executeToolCall returns error), then the boundary check catches it and stops the loop. This is acceptable because:
1. One wasted API call is negligible cost
2. The tool's error result provides useful context in conversation history
3. OpenClaw's own `wrapToolWithBeforeToolCallHook` pattern returns an error result too — semantically similar

**Binary gating (ALIGNED):**
Moving from selective (`ACCESSIBILITY_TOOLS` set) to binary (all tools) aligns with OpenClaw's pattern. When accessibility is lost, the agent can't interact with the phone at all — gating only specific tools was a false distinction.

**Suspend boundary (ENHANCEMENT):**
OpenClaw's hook system doesn't have suspend semantics — it uses callbacks. Making `BoundaryCheck.check` suspend is a Kotlin-native improvement that enables the wait-for-reconnect pattern cleanly without blocking threads.

### Gaps

- **INTENTIONAL DIVERGENCE**: Post-tool vs pre-tool timing (one wasted call max, acceptable)
- **ALIGNED**: Binary availability model
- **ENHANCEMENT**: Suspend-based wait (cleaner than callbacks)
- **DEFERRED**: No Layer 2 equivalent (after-tool-call hooks) — not needed for this PR

## Conclusion
Design is sound. The post-tool timing is an intentional divergence from OpenClaw's pre-tool pattern, but the practical impact (one wasted tool call) is negligible. Binary gating aligns with OpenClaw. Suspend semantics are a Kotlin-native improvement.
