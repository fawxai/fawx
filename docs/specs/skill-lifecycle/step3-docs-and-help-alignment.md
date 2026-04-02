# Step 3: Canonical Workflow and Docs Alignment

## Goal
Choose one canonical local-dev skill workflow and align CLI help, TUI help, and docs around it.

## Why this slice exists
The codebase currently implies multiple competing workflows:
- `skills/build.sh --install`
- `fawx skill build <project>`
- `fawx skill install <path>`

All three can be valid in some context, but the release needs one recommended path so users are not guessing.

## Expected targets
- `docs/WASM_SKILLS.md`
- `README.md`
- TUI skill help text
- CLI help text and examples
- any setup/docs pages that still tell users to use outdated commands

## Required outcome
Pick and document a canonical local-dev workflow.

Recommended framing:
- **Canonical local-dev path**: `fawx skill build <project>`
- **Repo maintainer path**: `skills/build.sh --install` for the built-in repo skills collection
- **Artifact path**: `fawx skill install <path>` for prebuilt wasm or skill directories

The docs should explain when each path is appropriate.

## Extra consistency check
If the build target differs across code and docs (`wasm32-unknown-unknown` vs `wasm32-wasip1` or similar), resolve that contradiction here or write down the exact blocker so the docs stop lying.

## Acceptance criteria
- one recommended local-dev workflow is clearly called out
- repo build-script workflow is described as a specialized path, not the universal answer
- CLI/TUI/docs no longer disagree about signing/build/install commands
- build target examples are consistent with the actual implementation

## Validation
- walk the docs from a new user perspective
- verify each example command actually exists
- verify build-target references match the current implementation

## Done means
- the product has one coherent written skill lifecycle story
- release docs no longer send users down dead or misleading paths
