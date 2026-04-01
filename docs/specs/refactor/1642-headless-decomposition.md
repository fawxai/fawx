# Spec: #1642 — Headless Decomposition After Detached-Lane Stabilization

## Status
Ready to implement.

This spec supersedes the earlier draft that treated `headless.rs` as a pure file-splitting exercise.

## Why this spec changed

Step 10, Step 11, and PR #1673 changed the landscape enough that the original `headless.rs -> command handlers` proposal is no longer sufficient on its own.

What is true now:

- Unified authority resolution landed in `#1668`
- Static dispatch cleanup landed in `#1669`, `#1670` / `#1671`, and `#1672`
- Detached-lane stabilization landed in `#1673`
  - replay-safe tool history
  - structured continuation evidence
  - detached workspace-root rebinding
  - detached verification workspace hardening

That means Step 12 is no longer "split a big file because it is big." Step 12 is now: **decompose headless responsibilities into explicit modules with stable contracts, while preserving the freshly-repaired detached-lane runtime behavior.**

The decomposition must make future changes safer. It must not reintroduce the exact coupling that just made detached verification unreliable.

## Goal

Replace the `fx-cli` headless monolith with a small composition root plus focused modules that each own one responsibility:

1. startup and lane-root binding
2. interactive session input routing
3. message execution against the loop engine
4. command parsing and command dispatch
5. command-domain implementations
6. output rendering / printing helpers
7. detached-lane and clean-bisect integration tests

The result should preserve all current behavior while making headless startup, command handling, and loop execution independently testable.

## Non-goals

- No product-surface changes
- No new slash commands
- No redesign of auth or config UX
- No trait-heavy framework for its own sake
- No string-dispatch tables that move the monolith into a different file
- No behavior regressions in detached headless lanes

## Architectural thesis

`headless.rs` currently mixes too many layers:

- runtime construction
- config binding
- loop execution
- session state
- command parsing
- command execution
- output rendering
- auth / keys / model / synthesis subdomains

That violates both doctrine and testability.

The fix is **layer decomposition**, not merely line-count reduction.

Each subsystem should own its own behavior behind typed entry points. The composition root wires them together. Command domains should be grouped by behavior, not by arbitrary chunks of the old file.

## Doctrine constraints

The implementation must obey `docs/doctrine.md`:

- Components declare their own behavior; systems discover them
- Avoid name-matching dispatch tables where behavior belongs on the component
- Do not replace one giant match with several smaller giant matches spread across files unless the behavior genuinely belongs there

This does **not** require a speculative dynamic plugin system for headless commands. YAGNI still applies. A typed command enum plus focused executors is acceptable if responsibility boundaries are clean.

## Current problems to solve

### P1. Startup and runtime binding are too entangled

PR `#1673` proved that headless startup order matters. Workspace-root rebinding has to happen before downstream runtime construction. That logic should not live buried inside a monolithic file where unrelated command work can accidentally disturb it.

### P2. Session routing and message execution are coupled

The interactive headless session does too much:

- parse raw input
- decide whether it is a command or plain message
- dispatch slash command families
- invoke loop execution
- render user-facing output

These should be separated so message-path regressions and command-path regressions can be tested independently.

### P3. Command domains are not isolated

Auth, keys, synthesis, model/thinking, setup/signing, and other command families currently live in the same file and share too much ambient state. This makes review difficult and encourages broad edits for small changes.

### P4. End-to-end detached verification must stay first-class

The clean bisect lane is now part of the architecture contract for this area. Step 12 is not complete if the code is prettier but the detached harness is no longer trustworthy.

## Proposed target structure

Create a `headless/` module directory under `engine/crates/fx-cli/src/`.

```text
engine/crates/fx-cli/src/headless/
├── mod.rs
├── startup.rs
├── engine.rs
├── session.rs
├── command.rs
├── output.rs
├── auth.rs
├── keys.rs
├── model.rs
└── tests/
    ├── mod.rs
    ├── startup_binding.rs
    ├── command_routing.rs
    ├── continuation_integrity.rs
    └── detached_lane.rs
```

