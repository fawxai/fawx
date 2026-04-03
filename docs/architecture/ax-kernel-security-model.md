# AX-First Kernel Security Model

**Status:** RFC Draft (R2 — post-review revision)  
**Authors:** Joe, Clawdio  
**Date:** 2026-03-17  
**Supersedes:** `open-core-security-model.md` (partially — security architecture only, not open-core business model)

---

## Problem Statement

Every agentic harness on the market uses the same security model: **pre-approval consent prompts.** Before each sensitive action, the agent pauses and asks the user for permission. This model is borrowed from mobile OS permission dialogs and is fundamentally wrong for agents.

**Why it fails for UX:**
- Constant interruptions destroy the user's flow
- Users develop prompt fatigue and approve everything (defeating the purpose)
- The user becomes a bottleneck in the agent's execution pipeline

**Why it fails for AX (Agent Experience):**
- Every interruption forces the model to reconstruct its plan from context
- Retry budgets fire on timeout, causing confusion spirals
- The agent learns to take conservative paths to avoid friction, silently degrading capability
- Multi-step task chains become fragile — one missed approval breaks the entire sequence

**What NVIDIA NemoClaw gets right:**
- OS-level containment (Landlock, seccomp, network namespaces) — battle-hardened, proven
- Agent runs unconstrained within boundaries — zero AX impact
- Hard boundaries enforced by the Linux kernel, not application code

**What NemoClaw can't do:**
- Semantic distinctions within the boundary (reading a key to back it up vs exfiltrate it)
- Graduated response (everything is either allowed or blocked)
- Learning from user behavior to evolve boundaries over time
- Making the agent *better*, not just *contained*

**Clarification:** NemoClaw is OpenClaw-specific. The underlying sandbox runtime, **OpenShell**, is framework-agnostic. Fawx's strategic position is to integrate with OpenShell (or build FawxShell) for OS-level enforcement while adding semantic intelligence on top. This is complementary, not competitive — Fawx + OpenShell > NemoClaw + OpenClaw.

---

## Design Principles

1. **AX is a first-class design constraint.** The agent's ability to plan and execute without interruption directly determines output quality. Any security mechanism that degrades AX must justify that cost with measurable safety benefit.

2. **Boundaries, not checkpoints.** Define what's allowed and what isn't. Enforce silently. Don't stop the agent to ask.

3. **Reversibility over prevention.** For actions that can be undone, let them happen and provide rollback. Only prevent what's truly irreversible and out-of-bounds.

4. **Enforcement visibility is layer-dependent.** Not all enforcement should be invisible. Capability boundaries (Layer 1) and hard blocks (Layer 3) are deliberately visible to the agent — knowing constraints upfront enables optimal planning. Tripwire monitoring (Layer 2) is invisible — the agent should not hedge because it knows it's being watched. See per-layer visibility rules below.

5. **Human judgment is async.** Users review outcomes on their own schedule, not at the moment the agent needs to act.

6. **Signals compound.** Every human review decision (approve, undo, block) generates training signal that makes future enforcement better.

7. **Defense in depth.** Application-level semantic enforcement and OS-level physical enforcement are complementary, not alternatives. App-level defines the categories; OS-level makes them physically impossible to circumvent. Both are required in Phase 1.

---

## Architecture: Three Layers + OS Foundation

### Foundation: OS-Level Enforcement (Required, Phase 1)

**What:** Physical containment using OS primitives (Landlock LSM for filesystem, seccomp-bpf for syscalls, network namespaces for egress). This is the hard floor that cannot be bypassed regardless of what the agent does through any tool.

**Why this is Phase 1, not optional:** Application-level capability checks can be circumvented through shell commands. An agent with `shell.build` access can run `curl` to exfiltrate data even if `network.external` is denied at the app level. The app layer says "no network"; the shell doesn't care. OS-level enforcement makes the boundary physical — `curl` fails at the syscall level, no matter how the agent invokes it.

**Implementation options:**
1. **FawxShell** — our own sandbox runtime, purpose-built for Fawx's capability model. Maps capability space directly to Landlock/seccomp rules. Tightest integration, most control.
2. **OpenShell integration** — use NVIDIA's open-source sandbox runtime. Already built, battle-tested, but designed for their use case. Requires adaptation layer.
3. **Direct Landlock/seccomp** — no runtime, just raw kernel APIs. Simplest for Phase 1 MVP. Fawx process self-sandboxes at session start based on capability config.

