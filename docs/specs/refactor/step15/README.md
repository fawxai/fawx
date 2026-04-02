# Step 15: PR-Sized Execution Pack

## Purpose
This folder breaks Step 15 into PR-sized implementation specs so a local Codex agent can execute the Swift session archive / browser / export UI work one slice at a time without having to infer the decomposition plan.

This pack follows **Option A** only:
- single-session archive / unarchive
- archived-session filter
- single-session export
- no bulk archive or bulk export in this step

Use these only after:
- Step 14 is fully merged on `dev`
- the working branch starts from fresh `origin/dev`
- the implementation stays aligned to the shipped Step 14 backend contract

## Execution rules
- One file in this folder = one PR-sized slice
- Run slices sequentially, not in parallel
- Fresh worktree for every slice
- Branch from current `origin/dev` every time
- No stacked PR tower unless a slice proves impossible to separate cleanly
- No backend contract redesign in this pack
- No bulk archive or bulk export work in this pack

## Global contract for Step 15
- use `archived=active|all|only`, not `include_archived=true`
- use `POST /v1/sessions/{id}/archive`
- use `DELETE /v1/sessions/{id}/archive`
- use `GET /v1/sessions/{id}/export?format=text|json`
- active view remains the default browser view
- archive remains distinct from clear and delete
- export stays per-session in this step

## Global validation gate
Every slice must pass the appropriate build and test gates for the touched code.

For Swift slices, require at minimum:
- formatting / lint steps already used by the app workflow if touched
- a successful build on the Mac build node for the affected Apple targets
- targeted UI/manual smoke for the feature introduced by the slice

The final slice must include a full manual smoke covering archive, filter, export, and clear/delete distinction.

## Suggested execution order
1. `step15-1-swift-client-and-models.md`
2. `step15-2-session-view-model-archive-state.md`
3. `step15-3-session-browser-archive-filter-ui.md`
4. `step15-4-session-export-ui-and-final-smoke.md`

## Fresh worktree template
```bash
git fetch origin
git worktree add /tmp/fawx-step15-<slice> origin/dev
cd /tmp/fawx-step15-<slice>
git checkout -b codex/step15-<slice-name>
git reset --hard origin/dev
```

## Notes for implementers
- Ground every change in the shipped Step 14 backend contract, not the stale issue draft.
- Read the existing Swift files before editing. The current app already has session list, clear/delete, grouped browsing, and macOS multi-select delete. Do not redesign those surfaces casually.
- Keep Option A scope tight. If bulk actions are needed later, that is a follow-up step.
- Prefer shared model/view-model plumbing before UI duplication across macOS and iOS.
