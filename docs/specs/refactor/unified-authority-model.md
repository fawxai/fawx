# Spec: Unified Authority Model

## Status
Not started. Needs design validation before implementation.

## Goal
Establish one canonical authority chain for every agent action. Today, five systems can independently allow, deny, or gate the same action, and they can disagree. The result is that neither the loop, the user, nor the developer can reliably predict what will happen.

## The Problem

Five overlapping control planes govern agent actions:

### 1. CapabilityMode (fx-config/types.rs)
Two modes: `Capability` (silent structured denial) and `Prompt` (interactive negotiation). Determines the *style* of enforcement, not the policy itself. But it changes observable behavior: the same action may silently fail in one mode and succeed after user approval in another.

### 2. PermissionPolicy / PermissionGateExecutor (fx-kernel/permission_gate.rs)
A `PermissionPolicy` struct with boolean flags per tool category: `file_write`, `file_delete`, `shell_execute`, `web_access`, `self_modify`, `kernel_modify`, `outside_workspace`. `PermissionGateExecutor` wraps a `ToolExecutor` and intercepts calls. 927 lines. Presets: `Power` (allow most), `Cautious` (restrict mutations).

### 3. ProposalGateExecutor (fx-kernel/proposal_gate.rs)
Wraps the permission-gated executor. Intercepts writes to sovereign/kernel paths and converts them to proposals. 1,578 lines. Has its own path classification (`is_tier3_path`, `is_kernel_blind_path`) that partially overlaps with self_modify.rs.

### 4. WriteDomain / PathTier (fx-core/self_modify.rs)
Classifies filesystem paths into domains: `Project`, `SelfLoadable`, `KernelSource`, `Sovereign`, `External`. Each domain maps to a permission category. Also provides `PathTier` (Allow/Propose/Deny) based on glob patterns. 756 lines. This is the most principled classification, but it's not the sole authority; the permission gate and proposal gate apply their own logic on top.

### 5. Ripcord / Tripwire (fx-ripcord/, fx-canary/)
Runtime behavioral constraints. Ripcord evaluates action journals against thresholds and can halt the agent. Tripwire monitors for anomalous patterns. These operate on completed actions, not pre-execution gating. They're the safety net, not the policy authority.

### How they conflict

Example: agent writes to `engine/crates/fx-kernel/src/lib.rs`

1. **self_modify.rs** classifies it as `WriteDomain::KernelSource` → permission category `kernel_modify`
2. **PermissionGateExecutor** checks `policy.kernel_modify` boolean. If `false`, denies. If `true`, allows.
3. **ProposalGateExecutor** has its own `is_tier3_path` / `is_kernel_blind_path` check that may independently decide this needs a proposal, regardless of what the permission gate said.
4. **CapabilityMode** determines whether a denial is silent or interactive, but the user doesn't know which system denied it.
5. **Ripcord** may independently halt the agent after the write completes if cumulative behavior crosses a threshold.

The result: a developer reading the permission policy sees `kernel_modify: true` and expects the write to work. But the proposal gate intercepts it anyway. Or the proposal gate allows it but the permission gate denies it. The user sees a denial but doesn't know which system caused it or how to override it.

## Design Principles

### 1. One path, one answer
For any (action, target) pair, there must be exactly one authority that produces the final allow/deny/propose verdict. No two systems may independently decide the same action.

### 2. Classification is separate from enforcement
Path/surface classification (what domain does this target?) is a pure function. Enforcement (should this action proceed?) reads the classification and applies policy. These are different concerns and must not be interleaved.

### 3. The user can always understand why
Every denial or proposal must reference the specific policy rule, the surface classification, and the authority that made the decision. No opaque "permission denied" without attribution.

### 4. Ripcord is a safety net, not a policy authority
Ripcord and tripwire operate on completed or in-progress action streams. They are the last-resort circuit breaker. They should never be the primary mechanism for routine access control.

## Proposed Architecture

### Surface Classification (pure, stateless)
```
ActionSurface {
    domain: SurfaceDomain,      // Project | Loadable | Kernel | Sovereign | External
    path: Option<PathBuf>,      // for filesystem actions
    tool: String,               // tool name
    effect: ActionEffect,       // Read | Write | Delete | Execute | Network
}
```

Every action is classified into an `ActionSurface` before any enforcement. This replaces the scattered classification in `self_modify.rs`, `proposal_gate.rs`, and `permission_gate.rs`.