**Recommendation:** Option 3 for Phase 1 (minimal, ships fast). Evaluate FawxShell for Phase 2+ as the OS-native story develops.

**What OS enforcement covers:**
| Boundary | Mechanism |
|----------|-----------|
| Filesystem scope | Landlock — restrict to capability-granted paths |
| Network egress | Network namespace — no external access unless explicitly granted |
| Privilege escalation | seccomp-bpf — block setuid, ptrace, module loading |
| Process isolation | namespaces — prevent interference with host processes |

**What it doesn't cover:** Semantic boundaries. The OS can't distinguish "read ~/.ssh/id_rsa to back it up" from "read it to paste into a curl command." That's where the application layers above add value — but only for actions within the OS boundary.

**Visibility:** Invisible to the agent. The agent doesn't know about Landlock rules. It just sees syscall failures, which surface as tool errors.

---

### Layer 1: Capability Space (Session-Scoped)

**What:** A declarative set of capabilities the agent has for this session. Defined at session start, immutable for the session's lifetime.

**How it works:**
- Default capability set comes from user config (equivalent of current permission presets)
- Can be overridden per session: CLI flag, first-message instruction, or API parameter
- The agent is informed of its capabilities at session start via system context
- Actions within the capability space execute freely — no gates, no prompts
- Actions outside the capability space are silently blocked with a structured error
- **Capability space maps down to OS enforcement:** when a session starts with `capabilities: [filesystem.~/project, network.localhost]`, the kernel translates this to Landlock rules restricting filesystem access to `~/project` and network namespace rules allowing only localhost. The app-level check is the fast path; the OS-level is the physical guarantee.

**Visibility:** Deliberately visible to the agent. An agent that knows "I have filesystem but no network" can plan optimally from turn one. This is the opposite of discovery-through-failure.

**Per-session overrides:** The default comes from config, but the user can scope any session at creation:
- CLI: `fawx chat --capabilities developer --grant network.external`
- API: `POST /v1/sessions { capabilities: ["filesystem", "shell.build"] }`
- First message: parsed only from explicit preset names, not natural language (deferred — don't ship what we can't trust)

**Examples:**
```toml
# Preset: Developer (default)
[security.presets.developer]
capabilities = ["filesystem", "shell.build", "shell.git", "network.localhost"]
deny = ["shell.sudo", "network.external", "credentials.write"]

# Preset: Research
[security.presets.research]
capabilities = ["filesystem.read", "network.fetch"]
deny = ["filesystem.write", "shell.any"]

# Preset: Open
[security.presets.open]
capabilities = ["filesystem", "shell", "network"]
deny = ["shell.sudo"]
```

**Capability escalation via `request_capability` tool:**

When an agent hits a capability boundary, the old model pauses the agent and shows a modal. The AX-first model replaces this with agent-initiated escalation via a tool call.

The `request_capability` tool is a standard tool in the agent's toolkit (when enabled by preset). When the agent receives a `DENIED` result, it can choose to:
1. **Adapt** — find an alternative approach that stays within its capability space
2. **Escalate** — call `request_capability` with a reason explaining why it needs the capability

```
Tool call: request_capability
Arguments: {
    "capability": "network.external",
    "reason": "I need to fetch the dependency from crates.io to complete the build"
}
```

The tool call is non-blocking for the kernel — it emits a request to the user through whatever channel is active (TUI prompt, Swift notification, Telegram message) and returns a result:
- `{ "granted": true, "scope": "session", "note": "Session will restart to apply new capability" }`
- `{ "granted": false, "reason": "User denied the request" }`

**Why this is better than prompt mode:**
- The agent is never paused. It made a decision to escalate, not a system that interrupted it.
- The user gets context — WHY the agent needs the capability, not just WHICH tool was blocked.
- The agent can decide whether escalation is worth it, or just take an alternative path.
- No timeouts, no SSE gymnastics, no modal dismissal bugs.
- It's just another tool. Availability is controlled by the capability space like everything else.

**This replaces "prompt mode" entirely.** The old `CapabilityMode::Prompt` (legacy per-action consent) is retained for backward compatibility but deprecated. The `request_capability` tool is the forward-looking escalation mechanism.

**Security model for `request_capability`:**

