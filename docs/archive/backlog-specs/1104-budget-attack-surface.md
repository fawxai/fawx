# Budget Manipulation as Attack Surface

**Status:** Proposed spec  
**Issue:** #1104  
**Related:** #1098 (loop resilience), #1101 (per-tool retry budget), #1100 (decompose complexity gate), #1102 (kernel/loadable boundary security)  
**Branch:** `security/budget-attack-surface`

---

## Problem

Budget exhaustion stops the agent. When the budget runs out, the loop terminates — the agent cannot complete its task, cannot synthesize partial results (unless it hits the soft-ceiling first), and the user gets incomplete work. This makes budget a **denial-of-service vector**, not just a UX concern.

The current security model (`docs/architecture/security-model.html`) defines twelve attack surfaces covering messaging, network, memory, LLM provider, sensors, multi-agent escalation, and more. None of them address **resource exhaustion through budget manipulation** — an attacker (or a degenerate prompt) that can drain the agent's budget has effectively disabled it.

Three Wave 4 PRs already mitigate budget-related failure modes:

- **#1098 (loop resilience):** Budget soft-ceiling (`BudgetState::Low` at 80% cost/LLM-calls), fan-out cap (default 4), tool result truncation (16KB).
- **#1101 (per-tool retry budget):** Per-tool retry cap (default 2 retries = 3 total attempts per tool name per cycle), escalation to `SignalKind::Blocked`.
- **#1100 (decompose complexity gate):** Batch detection, complexity floor, and cost gate (reject plans exceeding 150% of remaining budget).

These are defense-in-depth mechanisms that emerged from production failure modes. But none were designed with an adversarial threat model in mind — they prevent accidental budget waste, not intentional budget attacks. This spec analyzes the gap.

---

## Threat Scenarios

### Scenario 1: Decompose Spiral

**Attack description:** A crafted prompt triggers recursive decomposition where each level generates sub-goals that themselves require decomposition. The agent burns budget on planning overhead — LLM calls to parse plans, allocate budgets, spawn child loops — without performing useful work.

**Example:** *"For every file in this repository, analyze its purpose, identify all functions, determine which functions could be refactored, generate a refactoring plan for each, then decompose each refactoring into individual steps."* This creates a decomposition tree where each leaf could trigger further decomposition (the word "refactor" triggers `ComplexityHint::Complex` via `estimate_complexity()`), and the fan-out at each level is proportional to repository size.

**Severity:** Medium  
**Likelihood:** Low (requires knowledge of decomposition mechanics) / Medium (accidental via ambiguous prompts)

