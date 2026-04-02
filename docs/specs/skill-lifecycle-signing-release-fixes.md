# Spec: Skill Lifecycle and Signing Release Fixes

## Status
Ready to implement.

## Goal
Make the WASM skill lifecycle coherent and release-safe before the next DMG.

This spec covers the pre-release fixes exposed by the `x-api-skill` debugging session:
- broken or misleading signing commands
- conflicting build/install guidance across TUI, CLI, and docs
- unclear distinction between built skills, installed skills, active server-loaded skills, and Swift-visible skills
- inconsistent implied build targets and local-dev workflows

## Why this matters now
The current build feels stable enough to promote and release, but the skill workflow is still confusing in exactly the wrong places:
- the TUI advertises `/sign` even though it does not actually perform signing
- the TUI points users to `fawx sign <skill>`, but the CLI does not expose that command as a real first-class surface
- the repo skill build script, the CLI skill build path, and the docs do not present one canonical workflow
- `/skills` in the TUI mixes local built/install discovery, while the Swift app only shows skills loaded on the server

That means users can successfully build a skill and still not understand how it becomes usable.

## Source-of-truth findings from the code

### F1. TUI `/sign` is misleading
The TUI advertises `/sign <skill>` and `/sign --all`, but the headless command path only tells the user to run `fawx sign <skill>`.

### F2. CLI signing surface is missing or stale
The current CLI command surface exposes `fawx skill list/search/install/remove/build/status/rollback`, but not a real top-level `fawx sign` flow that matches the TUI guidance.

### F3. Built vs installed vs loaded are distinct states
The product currently has multiple different skill states:
- source project in the repo
- built WASM artifact in the repo tree
- installed skill in `~/.fawx/skills/`
- staged and activated revision in lifecycle state
- active server-loaded skill exposed through `/v1/skills`
- skill visible in the Swift app

Those are all real distinctions in code, but the product surfaces do not explain them clearly.

### F4. TUI and Swift show different things
The TUI `/skills` flow discovers local built and installed skills from the filesystem. The Swift app shows skills loaded on the running server via `/v1/skills`. Those are not the same thing.

### F5. Build/install guidance is fragmented
There are at least three competing implied workflows in the tree:
- `skills/build.sh --install`
- `fawx skill build <project>`
- `fawx skill install <path>`

The release needs one canonical story, with the others framed clearly as specialized paths.

## Product decisions for this spec

1. **Restore a real signing workflow.**
   The TUI and CLI must point to a real, working signing path.

2. **Choose one canonical local-dev workflow.**
   For building a local custom skill, users should have one recommended path that compiles, installs, and explains the next step.

3. **Name the lifecycle states explicitly.**
   Product surfaces and docs should distinguish:
   - built locally
   - installed locally
   - loaded on server

4. **Make TUI `/skills` wording honest.**
   If it shows local built or installed artifacts, it must not imply that those skills are necessarily loaded by the running server.

5. **Keep Swift scoped to server-loaded skills.**
   The Swift app does not need a full local artifact import UI in this release fix. It just needs wording and behavior that match the actual server-loaded contract.

## Non-goals
- no new Swift local-skill import UI
- no full marketplace redesign
- no new signing trust model
- no bulk skill lifecycle UX
- no release-blocking redesign of the skill runtime itself

## Proposed implementation slices
This spec is broken into PR-sized slices in `docs/specs/skill-lifecycle/`.

Recommended order:
1. `step1-sign-command-surface.md`
2. `step2-skill-state-semantics.md`
3. `step3-docs-and-help-alignment.md`
4. `step4-end-to-end-lifecycle-verification.md`

## Release exit criteria
Before release, a tester should be able to:
1. create or modify a local skill project
2. use the documented canonical command path to build/install it
3. use a real signing command if signing is required or recommended
4. understand whether the skill is merely built, installed, or actually loaded on the server
5. see the skill in the correct product surface once it is server-loaded
6. avoid being told to use commands that do not exist

## Reviewer focus
- Does the final command/help surface form one coherent skill lifecycle story?
- Are TUI and Swift semantics accurate rather than aspirational?
- Is the canonical workflow obvious enough that a user can build a local skill without reading engine code?