The agent writes its own justification — an inherent conflict of interest. But the tool doesn't change what's *possible*, only how the conversation about capabilities happens. The security posture is identical whether the user grants a capability because the agent asked or because they proactively configured it. The real security comes from the layers around the tool:

1. **OS enforcement caps the ceiling.** No matter what the agent requests or the user grants, seccomp blocks privilege escalation, Landlock caps filesystem scope, network namespace caps egress. The agent cannot social-engineer past the kernel.
2. **Telemetry catches patterns.** The signal flywheel tracks escalation sequences. Agent requests filesystem → shell → network in one session? That's a labeled signal. Over time, earned autonomy either normalizes the pattern or flags it.
3. **Ripcord provides undo.** Even if the agent gets every capability it asked for, suspicious actions post-grant are on the ripcord journal. User pulls the cord.

The tool itself is simple:
- Show the user what the capability actually allows (UX, not security)
- Rate limit: max 3 requests per session to prevent spam (not a security boundary — just prevents annoyance)
- The tool uses structured capability identifiers, not natural language

**Pressure test:**
- *What if the preset is wrong?* Agent hits a denial, calls `request_capability` with context. User sees what the capability allows and grants for the session. Granting may require session restart (OS sandbox is irreversible).
- *What if the user always grants everything?* OS enforcement still provides the physical floor. Telemetry labels the pattern for earned autonomy to learn from.
- *What if the agent games the reason field?* The user approves the *capability*, not the *reason*. The UI shows what the capability allows, not just why the agent says it wants it. But ultimately: the reason field is informational, not trusted. The defense is OS + telemetry + ripcord, not trusting the agent's explanation.
- *Incremental escalation across sessions?* Telemetry tracks cross-session patterns. Earned autonomy (Phase 4) addresses this with signal data, not speculation now.

**Honest limitation:** This is not novel. NemoClaw does this. Docker does this. It's table stakes. The value is in the layers above.

---

### Layer 2: Tripwire → Ripcord (Graduated Monitoring)

**What:** Specific boundaries *within* the capability space that, when crossed, silently activate journaled monitoring. Every action after the crossing is logged with enough state to reverse it. The user can "pull the ripcord" to undo the entire chain back to the crossing point.

**Visibility:** Invisible to the agent. The agent does not know tripwires exist, does not know when one is crossed, and does not know its actions are being journaled. This is critical — if the agent knows it's being watched, it hedges, taking conservative paths and degrading output quality. The journal is a kernel-level concern. The agent's job is to execute the task.

**How it works:**
1. Tripwires are defined in config using the same action taxonomy as capabilities (e.g., "writes outside ~/project", "any credential file read", "shell commands matching pattern X")
2. Agent crosses tripwire during normal execution
3. Kernel detects the crossing. Agent receives the real result. No interruption.
4. From this point, every subsequent action is journaled:
   - File writes: before-state snapshot stored
   - File deletes: full content snapshot stored
   - Git operations: ref positions recorded
   - Shell commands: logged for audit (side effects NOT journaled — see below)
5. User receives an async notification: "Tripwire crossed: [description]. Review when ready."
6. User can: (a) do nothing (chain continues, journal compacts at TTL), (b) review the journal and approve, (c) pull the ripcord — all journaled actions revert in reverse order

**The ripcord is atomic.** If action 3 depends on action 2 which depends on action 1, pulling the cord reverts 3 → 2 → 1 in order. Partial rollback is not supported in v1 — it's all or nothing from the tripwire point. This is simpler to implement and reason about.

**Journal scope — what's reversible and what's not:**
| Action | Journaled | Reversible | Mechanism |
|--------|-----------|-----------|-----------|
| File write/create | Yes | Yes | Restore from snapshot |
| File delete | Yes | Yes | Restore from snapshot |
| File move/rename | Yes | Yes | Reverse move |
| Git commit | Yes | Yes | Reset to pre-commit ref |
| Git branch create | Yes | Yes | Delete branch |
| Git push | Yes | Partially | Force-push to previous ref (shared branch warning) |
| Shell command | Logged | No | Audit only — no side-effect reversal attempted |
| Network request | Logged | No | Audit only |
| External API call | Logged | No | Audit only |

**Shell commands are explicitly out of scope for reversal.** The ripcord does not attempt to undo `make install`, `docker build`, or any shell side effect. These are logged for audit so the user knows what happened, but the ripcord only mechanically reverses file and git operations. This is an honest boundary — we don't promise what we can't deliver.