### Module responsibilities

#### `mod.rs`
Small composition root only.

Owns:
- module exports
- `HeadlessEngine` struct definition
- `HeadlessSession` struct definition
- shared small data types that are truly cross-cutting

Must not become a new dumping ground.

#### `startup.rs`
Owns headless startup and bundle construction.

Responsibilities:
- load config
- bind workspace root for headless startup
- create runtime dependencies in the correct order
- construct the loop engine bundle / session dependencies
- expose narrow builder helpers

Critical rule:
- The `#1673` startup ordering fix is locked here with direct regression tests.

#### `engine.rs`
Owns message execution through the loop engine.

Responsibilities:
- `process_message*` variants
- attachments / images / context entry points
- streaming and non-streaming execution paths
- conversion between headless request forms and kernel invocation

Must not parse slash commands.

#### `session.rs`
Owns interactive headless session state and top-level input routing.

Responsibilities:
- accept raw input text
- determine whether input is a command or a normal message
- dispatch parsed commands to command executors
- dispatch plain messages to `engine.rs`

Must not contain domain-specific auth / key / model logic.

#### `command.rs`
Owns command parsing and typed command routing.

Responsibilities:
- parse slash-command input into a typed enum
- central routing from typed command to domain executor
- shared command result types

Recommended shape:

```rust
pub enum HeadlessCommand {
    Auth(AuthCommand),
    Keys(KeysCommand),
    Thinking(ThinkingCommand),
    Synthesis(SynthesisCommand),
    Sign(SignCommand),
    // ...only commands that already exist today
}
```

This is preferred over free-form string dispatch because it gives compile-time boundaries without inventing a plugin framework.

#### `output.rs`
Owns user-facing output shaping for headless mode.

Responsibilities:
- printing / formatting helpers
- command success / error rendering
- shared headless textual output utilities

Keep this thin. It is formatting support, not business logic.

#### `auth.rs`, `keys.rs`, `model.rs`
Own their domain command implementations.

Responsibilities:
- parse domain-specific arguments after top-level command selection, or consume already-parsed typed variants
- perform the domain action
- return typed result objects or rendered output through shared helpers

These modules should hold the logic that today lives in large free functions like `handle_headless_auth_command()`.

## Design rules

### R1. Split by responsibility, not by line count

Do not scatter one flow across four files just to hit smaller file sizes.

### R2. Preserve the public surface

Existing imports and user behavior should remain stable unless a specific cleanup is required for internal structure.

### R3. Typed command routing over string soup

Top-level command routing should use a typed parse result.

Acceptable:
- one small parser that turns raw input into a typed command enum
- domain modules that operate on typed structs / enums

Not acceptable:
- multiple giant string matches in multiple files
- moving the existing monolith unchanged into `auth.rs` and `model.rs`

### R4. Startup ordering is a protected contract

`bind_headless_workspace_root` and related startup sequencing from `#1673` must remain early and explicit.

### R5. Detached clean-lane verification is part of done

No approval if detached-lane end-to-end verification is missing.

## Implementation plan

Implement this in small, reviewable phases.

### Phase 1. Convert file to module directory with no behavior change

Goal:
- turn `headless.rs` into `headless/mod.rs`
- move only the minimum code required to keep the build green

Deliverables:
- module directory exists
- `mod.rs` compiles
- no behavior changes

Regression coverage:
- existing tests stay green

### Phase 2. Extract startup into `startup.rs`

Goal:
- isolate all startup / bundle construction / workspace-root binding logic

Deliverables:
- `build_headless_startup` and adjacent helpers move into `startup.rs`
- `bind_headless_workspace_root` sequencing is explicit and easy to audit
- startup regression tests live next to the extracted code

Required regression tests:
- detected repo root overrides ambient main-checkout working dir
- read/write/git helpers bind to detached repo root after startup construction
- failure fallback still uses configured working dir safely when repo-root detection fails

### Phase 3. Extract loop execution into `engine.rs`

