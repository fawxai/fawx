# Step 4: End-to-End Skill Lifecycle Verification

## Goal
Prove the final skill lifecycle story end to end before release.

## Why this slice exists
The prior slices can repair commands, wording, and docs, but release confidence requires a real end-to-end smoke proving that a local custom skill can move through the documented lifecycle without ambiguity.

## Expected targets
- tests or smoke notes as appropriate
- release checklist docs if needed
- any final small fixes required to make the documented flow actually hold together

## Required verification flow
Use a local custom skill and prove this sequence:
1. create or modify a local skill project
2. use the documented canonical command path to build/install it
3. sign it with the real supported sign command if signing is part of the recommended flow
4. verify where the skill appears locally
5. verify whether it is installed in `~/.fawx/skills`
6. start or restart the server as needed
7. verify it appears in the server-loaded skills API or equivalent runtime surface
8. verify the Swift app reflects the same server-loaded state
9. verify the TUI wording does not overclaim any state transition

## Rules
- keep the smoke grounded in the actual release workflow, not a synthetic one-off path
- if the end-to-end flow still requires an unintuitive manual step, document it explicitly or fix it
- no shipping with dead signing instructions still present

## Acceptance criteria
- the documented workflow works as written
- command/help/docs/UI semantics line up with observed behavior
- a user can tell whether a skill is built, installed, or loaded on server
- the release no longer depends on tribal knowledge for local custom skills

## Done means
- the skill lifecycle story is coherent enough to ship
- the signing path is trustworthy enough to ship
- local custom skill debugging no longer starts with contradictory product surfaces
