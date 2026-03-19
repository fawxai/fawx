# Kernel Capability Roadmap

## Objective
Increase agent capability and execution quality without weakening the immutable safety boundary.

---

## Prioritization Framework
- **P0 (0–2 weeks):** Highest leverage, low architecture risk, immediate user-visible impact.
- **P1 (1–2 months):** Medium complexity, strong capability and reliability gains.
- **P2 (Quarter+):** Deeper architectural investments with large long-term upside.

---

## P0 — Quick Wins (0–2 weeks)

### 1) Policy Contract API (Read-Only)
**Why now:** Reduces avoidable permission/tool/path failures before execution starts.

**Deliverables**
- Add a machine-readable endpoint describing active constraints (working_dir, allowed tools, path restrictions, network/tool policy, retry limits).
- Expose a concise planner-facing struct for preflight checks.

**Acceptance Criteria**
- Planner can query policy once and validate a plan before first tool call.
- Reduction in policy-denied tool attempts in traces.

**Suggested Rust Surface**
```rust
pub struct PolicyContract {
    pub working_dir: String,
    pub allowed_tools: Vec<String>,
    pub writable_roots: Vec<String>,
    pub max_parallelism: usize,
    pub retry_budget: usize,
}

pub trait PolicyInspector {
    fn current_contract(&self) -> PolicyContract;
}
```

---

### 2) Structured Failure Taxonomy + Recovery Hints
**Why now:** Converts opaque failures into deterministic recovery behavior.

**Deliverables**
- Standard error classes: `PermissionDenied`, `Timeout`, `ValidationFailed`, `TransientNetwork`, `BudgetExceeded`.
- Attach optional `next_actions` hints (e.g., decompose, reduce scope, request approval).

**Acceptance Criteria**
- All kernel/tool errors map to canonical classes.
- Planner can branch on class without string matching.

**Suggested Rust Surface**
```rust
pub enum KernelErrorClass {
    PermissionDenied,
    Timeout,
    ValidationFailed,
    TransientNetwork,
    BudgetExceeded,
    Unknown,
}

pub struct KernelError {
    pub class: KernelErrorClass,
    pub message: String,
    pub next_actions: Vec<String>,
}
```

---

### 3) Budget Governor v1
**Why now:** Directly addresses retry spirals and incomplete tasks.

**Deliverables**
- Per-task counters for tool calls, retries, elapsed wall time.
- Adaptive backoff and hard stop with actionable reason.
- Early recommendation to decompose when thresholds are crossed.

**Acceptance Criteria**
- Retry storms terminate predictably with guidance.
- Improved task completion rate under fixed budget.

---

### 4) Agent-Centric Observability (Minimal)
**Why now:** Needed to tune behavior and validate roadmap outcomes.

**Deliverables**
- Trace IDs for decision/tool calls.
- Metrics: denial count, retries, average tool latency, budget burn.
- Compact query endpoint for recent runs.

**Acceptance Criteria**
- Can explain why a task stopped in one trace query.
- Baseline metrics available for P1/P2 comparisons.

---

## P1 — Core Capability Upgrades (1–2 months)

### 5) Capability-Scoped Permissions
**Why now:** Enables more autonomy with tighter least-privilege controls.

**Deliverables**
- Tokenized capabilities with scope + TTL + task binding (e.g., `fs.read:project`, `exec:cargo-test`).
- Revocation and audit logging.

**Acceptance Criteria**
- Tasks request minimal capability bundles.
- Elevated actions are attributable and revocable.

---

### 6) Transactional Side-Effect Framework
**Why now:** Makes multi-step actions safe and reversible.

**Deliverables**
- Extend atomic transaction model from files to kernel-managed side effects (memory writes, git operations, scheduling).
- Commit/rollback plans with validation gates.

**Acceptance Criteria**
- Failed validation leaves no partial side effects.
- Transaction audit trail records intent and outcome.

---

### 7) Native Parallel Subgoal Scheduler
**Why now:** Improves throughput and consistency for multi-step tasks.

**Deliverables**
- Kernel-managed parallel execution with quotas and cancellation trees.
- Shared intermediate artifact channel.

**Acceptance Criteria**
- Parallel tasks respect global budget and policy limits.
- One failing branch can be isolated/cancelled without corrupting siblings.

---

### 8) Human-in-the-Loop Risk Checkpoints
**Why now:** Preserves velocity while keeping risky actions controlled.

**Deliverables**
- Risk scoring for planned actions.
- Auto-execute low risk, batch-confirm medium risk, explicit confirmation high risk.

**Acceptance Criteria**
- Fewer unnecessary prompts for low-risk operations.
- High-risk actions consistently require explicit consent.

---

## P2 — Deep Architecture Investments (Quarter+)

### 9) Deterministic Execution Envelopes
**Why now:** Enables replay, debugging, and trust at scale.

**Deliverables**
- Snapshot execution profile (env, tool versions, policy state, cwd).
- Re-run capability for postmortem and regression validation.

**Acceptance Criteria**
- Replays produce equivalent decision trace under same inputs.

---

### 10) Trust Tiers for WASM Plugins/Tools
**Why now:** Expands extensibility without widening attack surface.

**Deliverables**
- Signed plugin verification.
- Tiered permissions and sandbox profiles.
- Per-tier audit requirements.

**Acceptance Criteria**
- Unsigned/elevated plugins blocked per policy.
- Tier transitions are explicit and logged.

---

### 11) Content-Addressed Workspace Snapshots
**Why now:** Enables safe experimentation and instant rollback.

**Deliverables**
- Fast snapshot/restore for workspace + relevant state.
- Snapshot references in traces/transactions.

**Acceptance Criteria**
- Experimental runs can revert to clean baseline in seconds.

---

### 12) Safe Capability Learning Loop
**Why now:** Improves autonomy while preserving least privilege.

**Deliverables**
- Learn minimal successful capability bundles by task class.
- Suggest permissions, never auto-escalate beyond policy.

**Acceptance Criteria**
- Reduced over-provisioned capability requests over time.
- No policy boundary bypasses.

---

## Dependency Order
1. Policy Contract API
2. Failure Taxonomy
3. Budget Governor + Observability
4. Capability-Scoped Permissions
5. Transactional Side Effects
6. Parallel Scheduler + Risk Checkpoints
7. Deterministic Envelopes + Trust Tiers + Snapshots
8. Capability Learning Loop

---

## Success Metrics
- **Reliability:** task completion rate, rollback success rate, validation pass rate.
- **Efficiency:** average tool calls per completed task, retry rate, budget overrun rate.
- **Safety:** policy denial rate (preflight vs runtime), unauthorized side-effect attempts, high-risk confirmation compliance.
- **UX:** median time-to-completion, interruptions per task, user approval friction.

---

## Recommended First Sprint (2 weeks)
- Implement Policy Contract API + planner preflight.
- Introduce canonical KernelError classes and mapping.
- Add Budget Governor thresholds with deterministic stop reasons.
- Ship minimal observability counters and trace IDs.

**Expected impact:** immediate drop in failed attempts, fewer wasted retries, faster successful task execution under current constraints.
