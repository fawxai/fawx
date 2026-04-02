# Step 1: Restore a Real Signing Command Surface

## Goal
Make the skill signing path real and consistent across CLI and TUI.

## Why this slice exists
Today the TUI advertises `/sign <skill>` and `/sign --all`, but the user is redirected to `fawx sign <skill>`, which is not a real first-class CLI command surface. That is a release blocker because the product is actively instructing users to run a command that does not exist.

## Expected targets
- `engine/crates/fx-cli/src/main.rs`
- `engine/crates/fx-cli/src/commands/skills.rs` or a closely related skill command module
- `engine/crates/fx-cli/src/headless/command.rs`
- TUI/help text that currently advertises signing

## Required outcome
Choose one real command path and make every surface agree with it.

Recommended outcome:
- restore or add a real CLI signing command
- make `/sign` either invoke that logic or accurately delegate to it
- support both one-skill signing and all-installed-skills signing if both are advertised

## Rules
- do not leave dead help text behind
- do not keep a redirect to a nonexistent command
- signing success/failure output should clearly name the skill being signed
- if `--all` is supported in TUI, it must be supported in the real underlying command path too

## Acceptance criteria
- there is a real CLI signing command that matches product guidance
- TUI `/sign` no longer points users to a dead path
- help text, slash help, and CLI help all agree on the supported syntax
- signing can be executed for a single skill and for all installed skills if both are documented

## Validation
- run CLI help for the chosen sign command
- run the TUI slash help path and confirm it matches the CLI
- manually sign one installed skill
- manually sign all installed skills if `--all` is supported

## Done means
- users are no longer told to use a command that does not exist
- the signing story is coherent enough for release
