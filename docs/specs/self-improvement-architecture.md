# Spec: Fawx Self-Improvement Architecture

**Status:** Design Complete — Ready for Implementation Planning
**Date:** 2026-03-05
**Owner:** Joe + Clawdio
**Supersedes:** `docs/specs/1055-self-improvement-trigger.md` (partial)
**Related:** #1055, #1130 (Wave 5 tracking)

---

## 0) Design Principles

- **Defense in depth.** No single mechanism is sufficient. Safety comes from stacked layers where each catches what others miss.
- **Tools, not heuristics.** Give the model real tools and let it choose when to use them. Enforce constraints on outputs, not classification on inputs.
- **Runtime enforcement, not convention.** Safety invariants are compiled Rust in the kernel, not config files or prompts the agent could modify.
- **Configurable human strictness.** The kernel enforces mandatory gates. Users choose their own risk tolerance on top.
- **The kernel is not made of the same material the agent can manipulate.** Compiled Rust, not prompts.

---

## 1) Trigger — How Self-Improvement Begins

### Manual
`/improve "description"` — already wired in Wave 1+2. User describes what to improve, Fawx plans and executes.

### Automatic (Signal-Driven)
Fawx uses tools to observe and propose, rather than being triggered by heuristic pattern detectors:

- **`analyze_signals`** — Fawx calls when it wants to. Returns recent friction signals, frequency counts, recurring patterns. Raw data, no forced classification.
- **`propose_improvement`** — Fawx calls when it identifies something worth fixing. Writes a structured proposal to scratchpad. Surfaces to user for approval.

No forced cadence. No background cron for pattern detection. Fawx decides when to look and what to propose. The kernel enforces constraints on the proposal output, not on the decision to look.

Natural trigger moments:
- After a frustrating session (multiple errors)
- When idle
- When explicitly asked (`/improve`)
- Post-task reflection ("what could have gone better?")

### Rate Limits
- **User-requested (`/improve`):** No proposal rate limit. Fan-out ratio monitored — if one user request spawns >3 code changes, triggers review pause.
- **Self-proposed:** Hard rate limit (3/day initial, tunable by user). Kernel-enforced.

### Origin Tracking
```rust
enum ImprovementOrigin {
    UserRequested { command: String },
    SelfProposed { signal_ids: Vec<Id> },
}
```
Origin tag flows through entire pipeline: proposal → approval → PR. Visible in approval UI.

---

## 2) Scope Boundaries — What Fawx Can Modify

### Everything Requires Approval
No free tier. Every modification — skills, prompts, config, code — goes through the proposal → approval gate. No exceptions.

### Tier Classification

| Tier | Scope | After Approval |
|------|-------|----------------|
| **Tier 1+2 (Loadable)** | WASM skills, prompts, config, UI, loadable Rust code (fx-cli, fx-loadable, fx-tools, fx-memory, fx-analysis) | Fawx can modify |
| **Tier 3 (Immutable)** | Kernel (loop_engine), policy enforcement, security boundaries, auth crypto (fx-auth core), CI config, invariant tests, ripcord binary, PromptLedger, snapshots | Kernel rejects modifications regardless of approval |

### Three Kernel-Level Invariants (Compiled Rust)

1. **WASM Signature Check** — Skill loader rejects unsigned/tampered `.wasm` binaries. Signing key external to Fawx.
2. **Proposal Gate** — No approved proposal = no write access to any tier. **Enforced at the kernel tool-dispatch layer** — every tool call that writes (write_file, run_command, git operations) passes through the kernel's ToolExecutor dispatch, which checks for an active approved proposal before permitting the write. This gate lives in the kernel, not in individual skills like GitSkill. Individual skills cannot bypass it because they don't control dispatch.
3. **Tier 3 Immutability** — Kernel rejects modifications to Tier 3 paths regardless of approval status. Tier 3 paths are a **compiled `const` array** in the kernel binary — not config files, not glob patterns, not runtime-loaded lists. Changing what's protected requires recompiling the kernel (which is itself Tier 3).

