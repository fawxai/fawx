# H1 PR 5: Tool Gating — Pressure Test vs OpenClaw

## OpenClaw's Architecture: 3 Layers of Tool Gating

OpenClaw gates tool availability at **three distinct layers**, each operating at a different lifecycle point:

### Layer 1: Session Init — Static Policy Filtering
**When:** Once, when tools are assembled for a session (`createOpenClawCodingTools()`)
**What:** Config-driven allow/deny lists, glob patterns, per-agent/per-provider policies

```
tools → applyToolPolicyPipeline() → filterToolsByPolicy() → filtered tools
```

Pipeline steps (in order):
1. Profile policy (per-auth-profile allow/deny)
2. Provider profile policy (per-model-provider allow/deny)
3. Global policy (`tools.allow` / `tools.deny` in config)
4. Global provider policy (`tools.byProvider.allow`)
5. Agent policy (`agents.X.tools.allow`)
6. Agent provider policy (`agents.X.tools.byProvider.allow`)
7. Group policy (channel/group-specific restrictions)
8. Sandbox policy (sandbox tools.allow)
9. Subagent policy (subagents get fewer tools by default)

Each step uses `makeToolPolicyMatcher()` with glob expansion. Deny matches first, then allow. Plugin tool groups get special handling.

**Key insight:** This is a **compile-time** filter. The tool list is built once and doesn't change mid-session. Tools that fail policy never reach the model.

### Layer 2: Per-Tool-Call — Before-Tool-Call Hook
**When:** Before EACH tool execution (`wrapToolWithBeforeToolCallHook()`)
**What:** Plugin hooks that can **block** or **modify** individual tool calls at runtime

```javascript
async function runBeforeToolCallHook(args) {
    const hookResult = await hookRunner.runBeforeToolCall({
        toolName, params
    }, { toolName, agentId, sessionKey });
    
    if (hookResult?.block) return {
        blocked: true,
        reason: hookResult.blockReason || "Tool call blocked by plugin hook"
    };
    
    if (hookResult?.params) return {
        blocked: false,
        params: { ...params, ...hookResult.params }  // modified params
    };
}
```

Every tool gets wrapped: `tools.map(tool => wrapToolWithBeforeToolCallHook(tool, ctx))`

If blocked, throws an error that becomes the tool result. If params modified, the tool runs with adjusted params.

**Key insight:** This is a **runtime** gate. It runs before every single tool call, can inspect the call parameters, and can make dynamic decisions (e.g., "block exec commands matching pattern X" or "this tool requires a node but none is connected").

### Layer 3: Capability-Based Inclusion — Skill Eligibility
**When:** At skill discovery/loading time
**What:** `shouldIncludeSkill()` checks runtime capabilities:

```javascript
function shouldIncludeSkill(params) {
    // OS check — skill requires macOS but we're on Linux
    if (osList.length > 0 && !osList.includes(resolveRuntimePlatform())) {
        // BUT: if a remote node has the right platform, still include
        if (!remotePlatforms.some(p => osList.includes(p))) return false;
    }
    
    // Binary check — skill requires `gh` but it's not installed
    for (const bin of requiredBins) {
        if (hasBinary(bin)) continue;
        if (eligibility?.remote?.hasBin?.(bin)) continue;  // remote node has it
        return false;
    }
    
    // Env check — skill requires ELEVENLABS_API_KEY
    for (const envName of requiredEnv) {
        if (process.env[envName]) continue;
        if (skillConfig?.env?.[envName]) continue;
        return false;
    }
    
    // Config check — skill requires specific config paths
    // ...
}
```

**Key insight:** This is **declarative**. Skills declare their requirements in SKILL.md frontmatter (`requires: { bins: ["gh"], env: ["GITHUB_TOKEN"] }`). The runtime checks these at load time and excludes skills that can't run. The model never sees tools it can't use.

### Layer 4 (implicit): Runtime Error Handling
**When:** During tool execution
**What:** Tools that need unavailable resources throw descriptive errors