Goal:
- isolate message execution paths from command routing

Deliverables:
- `process_message*` family moves into `engine.rs`
- streaming and non-streaming paths remain behaviorally identical
- attachments/context/image entry points stay covered

Required regression tests:
- plain message path still invokes loop execution correctly
- streaming path still emits expected headless behavior
- structured continuation evidence remains available across follow-up turns

### Phase 4. Add typed command parsing in `command.rs`

Goal:
- stop routing on raw slash-command strings deep inside business logic

Deliverables:
- a typed `HeadlessCommand` parse layer
- central command dispatch from `session.rs`
- domain-specific command enums as needed

Required regression tests:
- command parser recognizes current supported headless commands
- invalid commands still surface equivalent user-facing errors
- non-command input still routes to message execution

### Phase 5. Extract command domains

Goal:
- move auth / keys / model / synthesis / signing behavior into focused modules

Deliverables:
- `auth.rs`, `keys.rs`, `model.rs` own their command logic
- `session.rs` no longer contains domain command implementations
- `command.rs` no longer contains business logic beyond dispatch

Required regression tests:
- auth command behavior unchanged
- keys command behavior unchanged
- thinking/synthesis/model command behavior unchanged
- signing/setup behavior unchanged

### Phase 6. Add detached-lane end-to-end integration harness coverage

Goal:
- prove the decomposition did not break clean detached verification

Deliverables:
- live clean-bisect lane harness documented in this spec
- automated or semi-automated script/test entrypoint for the PR triage lane
- evidence captured in the PR description or triage comment

## Required code-quality constraints

- No `.unwrap()` outside tests
- No function longer than ~40 lines without decomposition
- No >5 params without a struct
- No new dependencies unless justified
- No behavior encoded by component name matches where metadata or typed contracts should own it

## Test plan

This work requires both local regression coverage and live detached-lane verification.

### A. Local automated test coverage

Add or preserve tests for these behaviors:

1. startup root binding
2. command-vs-message routing
3. continuation evidence preservation
4. replay-safe unresolved tool history pruning
5. detached read/write/git behavior through built runtime
6. current headless command families still parse and execute

Suggested test files:

- `tests/startup_binding.rs`
- `tests/command_routing.rs`
- `tests/continuation_integrity.rs`
- `tests/detached_lane.rs`

### B. Required cargo validation

Before review:

```bash
cargo fmt --all
cargo clippy --workspace --tests -- -D warnings
cargo test --workspace
```

### C. Live clean bisect lane end-to-end harness

This is mandatory for the PR because this area directly affects headless runtime correctness.

Use `docs/runbooks/clean-bisect-lane.md` as the baseline process.

The PR must run the following headless clean-lane battery on a fresh detached worktree and fresh data dir.

## Live clean bisect lane harness

### Environment

Run on the Mac Mini or other clean macOS lane host where the standard clean-bisect runbook is supported.

Set:

```bash
COMMIT=<branch-or-commit-under-test>
LANE="bisect-$COMMIT"
PORT=8401

REPO=/Users/joseph/fawx
WORKTREE=/private/tmp/fawx-$LANE
TARGET_DIR=/Users/joseph/.cargo-targets/fawx-$LANE
DATA_DIR=/Users/joseph/.fawx-$LANE
```

Then follow Steps 1-5 from `docs/runbooks/clean-bisect-lane.md` to:

- remove prior lane state
- create the detached worktree
- build `fx-cli`
- seed a minimal fresh data dir
- start the detached headless server
- adopt a fresh local device token

### Test battery

Run these in order on a fresh lane.

#### T1. Detached read-only verification

Prompt:

```text
Read README.md and quote the first paragraph exactly.
```

Pass criteria:
- response quotes content from the detached worktree README
- main checkout remains unchanged
- detached worktree remains unchanged
- no outside-working-directory refusal
- no hallucinated text not present in detached README

Evidence to capture:

```bash
git -C "$WORKTREE" status --short
 git -C "$REPO" status --short
```

#### T2. Detached absolute/anchored repo read verification

Prompt:

```text
Read Cargo.toml and tell me the package or workspace name you see.
```

Pass criteria:
- response reflects the detached checkout’s file contents
- no fallback to ambient main checkout
- no refusal caused by stale working-dir binding

#### T3. Detached append / write verification

Prompt:

```text
Read README.md, then append this exact marker on a new last line: <!-- STEP12_LANE_MARKER -->
```

Pass criteria:
- only the detached worktree file changes
- marker is appended exactly once in the detached README
- main checkout README is untouched
- response reflects a successful multi-step read-then-write path

Evidence to capture:

```bash
tail -5 "$WORKTREE/README.md"
grep -n "STEP12_LANE_MARKER" "$WORKTREE/README.md"
git -C "$WORKTREE" status --short
git -C "$REPO" status --short
```

#### T4. Detached git-status verification

Prompt:

```text
Run git status and tell me which file is modified.
```

Pass criteria:
- response identifies only detached `README.md` as modified
- no mention of unrelated files from the main checkout
- confirms git helpers are bound to the detached repo

#### T5. Fresh second-lane cleanliness check

Create a second fresh lane using the same runbook with a distinct `LANE` and `PORT`, then run:

Prompt:

```text
Read README.md and summarize the first paragraph.
```

Pass criteria:
- second lane starts clean
- no marker inherited from the first lane unless it exists in the tested commit itself
- detached lane is independent
- main checkout remains untouched

### Continuation-integrity follow-up battery

Because `#1673` fixed continuation-evidence loss, Step 12 must prove the decomposition does not regress that behavior.

#### T6. Structured follow-up read reasoning

Prompt:

```text
Read README.md, then tell me the exact heading that appears immediately after the first paragraph.
```

Pass criteria:
- answer is based on actual tool-read evidence from the detached README
- no synthesized paraphrase causing a wrong heading
- no missing-tool-output / continuation-history errors

#### T7. Structured append follow-up reasoning

Prompt:

```text
Read README.md, append <!-- STEP12_EVIDENCE_MARKER --> on a new last line, then tell me exactly what line you appended.
```

Pass criteria:
- write happens only in detached lane
- assistant reports the exact appended line
- no continuation corruption or replay-order failure

### Failure classification

A failure is blocking if any of these occur:

- main checkout is read/written instead of detached worktree
- command path breaks for existing supported command families
- missing tool output / unknown tool / replay-order errors appear
- multi-step follow-up turns lose structured evidence
- lane 2 inherits mutable state from lane 1 incorrectly

## PR evidence requirements

The PR description or triage comment must include:

1. local validation results
   - `cargo fmt --all`
   - `cargo clippy --workspace --tests -- -D warnings`
   - `cargo test --workspace`
2. clean bisect lane evidence
   - lane identifiers
   - tested commit / branch
   - PASS/FAIL for T1-T7
   - short proof snippets for detached file mutation and main-checkout cleanliness
3. explicit statement that Step 12 preserved the detached-lane guarantees established in `#1673`

## Files expected to change

At minimum:

- `engine/crates/fx-cli/src/headless.rs` removed or converted into `headless/mod.rs`
- new `engine/crates/fx-cli/src/headless/*.rs` modules
- tests under `engine/crates/fx-cli/src/headless/tests/` and/or equivalent integration-test locations
- possibly small import / module declaration updates where `headless` is referenced

## Review checklist

Reviewer must confirm:

- decomposition follows behavior boundaries, not arbitrary chunks
- startup binding fix from `#1673` is still early and protected
- no new stringly dispatch tables were introduced
- command parsing is typed and localizable
- detached clean-lane battery was actually run and evidence is attached
- no behavior regressions in existing headless command families

## Done means

Step 12 is done only when:

1. `headless.rs` is decomposed into focused modules
2. startup / routing / execution boundaries are clearer and independently testable
3. all workspace tests pass
4. live clean-bisect lane battery T1-T7 passes
5. detached-lane correctness from `#1673` is preserved

If the file is smaller but detached verification is shakier, the work is not done.
