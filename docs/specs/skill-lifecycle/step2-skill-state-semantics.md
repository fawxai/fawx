# Step 2: Clarify Skill Lifecycle States in Product Surfaces

## Goal
Make the product explicitly distinguish between built, installed, and server-loaded skills.

## Why this slice exists
Right now the TUI `/skills` flow and the Swift Skills UI are describing different lifecycle stages using language that is too easy to confuse. The result is that a user can see a skill locally and assume it should already be usable in the server or app.

## Expected targets
- TUI `/skills` messaging and help text
- Swift Skills UI labels / empty-state wording only if needed
- any small shared wording helpers or summaries that present skill state

## Required outcome
The product should make these distinctions obvious:
- **Built locally**: artifact exists in the repo/build tree
- **Installed locally**: skill exists in `~/.fawx/skills`
- **Loaded on server**: active runtime skill exposed through `/v1/skills`

## Rules
- do not claim a locally built skill is server-loaded unless the server actually reports it
- keep Swift scoped to server-loaded skills only
- keep this slice focused on semantics and wording, not a larger lifecycle redesign

## Acceptance criteria
- TUI `/skills` no longer implies that all discovered local skills are active on the server
- Swift Skills UI continues to mean "loaded on server"
- local built/install discovery wording is accurate and specific
- users can tell which step they have completed and which step they have not

## Validation
- inspect TUI `/skills` output with:
  - a built-only skill
  - an installed-but-not-loaded skill if reproducible
  - a loaded server skill
- inspect Swift Skills UI and verify it still matches `/v1/skills`

## Done means
- the TUI and Swift app no longer appear to contradict each other
- users can reason about the lifecycle without reading engine code
