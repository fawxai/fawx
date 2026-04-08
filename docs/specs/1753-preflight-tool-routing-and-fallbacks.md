# Spec: Preflight Tool Routing and Context-Aware Fallbacks

Date: 2026-04-07
Status: Implementation-ready
Issue: #1753

## Problem

The current harness can see available tools, skill descriptions, and some runtime
context, but it still makes first-move routing decisions too late and too loosely.

For external-resource tasks such as reviewing a GitHub PR, the loop can:

1. choose the wrong tool first
2. fan out into expensive parallel reads before doing cheap reconnaissance
3. stop after the first tool failure even when another tool is a much better fit

This is a control-plane bug, not a prompt-quality bug.

The system already has enough structure to support better behavior:

- skill manifests already carry `intent_hints`
- the loop already supports deterministic execution profiles
- runtime introspection already exposes loaded skills and tools

But those primitives stop at generic capability visibility. They do not yet encode:

- which tool is best for which external resource
- whether that tool is currently ready for a given resource class
- which failures should trigger an automatic reroute

## Concrete Failure

Observed in issue `#1753`:

- a GitHub PR review request started with `web_fetch`
- the target was better served by an authenticated GitHub-specific surface
- a GitHub visibility failure became a user-visible dead end instead of a same-turn pivot
- large diff retrieval skipped the cheap "how big is this?" probe and went straight to
  broad fan-out

## Expected Behavior

For a task like "review this GitHub PR":

1. classify the target resource before broad planning begins
2. check which GitHub-capable tools are present and ready
3. prefer the strongest authenticated GitHub surface available
4. probe size and structure before fetching large artifacts
5. if the first route fails in a way that implies another route should work,
   pivot within the same turn
6. only surface failure after the route chain is exhausted

## Architecture Decisions

### 1. Routing metadata must be typed control-plane state

Do not bury routing policy in free-form descriptions or `parameters["x-fawx-*"]`.

The loop needs typed metadata it can evaluate before tool execution. Tool descriptions
are for model selection; routing metadata is for kernel policy.

### 2. Failure recovery requires structured result envelopes

The current string-only WASM HTTP surface hides important distinctions such as:

- HTTP 404 vs 401 vs 403
- transport failure vs application failure
- visibility miss vs bad input

Same-turn rerouting depends on preserving those distinctions as typed diagnostics.

These two decisions are complementary:

- typed routing metadata improves the initial choice
- structured failures enable automatic same-turn correction

## Solution Overview

Add a preflight route planner for external-resource tasks and pair it with typed
fallback signals.

The kernel should:

1. detect resource-bearing tasks early
2. assemble a ranked route plan from typed tool metadata + runtime readiness
3. constrain the first execution path to the selected route
4. use probe-first retrieval for large artifacts
5. consume typed failure diagnostics to pivot when appropriate

## Proposed Data Model

### ToolRoutingMetadata

Attach to each tool outside the JSON schema used for LLM tool selection.

Suggested shape:

```rust
pub struct ToolRoutingMetadata {
    pub resource_kinds: Vec<ResourceKind>,
    pub operations: Vec<RouteOperation>,
    pub auth_mode: RouteAuthMode,
    pub artifact_strategy: ArtifactStrategy,
    pub fallback_rank: u16,
}
```

Examples:

- GitHub PR viewer: `resource_kinds = [GitHubPullRequest]`
- Browser fetcher: `resource_kinds = [GenericUrl]`
- GitHub file-list probe: `artifact_strategy = ProbeFirst`

### ToolReadinessSummary

Expose runtime readiness separately from static metadata.

Suggested shape:

```rust
pub struct ToolReadinessSummary {
    pub tool_name: String,
    pub available: bool,
    pub ready: bool,
    pub readiness_reason: Option<String>,
}
```

This should answer questions like:

- is the GitHub skill loaded?
- is required auth present?
- is the tool visible but not usable?

It must not expose secrets.

### RoutePlan

Built per turn for resource-bearing requests.

Suggested shape:

```rust
pub struct RoutePlan {
    pub resource: RouteResource,
    pub primary_route: PlannedRoute,
    pub fallback_routes: Vec<PlannedRoute>,
    pub requires_probe: bool,
}
```

`PlannedRoute` should include:

- tool names allowed for the route
- why the route was chosen
- whether the route is authenticated
- whether the route is probe-only or retrieval-capable

## Resource Policy

For GitHub resources, the policy should be:

1. prefer the strongest authenticated GitHub-capable route available
2. prefer probe-capable routes before full artifact retrieval
3. treat generic web fetch as a fallback path, not the default path

This policy must be expressed generically as "strongest ready route for resource X",
not as a one-off hardcoded "`gh` first" rule.

## Probe-First Contract

Large artifacts must use a reconnaissance step before full retrieval.

Examples:

- PR review: fetch changed files and patch stats before full patch bodies
- document review: fetch metadata and size before content chunking
- logs: inspect size/window before broad reads

Probe outputs should be typed so later retrieval can chunk by logical unit:

- file
- file group
- section
- page

Not arbitrary shell slicing.

## Failure and Fallback Contract

Tool failures that can inform rerouting must preserve structured cause.

Suggested failure classes:

- `auth_required`
- `visibility_mismatch`
- `not_found`
- `unsupported_resource`
- `transient_transport`
- `rate_limited`
- `invalid_request`

Structured HTTP responses from the WASM host should include at least:

- status code
- headers when safe
- body text or binary marker