The `WriteDomain` enum in `self_modify.rs` is the closest to this today. Promote it to the canonical classification, remove competing classifiers.

### Policy Resolution (single authority per surface)
```
SurfacePolicy {
    verdict: ActionVerdict,     // Allow | Propose | Deny
    authority: &'static str,    // which policy rule
    reason: String,             // human-readable
}

enum ActionVerdict {
    Allow,
    Propose { reviewer: ProposalTarget },
    Deny { style: DenialStyle },
}

enum DenialStyle {
    Silent,         // CapabilityMode::Capability
    Interactive,    // CapabilityMode::Prompt
}
```

For each `ActionSurface`, exactly one policy rule applies. The `CapabilityMode` affects the denial *style* (silent vs interactive), not the verdict itself.

### Enforcement Stack (ordered, non-overlapping)
```
Action
  → classify(action) → ActionSurface
  → resolve_policy(surface, config) → SurfacePolicy
  → enforce(policy)
      → Allow: execute
      → Propose: create proposal, notify user
      → Deny(Silent): return structured error
      → Deny(Interactive): prompt user, then re-evaluate
  → [post-execution] ripcord/tripwire journal
```

Key: `resolve_policy` is a single function, not a chain of wrapping executors. The current architecture has `PermissionGateExecutor<ProposalGateExecutor<FawxToolExecutor>>` where each wrapper applies its own logic. The new model collapses this into one resolution step.

### What changes

| Current | New |
|---|---|
| `PermissionGateExecutor` wraps executor | Removed. Policy resolution handles permissions. |
| `ProposalGateExecutor` wraps executor | Removed. Proposal is a verdict type, not a separate gate. |
| `CapabilityMode` changes behavior | Affects denial presentation only, not verdict. |
| `self_modify.rs` classifies paths | Promoted to canonical `ActionSurface` classifier. |
| `is_tier3_path` in proposal_gate.rs | Deleted. Redundant with WriteDomain classification. |
| Ripcord evaluates post-hoc | Unchanged. Correct role already. |

### What stays the same

- `WriteDomain` enum and path pattern lists (expanded, not replaced)
- Ripcord/tripwire architecture (post-execution safety net)
- Config surface: `PermissionPreset` still determines the policy profile
- Tool execution: `FawxToolExecutor` (or post-#1639 registry) executes tools after policy resolution

## Deliverables

### Phase 1: Surface classification consolidation
1. Promote `WriteDomain` + `PathTier` from `fx-core/self_modify.rs` to `ActionSurface` with full classification for all action types (not just filesystem)
2. Add `ActionEffect` enum (Read/Write/Delete/Execute/Network)
3. Remove duplicate classifiers from `proposal_gate.rs` (`is_tier3_path`, `is_kernel_blind_path`)
4. Every tool call classified through one path

### Phase 2: Policy resolution
1. `SurfacePolicy` struct with verdict, authority, reason
2. Single `resolve_policy(surface, config)` function
3. `CapabilityMode` moves into `DenialStyle`, no longer affects verdict logic
4. Remove `PermissionGateExecutor` wrapper
5. Remove `ProposalGateExecutor` wrapper
6. Policy resolution called before tool execution, not via wrapping

### Phase 3: User-facing attribution
1. Every denial/proposal includes: surface classification, policy rule name, human reason
2. Structured error format for denials (already partially done in CapabilityMode::Capability)
3. `/permissions` command shows the resolved policy for each surface domain

## Files to modify
- `engine/crates/fx-core/src/self_modify.rs` — expand to full ActionSurface
- `engine/crates/fx-kernel/src/permission_gate.rs` — remove (collapse into policy resolution)
- `engine/crates/fx-kernel/src/proposal_gate.rs` — remove (collapse into policy resolution)
- `engine/crates/fx-config/src/types.rs` — CapabilityMode becomes DenialStyle on verdict
- `engine/crates/fx-tools/src/tools.rs` (or post-#1639 tool registry) — classify before execute
- New: `engine/crates/fx-core/src/authority.rs` — ActionSurface, SurfacePolicy, resolve_policy

## Depends on
- #1639 (Tool trait) — tools self-declare their surface domain and effect type via trait methods. Without this, classification still requires string matching on tool names.

## Not in scope
- Changes to ripcord/tripwire (correct architecture already)
- Changes to WASM skill sandboxing (separate concern, uses OS-level enforcement)
- UI/TUI changes for permission prompts (separate follow-up)
- Multi-user permission models (single-user agent for now)
