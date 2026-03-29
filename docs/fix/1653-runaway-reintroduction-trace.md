# #1653 Runaway Tool-Call Reintroduction Trace

## Status
Working trace for the runaway tool-call regression that reappeared after `#1652` had already been partially fixed.

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

## Chain of custody

### Runaway tool-call regression v1
- Introduced by `8c6ec27a`
- Fixed by `e331ab5c`
- Absent at `e34dc733`

### Runaway tool-call regression v2
- Reintroduced by `13376838` (`fix(kernel): preserve mixed text across tool rounds`)

This means the current runaway behavior is **not** just the original `#1652` regression still hanging around unchanged. It was repaired once, then reintroduced later.

## Important distinction
The read-failure / fake-permission bug and the runaway bug are currently stacked on the same prompt, but they do **not** share the same introduction point.

- Read failure was introduced by `8c6ec27a` and never repaired.
- Runaway was introduced by `8c6ec27a`, repaired by `e331ab5c`, then reintroduced by `13376838`.

## Likely regression area
The reintroduction point is inside the initial mixed-text preservation change from `#1653`, not the follow-up fix commit `16389d24`.

The most likely surface is the mixed-text continuation / completion plumbing added in `13376838`, especially around:

- `ToolRoundState.accumulated_text`
- `record_tool_round_response_state(...)`
- `finalize_tool_response(...)`
- `synthesize_tool_fallback(...)`
- `tool_continuation_action_result(...)`
- `prepend_accumulated_text_to_action(...)`

Working theory: preserving mixed text changed turn completion semantics and caused turns that should terminate to continue looping.

## Not the same as
This trace is **not** the `Unknown tool call` symptom. That should stay separate unless later evidence ties the two together.

This trace is also **not** the original direct-inspection fake-permission bug tracked under `#1641`, even though the two bugs compound each other on the same prompt.

## Acceptance criteria for the follow-up fix
- fix the runaway behavior introduced by `13376838`
- preserve the original mixed-text goal of `#1653`
- keep whitespace-only and multi-text-block mixed responses working
- add a regression test that fails on `13376838` and passes on the fix
- confirm the direct-inspection prompt above no longer enters runaway continuation due to this reintroduction