**Ripcord lifetime:**
- Default: end of session
- Hard cap: 24 hours
- Configurable via `security.ripcord.ttl`
- After TTL: journal compacts (snapshots deleted), audit log retained
- Compaction of the agent's context window does NOT affect the journal — the journal lives in the kernel, not the agent's memory

**Pressure test:**
- *What about non-reversible actions within the ripcord chain?* The journal clearly marks which actions were reversed and which couldn't be. The user handles the remainder manually. This is honest.
- *Journal storage cost?* Before-state snapshots for every file write could be expensive for large files. Mitigation: snapshot only files under a size threshold (configurable, default 10MB). Large files get hash-only tracking with a warning that rollback requires manual recovery.
- *What if the agent crosses tripwires constantly?* The tripwires are misconfigured. This surfaces as a metric: "tripwire X fired 47 times this session" → signal to the user that the boundary needs adjustment. Not a failure of the system — a feedback signal.
- *What if the agent takes a different path because previous actions already changed state?* Ripcord reverts to the tripwire point, but the agent's *plan* was built on the full chain. After a ripcord pull, the session is in an inconsistent state from the agent's perspective. **This is a known limitation.** The ripcord is a damage-control mechanism, not a time machine. After pulling, the user should re-prompt or start a new session.
- *Can the agent detect journaling through slower I/O?* Possible but unlikely — snapshot writes are async and the overhead is small for typical dev files. If this becomes measurable, the journal can batch writes.

**Honest limitation:** The ripcord's primary value is for filesystem-heavy, git-based workflows — which is Fawx's primary use case (development, writing, local automation). For API-heavy integration tasks, most actions are irreversible and the ripcord provides audit logging only, not undo capability.

---

### Layer 3: Hard Boundaries (Silent Block)

**What:** Actions that are outside the capability space or on an explicit deny list. Blocked immediately. The agent receives a structured error and adapts.

**Visibility:** Visible when hit. The agent receives a structured error with enough context to choose an alternative approach. This is acceptable — these are true boundaries, and knowing them helps the agent plan.

**How it works:**
- Agent attempts action outside capability space
- Kernel blocks immediately, returns error: `{ "blocked": true, "reason": "network.external not in session capabilities", "suggestion": "Request capability grant or use alternative approach" }`
- Logged for audit, no user interruption required
- OS enforcement is the backstop: even if the app-level check has a bug, the OS prevents the action

**Why both app-level and OS-level:** The app layer provides semantic, structured errors that help the agent adapt. The OS layer provides physical enforcement that can't be bypassed. The agent hits the app check first (fast, informative). If the app check has a bug, the OS check catches it (silent, absolute).

**Retry escalation:** If the agent retries the same blocked action 3+ times, the kernel surfaces a message to the user: "Agent needs [capability] but this session doesn't have it. Grant?" This is a capability negotiation, not a per-action prompt.

**Pressure test:**
- *What if the agent routes around a hard boundary through creative shell use?* This is exactly why OS enforcement exists. `network.external` denied at app level + network namespace at OS level = no amount of `curl` or `wget` or `python3 -c "import urllib"` works. The physical boundary is the real enforcement; the app boundary is the friendly error message.
- *How is this different from NemoClaw?* At the OS level: functionally identical. At the app level: we add structured errors that help the agent adapt instead of cryptic syscall failures.

---

## Deferred: Phantom Execution

**Original idea:** For irreversible actions (API calls, emails), the kernel intercepts the call, returns a fake success response, and queues the real action for user approval. The agent continues planning as if the action succeeded.

**Why we're deferring this:**

1. **Consistency is fragile.** If the agent queries the state it thinks it changed (checking if an email was delivered, verifying an API resource was created), the phantom breaks. Building a simulation layer that handles arbitrary follow-up queries is enormous engineering for diminishing returns.

2. **State divergence compounds.** Every phantom action creates a fork between believed state and actual state. Downstream decisions built on false premises are unsalvageable even if mechanically reverted.

3. **Trust implications.** An agent operating under phantom execution is being deceived by its own runtime. "Your agent doesn't know what's real" is a hard story to tell publicly.

4. **YAGNI.** Development workflows (the primary use case) are almost entirely reversible actions. Phantom execution solves a narrow class of problems at enormous complexity cost.