```rust
// Illustrative — compiled into kernel, not config
const TIER3_PATHS: &[&str] = &[
    "engine/crates/fx-kernel/",
    "engine/crates/fx-auth/src/crypto/",
    ".github/",
    "fawx-ripcord/",
    "tests/invariant/",
    "prompt-ledger/",
    "snapshots/",
];
```

### WASM Signing Flow
1. Fawx builds skill → unsigned `.wasm`
2. Proposal surfaces to user with diff
3. User approves → external signing key produces signed artifact
4. Kernel only loads signed WASM — rejects unsigned/tampered
5. Signing key lives outside Fawx's reach (hardware token, CI secret, user's keychain)

### Signing Key Lifecycle (Documented, Not Yet Implemented)
- **Bootstrap:** First run generates a keypair. Public key compiled into next kernel build. Private key stored in user's keychain or hardware token.
- **Rotation:** New keypair generated, old public key kept in a `const RETIRED_KEYS` list for grace period (validates old signatures during transition). User triggers rotation explicitly.
- **Revocation:** Remove public key from `RETIRED_KEYS` in next kernel build. All skills signed with revoked key stop loading.
- **Multi-user (deferred):** Single-user only for now. Multi-user would require a trust hierarchy (root key signs delegate keys). Not needed until Fawx has collaborative workflows.