```javascript
// Node tools
if (nodes.length === 0) throw new Error(
    "exec host=node requires a paired node (none available)"
);

// Browser tools  
if (!normalizedSandbox) throw new Error(
    "Sandbox browser is unavailable. Enable ... or use target=\"host\""
);

// Broadcast
if (!cfg.tools?.message?.broadcast?.enabled) throw new Error(
    "Broadcast is disabled. Set tools.message.broadcast.enabled"
);
```

These are **not** pre-execution gates — they're runtime errors that become tool results. The model sees the error and can adjust.

---

## Citros Current State

### What we have:
1. **Inline pre-tool accessibility check** in AgentExecutor (Layer 2 equivalent):
   - Checks `toolCall.name in ACCESSIBILITY_TOOLS && !delegate.isScreenReaderAvailable()`
   - Waits for reconnection with timeout
   - Hard stops the loop on failure

2. **Prompt-level tool omission** in PhoneAgentPrompts (Layer 1 equivalent):
   - `buildSystemPrompt(phoneControlAvailable=false)` omits the tools section entirely
   - Model doesn't see phone tools when accessibility is detached

3. **Model floor enforcement** in PhoneAgentApi (Layer 1 equivalent):
   - `ModelClassifier.isAboveFloor()` rejects below-floor models at construction
   - Below-floor models can't even start a session

### What we're missing:
- **No formalized boundary check for accessibility** — it's inline special-case code
- **No extensible pre-tool hook** — the `wrapToolWithBeforeToolCallHook` pattern
- **No declarative capability system** — tools don't declare their requirements

---

## Comparison & Design Decisions

### OpenClaw Pattern → Citros Equivalent

| OpenClaw Layer | Mechanism | Citros Equivalent | Status |
|---|---|---|---|
| Session init policy | `filterToolsByPolicy()` pipeline | `buildSystemPrompt(phoneControlAvailable)` | ✅ Done (prompt-level) |
| Per-tool-call hook | `wrapToolWithBeforeToolCallHook()` | **Boundary check + pre-batch check** | 🔧 PR 5 |
| Skill eligibility | `shouldIncludeSkill()` + `hasBinary()` | N/A (single app, no skill system yet) | Deferred to H2 |
| Runtime error | `throw new Error("node unavailable")` | `"Failed: accessibility service detached..."` | ✅ Done (inline) |

### Key Difference: OpenClaw wraps tools, Citros checks at boundaries

OpenClaw's `wrapToolWithBeforeToolCallHook` wraps each tool's `execute` function with a pre-check. The gate is per-tool, per-call, with full access to the tool name and parameters.

Our boundary check system runs between tools, not wrapping individual tool execution. This means:
- We check **after** tool N, gating tool N+1 (same timing, different framing)
- We don't have access to tool N+1's name/params at the boundary check point
- First tool in a batch needs a separate pre-batch check

### Why boundary checks are the right fit for Citros (for now)

1. **We have exactly one gating concern:** accessibility. OpenClaw has dozens (node availability, sandbox state, plugin hooks, policy matching). Their `wrapToolWithBeforeToolCallHook` pattern makes sense for a plugin ecosystem. We don't have plugins.

2. **Accessibility is binary, not per-tool:** When the accessibility service drops, ~90% of phone tools are unusable. Checking "is the next tool in ACCESSIBILITY_TOOLS?" is surgical but barely adds value — if accessibility is gone, the agent is effectively dead regardless.

3. **Boundary checks are already the extensibility point:** CancellationCheck, StepLimitCheck, StuckDetectionCheck, SteerCheck are all boundary checks. Adding AccessibilityGateCheck keeps the pattern consistent.

4. **The wait/retry mechanism fits suspend checks:** Making `BoundaryCheck.check` a `suspend fun` lets AccessibilityGateCheck do the wait/reconnect logic cleanly. Existing checks don't suspend, so backward compatible.