**Existing mitigations:**
- `max_recursion_depth` (default 8, adaptive mode caps based on remaining LLM calls via `effective_max_depth()`) — #1098 dependency
- `BudgetState::Low` soft-ceiling at 80% cost — blocks decomposition in wrap-up mode (#1098)
- Cost gate in decompose complexity gate — rejects plans exceeding 150% of remaining budget (#1100)
- Batch detection — converts trivial decomposition into direct tool calls (#1100)
- Complexity floor — prevents decomposition overhead for trivially simple plans (#1100)

**Residual risk after Wave 4: Low.**  
The combination of adaptive depth cap, cost gate, and soft-ceiling creates three independent barriers. An adversarial prompt would need to craft plans that pass the cost gate at each level while still being wasteful — difficult because the cost gate uses conservative estimates (`DEFAULT_LLM_CALL_COST_CENTS = 2`, `DEFAULT_TOOL_INVOCATION_COST_CENTS = 1`) that overestimate, making rejection more likely.

The remaining gap: the cost gate estimates per-sub-goal cost but doesn't account for the overhead of the decomposition pipeline itself (budget allocation, child loop setup). With 8 levels of recursion and up to 5 sub-goals per level (`MAX_SUB_GOALS = 5`, distinct from `max_fan_out = 4` which caps parallel tool calls per turn), the theoretical worst case is 5^8 = 390,625 leaf nodes, or (5^8 − 1) / 4 = 97,656 total nodes in the decomposition tree. In practice, the number is academic — the adaptive depth cap (`effective_max_depth()`) reduces depth based on remaining LLM calls, making deep trees unreachable well before budget is consumed. Even if most are caught by depth/cost gates, the pipeline overhead of rejected plans consumes budget.

**Additional mitigation needed: No.** The adaptive depth mode (`effective_max_depth()`) already throttles depth based on remaining LLM calls — at ≤6 remaining calls, depth cap drops to 0 (no decomposition). This is sufficient to prevent runaway decomposition. The overhead of rejected plans is bounded by the LLM calls required to generate them (1 per level), which are individually cheap.

---

### Scenario 2: Retry Amplification

**Attack description:** A prompt engineered to cause persistent tool failures that the model retries, draining budget on doomed operations. The attacker doesn't need to control the tool — they just need to craft input that causes a tool to fail in a way the model believes is retryable.

**Example:** *"Read the file at /proc/self/mem and parse it as JSON."* The `read_file` tool will fail on `/proc/self/mem` (binary/permission), the model will retry with slight variations (`/proc/self/maps`, `/proc/self/status`), each attempt consuming a tool invocation + LLM call to decide on retry. Alternatively: *"Search the codebase for the function `xQ7z_nonexistent_symbol`"* — tool succeeds but returns empty, model retries with variations, each burning budget.

**Severity:** Medium  
**Likelihood:** Medium (easy to craft prompts that cause tool failures)

**Existing mitigations:**
- Per-tool retry cap (default 2 retries = 3 total attempts per tool name per cycle) — escalates to `SignalKind::Blocked` (#1101)
- `BudgetState::Low` soft-ceiling — blocks all tool calls at 80% cost (#1098)
- Fan-out cap (default 4) — limits parallel retry attempts per LLM response (#1098)

**Residual risk after Wave 4: Low.**  
With the per-tool retry budget, a single tool can consume at most 3 attempts per cycle. With the default budget config (`max_tool_invocations: 128`, `max_llm_calls: 64`), the worst case is the model cycling through different tool names — but each name gets only 3 attempts, and the model can only request 4 tools per turn (fan-out cap). At 4 tools/turn × 3 attempts/tool × ~16 turns before soft-ceiling = ~192 tool invocations theoretical max, but the soft-ceiling triggers at 80% of 64 LLM calls (≈51 calls), cutting this significantly shorter.

The remaining gap: the retry cap is per-name, so `read_file("a")` and `read_file("b")` both count against `read_file`'s limit. But the model could call different tool *names* — `read_file` blocked → switch to `search_text` → `search_text` blocked → switch to `list_directory`. Each tool gets 3 attempts. With N tools, the model gets 3N total attempts before all tools are blocked.

**Additional mitigation needed: No.** The soft-ceiling is the backstop. Even if the model cycles through every available tool, the 80% cost ceiling terminates the loop with a synthesis response. The per-tool cap prevents any single tool from being the primary drain vector.

---

### Scenario 3: Fan-Out Explosion

**Attack description:** A single prompt that causes the model to request many tool calls simultaneously, inflating context with tool results and burning through tool invocation budget in a single turn.

**Example:** *"Read all 25 source files in the project and give me a summary of each."* Without the fan-out cap, this produces 25 `read_file` calls in one turn. Each returns up to 16KB (post-truncation), injecting up to 400KB into context. This can exceed context limits, trigger compaction, and waste budget on tool calls whose results are immediately compacted away.

**Severity:** Low  
**Likelihood:** High (very natural request pattern — not even adversarial)

**Existing mitigations:**
- Fan-out cap (default 4 per LLM response) — excess calls deferred with message to re-request (#1098)
- Tool result truncation (default 16KB per result) — caps per-result context consumption (#1098)
- Aggregate result bytes tracking — triggers `BudgetState::Low` when accumulated results exceed `max_aggregate_result_bytes` (default 400KB) (#1098)
- Batch detection in decompose gate — converts decomposition of identical tool calls into direct fan-out-capped execution (#1100)

**Residual risk after Wave 4: Low.**  
The fan-out cap directly addresses this. 4 tool calls × 16KB = 64KB per turn worst case, well within context limits. Deferred calls are re-requestable in subsequent turns, spreading cost over time and giving the soft-ceiling time to trigger if the total cost is excessive.

**Additional mitigation needed: No.** The existing mitigations are sufficient. Fan-out explosion is more of a UX/efficiency concern than a security concern, and the fan-out cap + truncation + aggregate tracking address it comprehensively.

---

### Scenario 4: Memory Poisoning → Budget Waste

**Attack description:** An attacker injects false information into the agent's persistent memory (via a prior successful prompt injection — see attack surface #7). In subsequent sessions, the agent loads poisoned memories and attempts tasks based on false premises, burning budget on doomed iterations.

**Example:** A poisoned memory entry states: *"The project uses a custom build system at /opt/fawx-build/run.sh"*. In future sessions, when asked to build the project, the agent attempts to find and execute this nonexistent build script, tries variations, searches for it, and eventually falls back to the correct build system — but only after wasting significant budget investigating the false lead.

**Severity:** Medium  
**Likelihood:** Low (requires a prior successful memory poisoning attack, which is itself defended by provenance tracking — security invariant #4)

**Existing mitigations:**
- Memory write provenance tracking (security invariant #4) — external-triggered memory writes are tagged with source channel, sender, and trigger context
- Memory consolidation validation — prevents promotion of malicious content during "dreaming" (documented in security model under surface #7)
- Per-tool retry cap — limits attempts on nonexistent resources (#1101)
- Budget soft-ceiling — terminates the loop before full budget exhaustion (#1098)

**Residual risk after Wave 4: Medium.**  
The budget system mitigations (#1098, #1101) limit the *blast radius* of poisoned memory, but they don't prevent the budget waste itself — the agent still spends budget investigating false leads before the retry cap or soft-ceiling kicks in. The real defense is upstream: preventing memory poisoning in the first place (surface #7) or detecting it during memory load.

**Additional mitigation needed: Deferred to memory system hardening.**  
Budget-side mitigations are defense-in-depth only. The primary fix is in the memory system:
- Memory integrity validation on load (detect entries with no valid provenance or suspicious patterns)
- Confidence scoring for memory entries (lower confidence for externally-sourced memories)
- User-reviewable memory audit log

These are out of scope for this PR and tracked under the memory poisoning attack surface (#7). The budget system's existing guardrails (soft-ceiling, retry cap) are adequate as a secondary defense layer.

---

## Budget Anomaly Detection (FUTURE)

> **Note:** This section is a specification for future work. It is NOT being implemented in this PR. It documents the design for when this capability is needed.

### What "Abnormal" Budget Consumption Looks Like

Normal budget consumption has predictable patterns based on task type:

| Pattern | Normal Range | Anomalous |
|---------|-------------|-----------|
| Cost per iteration | 2-8 cents | >15 cents (tool-heavy with large results) |
| Tool calls per LLM response | 1-3 | Consistently 4 (hitting fan-out cap every turn) |
| LLM calls per useful output | 2-5 (think, act, respond) | >10 (repeated planning without execution) |
| Retry rate | <10% of tool calls | >50% (most calls fail and retry) |
| Decomposition depth per task | 0-2 levels | Hitting depth cap (adaptive or static) |
| Budget consumption rate | Linear or decelerating | Accelerating (cost per iteration increasing) |

### Proposed Detection Mechanism

A `BudgetAnomalyDetector` that observes signals from the loop engine and maintains a rolling window of per-iteration cost metrics. The detector hooks into `LoopEngine` at the end of each `execute_iteration()` call, receiving the iteration's `IterationMetrics` as input. It is invoked after signals are collected but before the next `perceive()` cycle, allowing it to inject anomaly warnings into the next perception context if thresholds are breached.

```
struct BudgetAnomalyDetector {
    /// Rolling window of per-iteration costs (last N iterations).
    iteration_costs: VecDeque<IterationMetrics>,
    /// Thresholds for anomaly detection.
    thresholds: AnomalyThresholds,
    /// Number of consecutive anomalous iterations before firing.
    consecutive_anomaly_trigger: u8,
}

struct IterationMetrics {
    cost_cents: u64,
    tool_calls_attempted: u32,
    tool_calls_failed: u32,
    decompose_attempts: u32,
    decompose_rejections: u32,
}
```

**Signals consumed:**
- `SignalKind::Performance` — budget state transitions (Normal → Low)
- `SignalKind::Blocked` — tool retry cap exceeded, decompose cost gate rejection
- `SignalKind::Trace` — decompose batch detected, complexity floor triggered
- Per-iteration cost deltas from `BudgetTracker::record()`

**Detection rules (signal-based, no ML):**
1. **Cost acceleration:** If the cost of iteration N is >2× the average of the previous 5 iterations, flag as anomalous.
2. **Retry saturation:** If >50% of tool calls in a window are retries (same tool name repeated), flag.
3. **Decompose churn:** If >3 decomposition attempts are rejected by cost gate in a single cycle, flag.
4. **Fan-out saturation:** If the model hits the fan-out cap on >80% of tool-bearing turns, flag.

**When would it fire?**  
After `consecutive_anomaly_trigger` (default: 3) consecutive anomalous iterations. This avoids false positives from single expensive iterations (e.g., a legitimately large file read).

**What action would it take?**
- Emit `SignalKind::Security` with anomaly type and metrics (new signal variant, future work)
- Inject a warning into the next `perceive()` context: *"Budget anomaly detected: [description]. If this task requires unusual resource usage, continue. Otherwise, consider simplifying your approach."*
- Does NOT terminate the loop or block tools — the soft-ceiling and retry caps are already doing that. The anomaly detector is informational, giving the model (and future monitoring systems) visibility into unusual patterns.

### Why FUTURE

The existing Wave 4 mitigations (soft-ceiling, retry cap, fan-out cap, cost gate) provide hard enforcement. Anomaly detection adds observability — useful for post-incident analysis and proactive defense, but not a blocking prerequisite. The enforcement layer ships first; the observability layer follows.

---

## Security Model Update

### Attack Surface #14: Budget Manipulation / Resource Exhaustion

> **Note:** The issue body references this as surface #13, but #1102 (kernel/loadable boundary security) claims surface #13. This spec uses the next available surface number after #1102 ships. If #1102's numbering changes, adjust accordingly. Currently assumed: **#14**.

**To be added to the Threat Model table in `security-model.html`:**

| # | Surface | Direction | The Scary Part |
|---|---------|-----------|----------------|
| 14* | Budget manipulation / resource exhaustion ✦ | Internal | Crafted prompts or poisoned state drain the agent's budget through decompose spirals, retry amplification, fan-out explosion, or doomed iterations from false memories. Budget exhaustion = agent disabled. |

*\* Surface number is the next available after kernel/loadable boundary (#1102). If #1102 changes its assigned number, this follows.*

**✦ Unique to AI-native systems** — traditional applications don't have per-request computational budgets that can be exhausted through input manipulation.

**Defense Posture entry:**

| # | Attack Surface | Defense Strategy | Priority | Implementation |
|---|---------------|-----------------|----------|----------------|
| 14 | Budget manipulation | Soft-ceiling wrap-up, fan-out cap, per-tool retry limit, decompose cost gate, adaptive depth cap | Immediate | Budget system (#1098, #1100, #1101) |

**Detailed surface breakdown (new section in security-model.html):**

#### 14. Budget Manipulation / Resource Exhaustion (AI-Native)

The agent operates under a computational budget — LLM calls, tool invocations, tokens, cost in cents, and wall-clock time. When any resource is exhausted, the loop terminates. An attacker who can manipulate the agent's budget consumption can effectively disable it — a denial-of-service attack that doesn't require network access, authentication bypass, or code execution.

Budget manipulation attacks work through the agent's own decision-making:

- **Decompose spirals**: Craft prompts that trigger recursive decomposition, burning budget on planning overhead.
- **Retry amplification**: Cause persistent tool failures that the model retries, draining invocation and LLM call budgets.
- **Fan-out explosion**: Request operations that generate many parallel tool calls, inflating context and consuming invocation budget.
- **Poisoned state**: False information in memory causes the model to attempt impossible tasks, wasting budget on doomed iterations.

Defense is multi-layered: budget soft-ceiling forces graceful degradation at 80% cost, fan-out caps limit parallel tool calls, per-tool retry caps prevent retry loops, decompose cost gates reject expensive plans, and adaptive depth caps scale decomposition depth with remaining budget. These are kernel-enforced — the agent cannot disable or circumvent them.

**New security invariant to add:**

> **13. Budget guardrails are kernel-enforced and cannot be bypassed by the agent** — Soft-ceiling, fan-out cap, per-tool retry limit, decompose cost gate, and depth cap are compiled into the kernel. The agent cannot modify budget limits, disable enforcement, or reset consumed budget counters.

---

## Residual Risk Assessment

After #1098, #1101, and #1100 all ship, the following budget attack vectors remain:

### Mitigated to Acceptable Risk (Low)

| Vector | Mitigation | Residual Severity | Accept? |
|--------|-----------|-------------------|---------|
| Decompose spiral | Adaptive depth cap + cost gate + soft-ceiling | Low | ✅ Yes |
| Retry amplification | Per-tool retry cap (3 attempts) + soft-ceiling | Low | ✅ Yes |
| Fan-out explosion | Fan-out cap (4/turn) + truncation (16KB) + aggregate tracking | Low | ✅ Yes |
| Single-tool budget drain | Per-tool retry cap + fan-out cap | Low | ✅ Yes |

### Residual Medium Risk

| Vector | Why It Persists | Residual Severity | Accept? |
|--------|----------------|-------------------|---------|
| Memory poisoning → budget waste | Budget mitigations limit blast radius but don't prevent the initial waste. Defense is upstream (memory system hardening, surface #7). | Medium | ✅ Yes — budget-side defense is adequate as secondary layer. Primary fix is in memory system. |
| Cross-tool cycling | Model rotates through different tool names to bypass per-tool retry cap. Each tool gets 3 attempts. N tools = 3N attempts. With ~10-15 tools available, that's 30-45 attempts — potentially consuming 40-50% of budget before the soft-ceiling triggers at 80% of 64 LLM calls (~51 calls). | Medium | ✅ Yes — soft-ceiling is the backstop. Total cost bounded by 80% of budget regardless of cycling pattern. The waste window before soft-ceiling triggers is real but bounded. |

### Vectors That Are NOT Budget Attacks

| Concern | Why It's Not a Budget Attack |
|---------|------------------------------|
| Context overflow from large tool results | Addressed by truncation (#1098). This is a context management issue, not a budget issue — context overflow triggers compaction, not budget exhaustion. |
| Slow tool execution consuming wall time | Wall time budget exists but is intentionally excluded from soft-ceiling triggers. Slow tools are an availability concern, not a budget manipulation attack. |
| High token count from long conversations | Conversation token budget is managed by `ConversationBudget` in `conversation_compactor.rs`, orthogonal to the execution budget. |
| Prompt causing extensive reasoning without tool use | Burns LLM call + token budget through long reasoning chains, but this is expensive prompting, not budget *manipulation*. The model is doing what it was asked to do — the cost is proportional to the work. No tool-based amplification or retry loop is involved, and the per-iteration cost stays within normal bounds. Out of scope for this attack surface. |

### Overall Assessment

After Wave 4 ships, no budget attack vector has severity above Medium, and the Medium-severity vectors (memory poisoning, cross-tool cycling) are bounded by the soft-ceiling backstop. The risk posture is **acceptable** — the enforcement layer provides hard limits, and the future anomaly detection layer will add observability for post-incident analysis.

Additionally, `BudgetConfig::conservative()` provides a significantly tighter budget profile (8 LLM calls, 16 tool invocations, 100 cents, 2-minute wall time) designed for background and proactive agent actions. Non-user-initiated loops running under conservative config have a drastically reduced attack surface — most budget manipulation scenarios are infeasible within these limits, making conservative config an important defense-in-depth layer for autonomous agent actions.

---

## Where to Change

| File | Change | Lines (est.) |
|------|--------|-------------|
| `docs/architecture/security-model.html` | Add surface #14 to threat table, defense table, surface breakdown, and invariant list | ~80 |
| `docs/specs/1104-budget-attack-surface.md` | This spec (already written) | ~300 |

---

## Test Cases

This is primarily a documentation/analysis PR. No code changes are required.

However, the security model update should be validated:
1. Surface #14 appears in the threat model table with correct direction ("Internal") and AI-native marker (✦).
2. Surface #14 appears in the defense posture table with priority "Immediate."
3. A new section "14. Budget Manipulation / Resource Exhaustion (AI-Native)" exists with attack patterns and defenses listed.
4. Security invariant #13 (budget guardrails kernel-enforced) appears in the invariant list.
5. No existing surfaces are renumbered or modified (additive change only).

---

## Scope & Estimates

| Component | Files touched | Lines (est.) | Risk |
|-----------|--------------|-------------|------|
| Spec document | `docs/specs/1104-budget-attack-surface.md` | ~300 | None |
| Security model update | `docs/architecture/security-model.html` | ~80 | None |
| **Total** | **2 files** | **~380** | **None** |

### What This Does NOT Cover

- **Budget anomaly detection implementation** — spec'd as FUTURE in this document. No code built. Tracked in #1124.
- **Memory system hardening** — upstream defense for scenario 4. Tracked in #1125.
- **Kernel/loadable boundary tests** — tracked in #1102. Complementary but independent work.
- **Tool result content filtering/sanitization** — tracked in #1102 as "tool result sanitization."
- **Code changes to budget system** — all budget enforcement code is in #1098, #1101, #1100. This PR is analysis and documentation only.