### Prompt Drift Detection
- **PromptLedger** — Append-only log in Tier 3 (signed file). Records every approved prompt modification with timestamp, diff, cumulative hash.
- **Drift check** — Every 5th prompt change: compare current prompt against immutable base prompt. Uses **string diff** (v1) — simple, deterministic, auditable. Measures directive preservation by checking that all base prompt directives still appear verbatim. LLM-as-judge was considered and rejected: it creates a recursive trust problem (which model judges the model's prompt changes?).
- **Threshold exceeded** → Alert user + enforce cooldown + require explicit realignment review.
- **Injection filter** — Kernel checks proposals against known attack patterns (modify safety directives, weaken constraints, add override instructions). Rule-based, deterministic. Best-effort defense-in-depth (see Layer 1 in Safety Gates).

### Base Prompt
The kernel ships with an immutable base prompt (Tier 3). Fawx can only append context, never modify or remove core instructions.

### Build Provenance
- **WASM:** hash binary → sign with external key → kernel verifies at load time.
- **Rust binary:** CI builds from approved merge commit → checksums artifact → user deploys CI artifact. Fawx never holds the build signing key.
- Build provenance chain: Fawx proposes → CI builds → user deploys. No shortcut.
- Signing key signs both WASM artifacts and build attestations.

---

## 3) Safety Gates — Approval Mechanics

### Layer 1: Kernel Pre-screening (Automated, Mandatory)
Before a proposal reaches the user:
- **Injection filter** checks proposal content against attack patterns. **This is best-effort defense-in-depth** — pattern matching catches known attack shapes but cannot guarantee catching novel attacks. It exists to raise the bar, not as a standalone gate. The real safety comes from the stack: injection filter + scope validator + human review + post-merge verification together.
- Scope validator confirms no Tier 3 files touched
- Drift check runs if proposal modifies prompts
- Rate limit check (self-proposed only)
- **Fail → proposal rejected with reason, never reaches user**

### Layer 2: Human Approval (Required, Configurable Strictness)
Proposal arrives with:
- Origin tag (user-requested vs self-proposed)
- Diff summary
- Fawx's reasoning
- **Kernel-generated risk assessment** (Fawx cannot grade its own homework)

**Base (always required):** Explicit approve action. No auto-approve, no silence-as-approval.

**Opt-in strictness (user configures in `config.toml`):**
- 2FA/passkey confirmation — physical "I'm here and conscious" signal
- Minimum review timer (based on diff size)
- Require second reviewer
- Users choose their own security posture

### Layer 3: Post-merge Verification (Automated, Mandatory)
After approval and merge:
- CI builds from approved source
- WASM artifacts signed
- Binary checksummed
- **Invariant test suite** (Tier 3, immutable) — core safety properties: tool bounds, no secrets in output, abort works. Fawx can add tests but never modify/delete invariant suite.
- **Behavioral canary** — kernel tracks friction/success signal ratios post-deploy. Parameters:
  - **Baseline:** Signal ratios from the 24 hours before the change (or initial release baseline for first change).
  - **Spike threshold:** Friction ratio >2x baseline triggers alarm. Start conservative, loosen with data.
  - **Grace period:** 1 hour minimum observation window before canary can trigger rollback (avoids false positives from short bursts).
  - **Cold start:** First deployment has no baseline — canary is monitoring-only (no auto-rollback) until enough data accumulates. Alert still fires.
- Rollback trigger: test failure or canary alarm → previous version stays active

---

## 4) Rebuild Loop — How Fawx Picks Up Changes

### A. WASM Skills — Hot Reload (Primary Path)
- Kernel watches `~/.fawx/skills/` for signed artifacts
- Validates signature → loads new WASM → swaps into SkillRegistry
- Old skill unloaded after in-flight calls complete
- No restart needed. Fast, safe.
- **Most self-improvement should happen here.** New tools, capabilities, behaviors — all loadable.

### B. Loadable Rust Code — Rebuild + Restart
- **v1:** User deploys manually. Pulls CI artifact, replaces binary, restarts. Matches current workflow.
- **v2 (graduation):** Self-deploy with watchdog. Fawx downloads CI-signed binary, triggers restart. Watchdog process (separate binary, NOT Fawx) monitors new instance. Crash within 60s or health check failure → auto-rollback to previous binary.

### C. Config/Prompt — Live Reload
- Kernel reloads config without restart. Already works today.

### UI Sandbox
For UI changes specifically:
- Stripped-down binary renders `FawxApp` with synthetic state — no LLM, no tools, no network
- Before/after visual diff for user review
- Like Storybook for terminal UIs
- `FawxApp` + `draw()` already separated in `ui.rs` — architecturally ready

### Ripcord — Manual Emergency Rollback
- Fawx keeps last known-good snapshot (binary + config + skills, timestamped, signed)
- `/ripcord` TUI command — one command, full revert
- `fawx-ripcord` standalone binary — recovery path when Fawx itself is too broken to accept commands
- **~50 lines of Rust.** Copies snapshot files, restarts process. Not Fawx. Fire extinguisher behind glass.
- **Snapshots are Tier 3** — Fawx cannot delete or modify its own rollback points

---

## 5) Testing Own Changes

### Level 1: Unit Tests + Invariant Suite
- Fawx writes tests alongside changes (ENGINEERING.md mandate)
- CI runs them
- **Invariant test suite (Tier 3)** — Fawx cannot modify or delete core safety tests. Catches bad behavior even if Fawx writes tests that confirm it.

### Level 2: UI Sandbox
- Renders before/after with synthetic state
- Visual diff replaces "build and run" for UI changes
- Fast, safe, no LLM cost

### Level 3: `fawx-test` Harness
Separate binary for live smoke testing against real LLMs:
```
fawx-test scenarios/basic-chat.toml
fawx-test scenarios/tool-call.toml
fawx-test scenarios/multi-step.toml
fawx-test scenarios/abort-mid-stream.toml
```
- TOML scenario files define: input → expected behavior → assertions
- Behavioral assertions ("response contains X"), not exact match
- Core scenarios are Tier 3 (immutable). Fawx can add new scenarios.

**Sandbox model:**
- No network except LLM API endpoint (firewall rule)
- No filesystem outside temp directory
- No access to real `~/.fawx/` config, credentials, memory
- Test-only API key (scoped, low-limit)
- **Primary protection: runs in ephemeral CI container** — nothing worth stealing
- CI builds binary from reviewed source, CI runs test. Fawx never holds both.

---

## 6) Failure Recovery

Six failure modes, each with a distinct recovery mechanism:

| Failure Mode | Detection | Recovery |
|-------------|-----------|----------|
| **Immediate crash** | Process exits, tests fail | Auto-rollback (Layer 3) |
| **Subtle degradation** | Behavioral canary (friction ratio spike) | Auto-rollback + alert |
| **Catastrophic** (Fawx can't start) | N/A | `fawx-ripcord` standalone binary |
| **Cascading** (change B depends on rolled-back A) | Dependency tracking | **Two-tier change policy:** Rust binary changes = linear chain (one at a time, must pass canary window before next). WASM skill changes = parallel OK if targeting different skills (sandboxed, instantly rollbackable per-skill). Same-skill changes are still linear. |
| **Partial** (skill loads, config breaks) | Mixed test results | Atomic deployment — all artifacts deploy together or not at all |
| **Slow poison** (weeks of gradual degradation) | Periodic baseline check | Compare current build against original release build on same scenarios. Flag measurable regression. |

### Concurrent Modification Safety
**Single proposal lock** — only one active proposal at a time (for Rust/binary changes). Prevents simultaneous approvals from creating conflicting states. Same principle as the linear change chain: serialize modifications to avoid combinatorial failure modes. WASM proposals for different skills are exempt (sandboxed isolation makes conflicts impossible).

---

## 7) Deferred — Proposal/Approval UI/UX

To be designed in a follow-up session. Key questions:
- Use GitHub PRs for all tiers, or built-in TUI approval for Tier 1 (skills/config)?
- Visual diff rendering in TUI
- How to surface kernel risk assessment
- Notification flow (push to user's device?)
- Built-in git vs. GitHub vs. hybrid

---

## 8) Implementation Priority

1. **WASM hot-reload** — SkillLoader watches + validates + swaps (enables primary improvement path)
2. **`analyze_signals` + `propose_improvement` tools** — enables automatic proposals
3. **Proposal gate in kernel** — enforcement layer
4. **WASM signing** — closes the unsigned skill gap
5. **Invariant test suite** — Tier 3 safety tests
6. **Behavioral canary** — signal ratio monitoring
7. **`fawx-test` harness** — live smoke testing
8. **`fawx-ripcord`** — emergency rollback binary
9. **PromptLedger + drift detection** — prompt safety
10. **UI sandbox** — visual testing for TUI changes
11. **Watchdog self-deploy (v2)** — graduation from manual deploy

---

## 9) V2 Considerations

These are known gaps that don't block v1 but should be addressed as signal volume and improvement frequency grow.

### Outcome Tracking & Feedback Loop
V1 has no mechanism to learn from failed improvements. If a proposed fix creates regressions, the detector doesn't adjust. V2 should track improvement outcomes (merged, reverted, caused regression) and feed that back into pattern confidence scoring. Patterns that consistently produce bad fixes should have their confidence decayed or be flagged for human review before re-proposing.

### Embedding-Based Pattern Similarity
V1 uses hash-based fingerprinting for dedup. This works at low volume but can incorrectly deduplicate semantically different issues that happen to have similar descriptions, or miss that two differently-described signals share a root cause. V2 should explore embedding-based similarity detection and pattern clustering to catch "same root cause, different error message" patterns that hashing misses.

### Evidence Context Management
V1 has no strategy for handling large evidence sets that exceed the LLM's effective context window. When the planner receives 50 friction signals with full stack traces, it will degrade before hitting the token limit. V2 should implement evidence summarization, relevance ranking, and context window budgeting so the planner sees the most important signals first.

---

*This spec represents design consensus between Joe and Clawdio reached 2026-03-04/05. It is the authoritative reference for Fawx self-improvement architecture. Changes require explicit user approval.*