### What Citros should adopt from OpenClaw (future):

1. **`wrapToolWithBeforeToolCallHook` pattern** — when we add the WASM skill system (H3) or MCP tools, we'll need per-tool pre-execution hooks. The boundary check system won't be enough for per-tool parameter inspection.

2. **Declarative requirements** — tools/skills should declare `requires: { accessibility: true }` rather than checking a hardcoded set. This enables future gating on network, permissions, specific Android APIs, etc.

3. **The `disableTools` flag** — OpenClaw has `params.disableTools ? [] : createOpenClawCodingTools(...)`. A simple boolean that strips all tools. We could use this for a "chat-only mode" when accessibility is fully detached.

---

## PR 5 Design

### Changes

1. **`BoundaryCheck.check` becomes `suspend fun`** — backward compatible, enables async checks

2. **`AccessibilityGateCheck`** — new boundary check:
   ```kotlin
   class AccessibilityGateCheck(
       private val isAvailable: () -> Boolean,
       private val waitForReconnect: suspend (Long) -> Boolean,
       private val onReconnected: suspend () -> Unit,
       private val onLost: () -> Unit,
       private val waitTimeoutMs: Long = 5000L
   ) : BoundaryCheck {
       override suspend fun check(state: LoopState): CheckResult {
           if (isAvailable()) return CheckResult.Continue
           val reconnected = waitForReconnect(waitTimeoutMs)
           if (reconnected) {
               onReconnected()  // refresh screen
               return CheckResult.Continue
           }
           onLost()
           return CheckResult.Stop("accessibility_lost")
       }
   }
   ```

3. **Pre-batch accessibility check** at the top of the while loop (mirrors pre-batch steer)

4. **Remove inline accessibility block** from the inner for loop

5. **Default boundary checks updated:**
   ```kotlin
   fun defaultBoundaryChecks(...): List<BoundaryCheck> = listOf(
       AccessibilityGateCheck(...),  // First — no point running tools without accessibility
       CancellationCheck(),
       StepLimitCheck(),
       StuckDetectionCheck.withDefaults(),
       SteerCheck()
   )
   ```

### Check ordering rationale
- AccessibilityGateCheck **first**: If accessibility is gone, no other check matters
- CancellationCheck **second**: User cancel should still work even during accessibility wait
- Actually — **CancellationCheck should remain first**. If the user cancels during an accessibility wait, they shouldn't have to wait for the timeout. The accessibility check should respect cancellation.

Revised order:
```kotlin
CancellationCheck(),           // Always first — user intent
AccessibilityGateCheck(...),   // Gate on capability
StepLimitCheck(),              // Hard ceiling
StuckDetectionCheck.withDefaults(),
SteerCheck()                   // Last — inject after all gates pass
```

### Edge case: cancellation during accessibility wait

The `waitForReconnect` lambda should check cancellation internally (it's a suspend function, can be cancelled via coroutine cancellation). If CancellationCheck runs first and returns Stop, AccessibilityGateCheck never runs. If accessibility wait is in-progress when cancel arrives, coroutine cancellation handles it.

---

## Gaps Identified

1. **No tool-level capability declaration** — tools don't declare `requires: accessibility`. The `ACCESSIBILITY_TOOLS` set is hardcoded. This is fine for H1 but should be declarative by H2 (when we add tool grouping).

2. **No per-tool parameter inspection** — boundary checks don't see the upcoming tool's name or params. This is fine because accessibility is binary, but won't work for future gating that's tool-specific (e.g., "allow web_search but confirm http_request").

3. **No `disableTools` equivalent** — when accessibility is fully detached and not coming back, we should strip tools from the API call entirely (chat-only mode). Currently we just omit the tools section from the system prompt, but still send tool schemas to the API.

---

*Pressure test completed 2026-02-16*
*Reference: OpenClaw dist (minified), `pi-tools.policy`, `before-tool-call`, `skills`, `dangerous-tools`*
