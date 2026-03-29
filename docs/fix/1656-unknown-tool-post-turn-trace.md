# #1656 Unknown Tool Call Post-Turn Trace

## Status
Working trace for the `Unknown tool call` symptom that appears after a turn finishes.

## Prompt used

`Read the file at ~/.zshrc and explain what each line does`

## Observed checkpoints

### `e34dc733`
- no runaway tool calls
- read-failure / fake-permission still present
- **no `Unknown tool call` after the turn finishes**

### `13376838`
- runaway tool calls present
- read-failure / fake-permission still present
- **no `Unknown tool call` mid-turn**
- **`Unknown tool call` appears after the turn finishes**

## Interpretation
This narrows the symptom away from live tool execution and toward a post-turn boundary problem such as:

- transcript write assembly
- tool-use / tool-result ordering at persistence time
- turn-finalization regrouping
- historical replay / rendering after the run completes

The critical shape is:
- the symptom does **not** appear during live tool execution
- it **does** appear after turn completion on `13376838`
- it is absent after turn completion on `e34dc733`

## Relationship to #1656
This is the same symptom family as `#1656`:
- poisoned session history
- `Unknown tool` fallback rows
- tool-use / tool-result integrity problems around persistence and replay

But this trace does **not** yet prove that the exact root cause is identical to the earlier `tool_result before matching tool_use` case. It does prove that `13376838` is a checkpoint where the post-turn symptom begins appearing.

## Scope boundary
This trace is **not** the direct-inspection execution-profile bug in `#1641`.

It is also separate from the original runaway-tool regression introduced by `#1652`, even though `13376838` currently shows both runaway and post-turn unknown-tool behavior.

## Suggested next checks
- inspect persisted message ordering for the affected turn on `13376838`
- verify whether matching `tool_use` / `tool_result` pairs are written in the correct order
- verify whether any end-of-turn regrouping changes `call_*` / `fc_*` identity mapping
- determine whether the symptom appears immediately in persisted history or only during replay/rendering

## Acceptance criteria extension for #1656
- no `Unknown tool call` appears after turn completion for the prompt above
- persisted history preserves correct `tool_use` / `tool_result` ordering
- grouped tool history renders without unknown-tool fallback rows
- provider continuation cannot fail due to missing/misaligned tool output
