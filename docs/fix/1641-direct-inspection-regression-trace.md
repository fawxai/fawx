# #1641 Direct Inspection Regression Trace

## Status
Working trace for the direct-inspection / fake-permission regression.

## Prompt used

`Read the file at ~/.zshrc and explain what each line does`

## Matrix

| Commit | Runaway tool calls | Read failure / fake permission denial |
|---|---:|---:|
| `92163638` | No | No |
| `8c6ec27a` | Yes | Yes |
| `e331ab5c` | No | Yes |
| `e34dc733` | No | Yes |
| `13376838` | Yes | Yes |
| current `dev` | Yes | Yes |

## Conclusions

### Read-failure / fake-permission regression
- Introduced by `8c6ec27a`
- Still present in `e331ab5c`
- Still present in `e34dc733`
- Still present after `#1653`

This means the direct-inspection read failure was **not** introduced by `#1653`. It predates it.

### Why this belongs under #1641
The behavior matches the direct-inspection profile problem described in:

- `docs/specs/refactor/1641-direct-inspection-profile.md`
- `docs/specs/refactor/1641-phase-1-direct-inspection-detection.md`

The failing shape is:
1. the turn begins in `Standard`
2. observation tools run successfully, including `read_file`
3. the loop obtains a usable post-tool answer
4. instead of finishing, it re-enters the standard outer loop
5. the next pass inherits the wrong continuation contract
6. the model fabricates a blocker such as "outside my working directory" even though the read already succeeded

This is an execution-profile / continuation-contract bug, not a raw filesystem permission failure.

## Scope boundary
This trace is specifically about the **direct-inspection read-failure / fake-permission** regression.

It does **not** cover the separate `Unknown tool call` symptom. That should stay separate unless later evidence proves a shared root cause.

## Requested acceptance criteria for #1641

The direct-inspection slice should explicitly guarantee:

- a successful direct-inspection turn cannot re-enter repeated tool continuation once a usable post-tool answer exists
- a successful direct-inspection read cannot end in an outside-working-directory refusal
- direct-inspection turns never inherit `MutationOnly` follow-up scope from standard observation-only rounds
- the first usable post-tool answer for a direct-inspection turn is terminal

## Related docs
- `docs/specs/refactor/1641-direct-inspection-profile.md`
- `docs/specs/refactor/1641-phase-1-direct-inspection-detection.md`
- `docs/specs/refactor/1641-phase-2-direct-inspection-tool-surface.md`
- `docs/specs/refactor/1641-phase-3-terminal-inspection-completion.md`
- `docs/specs/refactor/1641-phase-4-standard-profile-boundaries.md`