The loop should map those to reroute decisions such as:

- `visibility_mismatch` on a GitHub URL from a generic fetch path
  -> try authenticated GitHub route
- `auth_required`
  -> try another authenticated route if available, else fail clearly
- `not_found`
  -> do not retry equivalent routes indefinitely

## Phases

### Phase 0: Regression Coverage

Add tests that fail on current behavior.

Acceptance:

1. GitHub PR URL plus ready GitHub route does not start with `web_fetch`
2. GitHub visibility/auth miss reroutes within the same turn
3. large PR review starts with a probe path

### Phase 1: Typed Routing Metadata and Readiness

Add typed routing metadata and runtime readiness summaries.

Acceptance:

1. runtime state includes tool routing metadata and readiness
2. `self_info` and `kernel_manifest` can show route-relevant readiness
3. no routing-critical state depends on parsing human descriptions

### Phase 2: Preflight Route Planner

Add a kernel-owned preflight routing step for external resources.

Acceptance:

1. resource-bearing tasks produce a `RoutePlan`
2. the initial tool surface is constrained to the selected route
3. route choice is visible in traces/signals

### Phase 3: GitHub Probe-First Vertical Slice

Refactor GitHub retrieval so PR review can probe before full diff retrieval.

Acceptance:

1. PR metadata and changed-file inventory can be fetched without full patch bodies
2. full diff fetch is opt-in or file-scoped
3. review flow chunks by file or file group instead of overlapping raw offsets

### Phase 4: Structured Failures and Same-Turn Pivot

Upgrade the WASM HTTP path and loop recovery to preserve typed failures.

Acceptance:

1. HTTP status and typed failure cause survive tool execution
2. the loop can pivot routes without asking the user to intervene
3. user-visible failure happens only after route exhaustion

### Phase 5: Advisory Memory Injection

After the control plane is correct, use memory to improve route ranking.

Acceptance:

1. routing works correctly with empty memory
2. memory only improves ranking and context, not correctness

## PR Slices

### PR 1

- Phase 0 tests
- Phase 1 typed routing metadata
- Phase 1 readiness surfaced in runtime state

### PR 2

- Phase 3 GitHub probe-first tool split

### PR 3

- Phase 2 preflight route planner

### PR 4

- Phase 4 structured HTTP/tool failure envelopes
- same-turn reroute integration

### PR 5

- Phase 5 advisory memory injection
- generalization beyond GitHub

## Likely Files

- [engine/crates/fx-skills/src/manifest.rs](/Users/joseph/fawx/engine/crates/fx-skills/src/manifest.rs)
- [engine/crates/fx-loadable/src/skill.rs](/Users/joseph/fawx/engine/crates/fx-loadable/src/skill.rs)
- [engine/crates/fx-loadable/src/registry.rs](/Users/joseph/fawx/engine/crates/fx-loadable/src/registry.rs)
- [engine/crates/fx-loadable/src/lifecycle.rs](/Users/joseph/fawx/engine/crates/fx-loadable/src/lifecycle.rs)
- [engine/crates/fx-core/src/runtime_info.rs](/Users/joseph/fawx/engine/crates/fx-core/src/runtime_info.rs)
- [engine/crates/fx-core/src/kernel_manifest.rs](/Users/joseph/fawx/engine/crates/fx-core/src/kernel_manifest.rs)
- [engine/crates/fx-cli/src/startup.rs](/Users/joseph/fawx/engine/crates/fx-cli/src/startup.rs)
- [engine/crates/fx-kernel/src/loop_engine/mod.rs](/Users/joseph/fawx/engine/crates/fx-kernel/src/loop_engine/mod.rs)
- [engine/crates/fx-kernel/src/loop_engine/request.rs](/Users/joseph/fawx/engine/crates/fx-kernel/src/loop_engine/request.rs)
- [engine/crates/fx-kernel/src/loop_engine/bounded_local.rs](/Users/joseph/fawx/engine/crates/fx-kernel/src/loop_engine/bounded_local.rs)
- [engine/crates/fx-kernel/src/act.rs](/Users/joseph/fawx/engine/crates/fx-kernel/src/act.rs)
- [engine/crates/fx-skills/src/host_api.rs](/Users/joseph/fawx/engine/crates/fx-skills/src/host_api.rs)
- [engine/crates/fx-skills/src/live_host_api.rs](/Users/joseph/fawx/engine/crates/fx-skills/src/live_host_api.rs)
- [engine/crates/fx-skills/src/runtime.rs](/Users/joseph/fawx/engine/crates/fx-skills/src/runtime.rs)
- [skills/github-skill/src/lib.rs](/Users/joseph/fawx/skills/github-skill/src/lib.rs)
- [skills/github-skill/manifest.toml](/Users/joseph/fawx/skills/github-skill/manifest.toml)
- [skills/browser-skill/src/lib.rs](/Users/joseph/fawx/skills/browser-skill/src/lib.rs)

## Non-goals

- fixing this with more system-prompt prose alone
- encoding routing policy in human-readable tool descriptions
- hardcoding a GitHub-only exception path in `web_fetch`
- making correctness depend on journal recall
- preserving the current string-only HTTP surface if it blocks typed rerouting

## Why This Matters

This issue is the same class of bug as other deterministic-control-plane failures in
this repo: the system already knows enough to behave correctly, but it lacks the typed
surface that lets the kernel act on that knowledge early and consistently.

The durable fix is to make route selection and reroute conditions explicit, typed, and
testable.