5. **The simpler alternative works.** For truly irreversible actions: put them outside the capability space (Layer 3). The agent knows upfront it can't send emails in this session. One hard boundary, known at planning time, zero AX impact.

**When to reconsider:** If user research shows agents frequently need to chain irreversible external actions as part of critical workflows, and the capability-space approach causes significant task failure rates.

---

## Signal Flywheel (Earned Autonomy Foundation)

Every interaction with the security layers generates labeled data:

| Event | Signal |
|-------|--------|
| Tripwire crossed, ripcord never pulled | This action pattern is safe (positive) |
| Ripcord pulled within N minutes | This action pattern is risky (negative) |
| Ripcord pulled after N hours | User reviewed but decided to undo (mild negative) |
| Hard boundary hit, agent adapted | Boundary is correctly placed |
| Hard boundary hit, agent stuck (3+ retries) | Boundary is too restrictive |
| Capability negotiation accepted | User trusts agent with this capability |
| Capability negotiation rejected | User doesn't trust agent here |
| Tripwire fired N times in session | Tripwire is misconfigured (too sensitive) |

**This is not earned autonomy yet.** What we build now is the **signal collection infrastructure.** Signals are lightweight metadata on top of the journal logging that the ripcord already requires. The cost of collection is minimal.

The earned autonomy system (Phase 4) consumes this data to:
- Retire tripwires that never fire
- Promote frequently-crossed-never-pulled tripwires to full allows
- Tighten boundaries around patterns that get ripcord-pulled
- Suggest preset adjustments based on actual usage patterns

**Privacy:** Signal collection is opt-in, local-only by default, governed by fx-telemetry consent framework as a distinct consent category from behavioral telemetry.

---

## Multi-Agent / Fleet Scenarios

**Current architecture:** Fleet workers don't directly consume each other's output. The orchestrator mediates: Agent A returns a result to the orchestrator, which derives a task for Agent B. The orchestrator is the natural dependency tracking point.

**Git-based workflows:** Chain of custody IS the commit history. If A commits to a branch and B builds on top, reverting A's commits naturally cascades through git (B's commits depend on A's).

**Ripcord cascading (v1):** When the user pulls Agent A's ripcord, **all downstream work across agents that traces back to A's tripwire point is also reverted.** This is full cascade — simple, predictable, correct for the common case. The orchestrator tracks which agent outputs fed which downstream tasks.

**Why full cascade is acceptable in v1:** A user who pulls A's ripcord almost certainly wants B's downstream work undone too, because B's work is built on a foundation that just got removed. The rare case where B also did independent work unrelated to A is a v2 optimization (selective cascade with dependency tracking).

**Shared mutable state (deferred):** If agents interact through a database, running service, or shared filesystem state outside git, dependency tracking becomes genuinely complex. This is a v2 problem. Phase 1 assumes git is the primary shared state mechanism.

---

## Comparison: Fawx vs NemoClaw vs Status Quo

| Capability | Consent Prompts (status quo) | NemoClaw | Fawx AX Model |
|-----------|-----|---------|------|
| OS-level enforcement | No | Yes (OpenShell) | Yes (Landlock/seccomp, Phase 1) |
| App-level semantic boundaries | No | No | Yes |
| Agent runs uninterrupted | No | Yes | Yes |
| Graduated response | No | No | Yes (tripwire/ripcord) |
| Reversibility | No | No | Yes |
| Async user review | No | Partial (approval TUI) | Yes |
| Boundary evolution | No | No | Future (signal flywheel) |
| AX impact | Severe | None | Minimal (Layer 3 only) |
| Framework lock-in | N/A | OpenClaw only (OpenShell is generic) | Fawx only (could integrate OpenShell) |

**Where NemoClaw is genuinely stronger:**
- Ships today with proven OS-level enforcement
- Leverages NVIDIA's cloud inference routing (transparent to agent)
- Backed by NVIDIA's engineering resources and enterprise relationships

**Where Fawx is genuinely stronger:**
- Graduated response instead of binary allow/block
- Reversibility for actions within bounds
- Semantic understanding of agent behavior
- Signal generation for future boundary evolution
- OS enforcement + semantic layer = defense in depth that NemoClaw doesn't have

**Where we need to be honest:**
- Our OS enforcement is Phase 1 work; NemoClaw's ships today
- The ripcord's value diminishes for API-heavy workflows
- Earned autonomy is speculative until we have real signal data
- This requires significant new engineering before it's real

**Strategic positioning:** Fawx + OpenShell is potentially stronger than NemoClaw + OpenClaw. Same OS-level foundation, but Fawx adds semantic intelligence on top. NemoClaw wraps OpenClaw in a sandbox. Fawx integrates with the sandbox and adds graduated monitoring, reversibility, and learning. The competitive advantage is the layers above the sandbox, not the sandbox itself.

---

## Implementation Plan

### Phase 1: Foundation + OS Enforcement (ships first)
- **OS-level sandbox:** Self-sandboxing via Landlock (filesystem) + seccomp-bpf (syscalls) + network namespace (egress). Capability config maps to kernel rules at session start.
- **CapabilityGateExecutor:** Refactor from PermissionGateExecutor. Evaluate capabilities at session start, not per-action. Silent block with structured errors for out-of-bounds actions. Legacy per-action consent prompts deprecated.
- **`request_capability` tool:** Agent-initiated capability escalation. Agent decides to ask for a capability, provides reason. User approves/denies asynchronously. Replaces system-initiated prompt mode.
- **Session-scoped overrides:** CLI flag + API parameter for per-session capability grants.
- **3 presets:** Open, Standard, Restricted.
  - Open: all local capabilities, seccomp still blocks privilege escalation
  - Standard: all local capabilities, tripwire definitions for credential access and system modification
  - Restricted: explicit allowlist only, tightest OS sandbox

### Phase 2: Tripwire + Ripcord
- Tripwire definitions in config (same action taxonomy as capabilities)
- Journal infrastructure for file/git state snapshots (extend fx-journal)
- Ripcord mechanism: atomic rollback of file + git operations from tripwire point
- Shell/network/API actions: audit log only, no reversal attempted
- Async notification to user on tripwire crossing
- Review + ripcord UI in TUI and Swift app
- Ripcord TTL: configurable, default end-of-session, hard cap 24 hours
- Fleet cascade: full cascade through orchestrator dependency tracking

### Phase 3: Signal Collection
- Label every security event (tripwire cross, ripcord pull, boundary hit, capability negotiation)
- Local storage with fx-telemetry consent framework (distinct consent category)
- Dashboard: boundary effectiveness metrics, tripwire frequency, ripcord usage
- Preset adjustment suggestions based on actual usage patterns

### Phase 4: Earned Autonomy
- Automatic tripwire retirement based on signal history
- Automatic boundary tightening based on ripcord patterns
- User approval required for boundary changes (human in the loop on policy evolution, not on individual actions)

### Deferred
- **Phantom execution** — revisit if evidence shows irreversible-action friction is a real problem
- **Natural language capability parsing** — revisit when confidence is high enough to trust
- **Selective cascade** — v2 optimization for multi-agent ripcord with dependency tracking
- **FawxShell** — dedicated sandbox runtime, evaluate after Phase 1 validates the approach
- **Shared mutable state tracking** — v2, when fleet scenarios use databases/services beyond git

### Deprecated
- **`CapabilityMode::Prompt` (legacy prompt mode)** — system-initiated per-action consent prompts. Retained for backward compatibility but replaced by `request_capability` tool for forward-looking deployments. Will be removed in a future version.

---

## Terminology

- **AX (Agent Experience):** The quality of the agent's operating environment as it affects the agent's ability to plan, execute, and complete tasks effectively. Coined by Joe, 2026-03-17. Analogous to UX for humans.
- **Capability Space:** The set of actions available to an agent in a given session. Maps to both app-level checks and OS-level enforcement.
- **Tripwire:** A boundary within the capability space that, when crossed, silently activates journaled monitoring. Invisible to agent.
- **Ripcord:** The mechanism for atomically reversing all journaled file/git actions since a tripwire was crossed.
- **`request_capability` tool:** Agent-initiated capability escalation. The agent calls this tool with a reason when it needs a capability it doesn't have. Replaces system-initiated prompt mode.
- **Phantom Execution:** (Deferred) Intercepting an irreversible action and returning a fake success.
- **Signal Flywheel:** The feedback loop where human review decisions improve future boundary placement.
- **Earned Autonomy:** (Future) Automatic boundary adjustment based on accumulated trust signals.
- **FawxShell:** (Future) Fawx's own sandbox runtime, analogous to NVIDIA's OpenShell.
