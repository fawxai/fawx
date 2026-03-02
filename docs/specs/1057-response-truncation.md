# #1057 — Response Truncation Bug Fix

## Status: Spec Complete | Complexity: Medium

> **Note:** Line references target `origin/main` as of 2026-03-02. Branch-local offsets from unmerged features do not affect spec accuracy.

---

## 1. Problem Statement

Fawx responses are being cut off mid-sentence when output hits the configured
`max_tokens` ceiling.

Observed failure case:

- **Input tokens:** ~222k (large context window)
- **Output tokens at truncation:** ~2,667
- **Cause:** `REASONING_MAX_OUTPUT_TOKENS` is currently **768** in
  `loop_engine.rs` (line 242). Anthropic fallback default is **1024** when
  `max_tokens` is `None` (`anthropic.rs:158`).

Today, the loop treats truncated responses as complete. We already carry
`CompletionResponse.stop_reason`, but truncation handling is incomplete:

- **Anthropic:** `stop_reason: "max_tokens"` when capped.
- **OpenAI Chat Completions:** `finish_reason: "length"`.
- **OpenAI Responses API:** can return `status: "incomplete"`; this currently
  does not get normalized to `"max_tokens"`, so truncation can be missed.

This affects both:

1. **Main reasoning path** (`reason()`)
2. **Tool continuation path** (`act_with_tools` → `finalize_tool_response`)

The tool-synthesis fallback path is separately capped at 384 tokens and should
also be increased.

---

## 2. Exact Files to Change

### Primary changes

| File | Lines | Change |
|------|-------|--------|
| `engine/crates/fx-kernel/src/loop_engine.rs` | 242–244 | Raise `REASONING_MAX_OUTPUT_TOKENS` 768 → 4096 and `TOOL_SYNTHESIS_MAX_OUTPUT_TOKENS` 384 → 1024 |
| `engine/crates/fx-kernel/src/loop_engine.rs` | `reason()` (starts at 658), tool path (`request_tool_continuation` at 1404 + `finalize_tool_response` at 1427) | Detect truncation and auto-continue in both reasoning and tool-result response paths (before final extraction) |
| `engine/crates/fx-kernel/src/loop_engine.rs` | helper section near existing prompt/text helpers (~2200+) | Add/extend truncation helpers: `is_truncated()`, `merge_usage()`, `continue_truncated_response()` |

### Secondary changes

| File | Lines | Change |
|------|-------|--------|
| `engine/crates/fx-llm/src/lib.rs` | 46–60 | No truncation-detection changes in legacy `generate`/`generate_streaming` trait (`String`-only return, no stop metadata) |
| `engine/crates/fx-llm/src/types.rs` | 34–43 | No struct changes needed—`stop_reason: Option<String>` already exists on `CompletionResponse` |
| `engine/crates/fx-llm/src/anthropic.rs` | 158 | Raise default `max_tokens` fallback from 1024 to 4096 |
| `engine/crates/fx-llm/src/openai.rs` | ~139, ~171–179 + tests | Keep Chat Completions mapping (`finish_reason` passthrough, incl. `"length"`) and add/retain regression coverage |
| `engine/crates/fx-llm/src/openai_responses.rs` | test-only parse helper (~173), done-event path (~340), SSE structs (~620), tests | Keep `parse_response` (`#[cfg(test)]`) normalization for test parity; implement production fix in `usage_chunk_from_done_event` by decoding `response.status` and mapping `"incomplete"` → `"max_tokens"`; explicitly add `status: Option<String>` to `SseResponseBody` |

### TUI (display layer — informational only)

| File | Lines | Change |
|------|-------|--------|
| `engine/crates/fx-cli/src/tui.rs` | 2196–2220 (`loop_result_response_text`) | No change needed—stitched response still arrives via `LoopResult::Complete` |

**Note:** No `continuation.rs` changes are required for this bug.
Truncation continuation logic is in `loop_engine.rs`.

---

## 3. API Design

### 3.1 Provider stop-reason normalization (required)

`LoopEngine` truncation detection should remain provider-agnostic and operate on
`CompletionResponse.stop_reason`.

To make that reliable, provider adapters must normalize truncation signals:

- Anthropic: keep `"max_tokens"`
- OpenAI Chat: keep `"length"`
- OpenAI Responses: map `status: "incomplete"` → `"max_tokens"`

In `openai_responses.rs`, apply normalization in these two contexts with clear scope:

1. **Test-only helper (`parse_response`)** — keep mapping for unit-test consistency.
   `parse_response` is `#[cfg(test)]` and is not a production runtime path.
2. **Production stream done-event mapping (`usage_chunk_from_done_event`)** —
   this is the runtime path used by `complete_via_stream`. Extend done-event
   decoding by adding `status: Option<String>` to `SseResponseBody`, then map
   `response.status == "incomplete"` to `stop_reason: "max_tokens"`.

`LoopEngine` helper:

```rust
fn is_truncated(stop_reason: Option<&str>) -> bool {
    matches!(
        stop_reason.map(|s| s.to_ascii_lowercase()).as_deref(),
        Some("max_tokens" | "length")
    )
}
```

(Defensive fallback to also treat `"incomplete"` as truncated is acceptable,
but provider-side normalization is the primary fix.)

### 3.1.1 `complete()` default/legacy path decision (explicit)

No trait-default `complete()` truncation logic is introduced in this spec.

- Legacy prompt APIs in `fx-llm/src/lib.rs` (`generate` / `generate_streaming`,
  lines 46–60) return raw `String` and do not expose `stop_reason`.
- Therefore #1057 performs truncation detection only where
  `CompletionResponse.stop_reason` is available (provider-backed `complete()`
  calls used by `loop_engine.rs`).
- This explicitly avoids speculative heuristics (e.g., char-count/token-count
  guessing) on string-only paths.

### 3.2 Continuation logic in `loop_engine.rs`

Add continuation handling in both paths:

1. `reason()` path (initial reasoning completion)
2. Tool path before final response extraction

In `reason()`, capture the request messages from `build_reasoning_request(...)`
**before** moving `request` into `llm.complete(...)`:

```rust
let request = build_reasoning_request(...);
let reasoning_context_messages = request.messages.clone(); // capture before move
let response = llm.complete(request).await?;
let response = continue_truncated_response(
    response,
    &reasoning_context_messages,
    llm,
    LoopStep::Reason,
).await?;
```

`continue_truncated_response()` should iterate up to 3 attempts:

```rust
const MAX_CONTINUATION_ATTEMPTS: u32 = 3;

async fn continue_truncated_response(
    &mut self,
    initial_response: CompletionResponse,
    base_messages: &[Message],
    llm: &dyn LlmProvider,
    step: LoopStep,
) -> Result<CompletionResponse, LoopError> {
    let mut full_text = extract_response_text(&initial_response);
    let mut last = initial_response;
    let mut attempts = 0;

    while is_truncated(last.stop_reason.as_deref()) && attempts < MAX_CONTINUATION_ATTEMPTS {
        attempts += 1;

        let mut continuation_messages = base_messages.to_vec();
        continuation_messages.push(Message::assistant(full_text.clone()));
        continuation_messages.push(Message::user(
            "Continue from exactly where you left off. Do not repeat prior text.",
        ));

        // Reuse same system prompt basis as original request.
        let tools = tool_definitions_with_decompose(self.tool_executor.tool_definitions());
        let continuation_request = CompletionRequest {
            model: llm.model_name().to_string(),
            messages: continuation_messages,
            tools: Vec::new(),
            temperature: Some(REASONING_TEMPERATURE),
            max_tokens: Some(REASONING_MAX_OUTPUT_TOKENS),
            system_prompt: Some(build_reasoning_system_prompt(&tools, self.memory_context.as_deref())),
        };

        // Budget check/record per continuation call (same as any LLM call).
        // ... self.budget.check_at(...)
        let stage = match step {
            LoopStep::Reason => "reason",
            LoopStep::Act => "act",
            _ => "act",
        };
        let continued = llm.complete(continuation_request).await.map_err(|error| {
            loop_error(
                stage,
                &format!("continuation completion failed: {error}"),
                true,
            )
        })?;
        // ... self.budget.record(...)

        let new_text = extract_response_text(&continued);
        // Seam de-dupe before append (120-char window, 80-char minimum overlap).
        let deduped = trim_duplicate_seam(&full_text, &new_text, 120, 80);
        full_text.push_str(&deduped);
        last = CompletionResponse {
            content: vec![ContentBlock::Text { text: full_text.clone() }],
            tool_calls: continued.tool_calls,
            usage: merge_usage(last.usage, continued.usage),
            stop_reason: continued.stop_reason,
        };

        self.emit_signal(
            step,
            SignalKind::Trace,
            format!("response truncated, continuing ({attempts}/{MAX_CONTINUATION_ATTEMPTS})"),
            serde_json::json!({"attempt": attempts}),
        );
    }

    Ok(last)
}
```

`trim_duplicate_seam(...)` is the seam de-dupe insertion point (between
`extract_response_text(&continued)` and `full_text.push_str(...)`).

Integration contract for signal semantics:

- `reason()` builds the request via `build_reasoning_request(...)`, clones `request.messages` before `llm.complete(...)`, then calls `continue_truncated_response(..., LoopStep::Reason)`
- Tool continuation path (`request_tool_continuation` → `finalize_tool_response`)
  calls `continue_truncated_response(..., LoopStep::Act)`

### 3.3 Helper function definitions and reuse points

- `extract_response_text()` is already an existing private helper in
  `loop_engine.rs` (around line 2259). It extracts text from
  `ContentBlock::Text` variants. Reuse this helper directly.
- `build_reasoning_system_prompt()` is already an existing function in
  `loop_engine.rs` (around line 2206). Reuse it for continuation requests.
- Add/use a small seam helper with explicit signature and return type:
  `trim_duplicate_seam(full_text: &str, new_text: &str, overlap_window: usize, min_overlap: usize) -> String`.
  Semantics: return only the non-duplicated suffix to append; if no overlap meeting
  `min_overlap` is found, return `new_text.to_string()` unchanged. Overlap matching
  and trimming must operate on UTF-8 character boundaries (never split a code point).
- Message constructors are already available in `fx-llm`:
  `Message::assistant(...)` and `Message::user(...)` (`types.rs` lines ~66 and
  ~58). Use these constructors rather than manual struct literals.

### 3.4 `merge_usage()` behavior (required)

`merge_usage()` must sum token counts from two optional `Usage` values:

```rust
fn merge_usage(left: Option<Usage>, right: Option<Usage>) -> Option<Usage> {
    if left.is_none() && right.is_none() {
        return None;
    }

    let left_in = left.as_ref().map(|u| u.input_tokens).unwrap_or(0);
    let left_out = left.as_ref().map(|u| u.output_tokens).unwrap_or(0);
    let right_in = right.as_ref().map(|u| u.input_tokens).unwrap_or(0);
    let right_out = right.as_ref().map(|u| u.output_tokens).unwrap_or(0);

    Some(Usage {
        input_tokens: left_in + right_in,
        output_tokens: left_out + right_out,
    })
}
```

### 3.5 Tool-path continuation behavior

There are two tool-related LLM paths, and they are handled differently:

1. **Tool continuation response path** (`request_tool_continuation` →
   `finalize_tool_response`)
   - If `stop_reason` is truncated, auto-continue before final text extraction.
   - Use `LoopStep::Act` for continuation trace signals.
   - Reuse the same system-prompt basis and normal budget accounting.

2. **Tool synthesis fallback path** (`generate_tool_summary` via
   `generate_streaming`)
   - This path returns streamed/raw `String` and does not expose `stop_reason`.
   - Auto-continuation is therefore **out of scope for #1057** on this path.
   - #1057 mitigation here is only increasing
     `TOOL_SYNTHESIS_MAX_OUTPUT_TOKENS` from 384 to 1024.

### 3.6 Max tokens configuration

| Constant | Current | New | Rationale |
|----------|---------|-----|-----------|
| `REASONING_MAX_OUTPUT_TOKENS` | 768 | 4096 | Substantially reduces truncation on large-context responses |
| `TOOL_SYNTHESIS_MAX_OUTPUT_TOKENS` | 384 | 1024 | Tool-result synthesis frequently needs more output room |
| Anthropic fallback | 1024 | 4096 | Keep provider default aligned with loop reasoning cap |

---

## 4. Implementation Plan

**Important:** Phase 1 and Phase 2 ship in the **same PR**. These phases are
implementation order only, not separate releases.

### Phase 1: Token ceilings + provider mapping

1. Raise constants in `loop_engine.rs`:
   - `REASONING_MAX_OUTPUT_TOKENS`: 768 → 4096
   - `TOOL_SYNTHESIS_MAX_OUTPUT_TOKENS`: 384 → 1024
2. Raise Anthropic fallback in `anthropic.rs`: 1024 → 4096
3. In OpenAI Responses mapping:
   - Keep `parse_response` normalization for `#[cfg(test)]` parity
   - In production `usage_chunk_from_done_event`, decode `response.status`
     from `SseResponseBody` (add `status: Option<String>`) and normalize
     `"incomplete"` → `"max_tokens"`
4. Add/update provider tests for truncation stop-reason mapping

**Estimated effort:** 2–3 hours.

### Phase 2: Auto-continuation in loop engine

1. Add/confirm helpers: `is_truncated()`, `merge_usage()`,
   `continue_truncated_response()`
2. Integrate into `reason()` using `LoopStep::Reason`
3. Integrate into the tool continuation path before final response extraction using `LoopStep::Act`
4. Emit trace signals on each continuation attempt
5. Ensure each continuation call performs budget check + `self.budget.record()`
6. Add loop-engine and integration tests

**Estimated effort:** 4–6 hours.

### Phase 3: Configuration (optional follow-up)

1. Add configurable max output tokens to config/CLI
2. Wire through request builders

---

## 5. Test Plan

### Provider tests (`fx-llm`)

1. **`openai_chat_length_stop_reason_is_preserved`**
   - Verify Chat Completions `finish_reason: "length"` remains `stop_reason: "length"`.

2. **`openai_responses_incomplete_status_maps_to_max_tokens`**
   - Input: Responses API body with `status: "incomplete"` (test helper path).
   - Assert: `parse_response` maps to
     `CompletionResponse.stop_reason == Some("max_tokens")` for parity with
     streaming behavior.

3. **`openai_responses_done_event_incomplete_maps_to_max_tokens`**
   - Input: stream done/completed frame carrying `response.status: "incomplete"`.
   - Assert: terminal `StreamChunk.stop_reason == Some("max_tokens")`.

### Loop-engine unit/integration tests (`loop_engine.rs`)

4. **`is_truncated_detects_anthropic_stop_reason`**
5. **`is_truncated_detects_openai_finish_reason`**
6. **`is_truncated_handles_none_and_unknown`**
7. **`merge_usage_combines_token_counts`** (including `None` branches via `unwrap_or(0)` behavior)
8. **`continue_truncated_response_stitches_text`**
9. **`continue_truncated_response_respects_max_attempts`**
10. **`continue_truncated_response_stops_on_natural_end`**
11. **`run_cycle_auto_continues_truncated_response`**
12. **`tool_continuation_auto_continues_truncated_response`**
13. **`finalize_tool_response_receives_stitched_text_after_continuation`**
14. **`truncation_continuation_emits_reason_and_act_trace_signals`**
15. **`continuation_calls_record_budget`**

### Regression checks

16. **`raised_max_tokens_constants_are_applied`**
   - Verify request builders use new max token values.
17. **`tool_synthesis_uses_raised_token_cap_without_stop_reason_assumptions`**
   - Verify synthesis path uses 1024 cap and does not rely on `stop_reason`-based continuation.

---

## 6. Edge Cases and Risks

### Infinite continuation loop

- **Risk:** model repeatedly hits cap
- **Mitigation:** `MAX_CONTINUATION_ATTEMPTS = 3`

### Duplicate seam text

- **Risk:** continuation repeats trailing text
- **Mitigation:** explicit continuation instruction plus deterministic seam de-dupe:
  if overlap between accumulated suffix and continuation prefix is **≥80 chars**
  (checked over a 120-char window), trim the duplicated prefix before append.

### Context growth

- **Risk:** accumulated assistant text increases prompt size
- **Mitigation:** bounded attempts + existing budget checks

### Tool-path consistency

- **Risk:** tool and reasoning continuations diverge in prompt behavior
- **Mitigation:** reuse same system-prompt construction path
  (`build_reasoning_system_prompt`) for both

### Budget impact

- **Risk:** additional continuation calls consume budget
- **Mitigation:** every continuation call runs normal budget check and
  `self.budget.record()` accounting

---

## 7. Estimated Complexity

| Phase | Effort | Risk | Priority |
|------|--------|------|----------|
| Phase 1: Token + mapping fixes | 2–3 hours | Low | High |
| Phase 2: Auto-continuation | 4–6 hours | Medium | High |
| Phase 3: Config follow-up | 2–3 hours | Low | Optional |

**Total (Phase 1 + 2 in same PR):** ~6–9 hours including tests.

**LoC estimate:**

- **New code:** ~150–200 lines (continuation flow + helpers + provider mapping)
- **Modified code:** ~20–40 lines (constants + integration points)
- **New tests:** ~220–320 lines

**Crates touched:** `fx-kernel` (primary), `fx-llm` (provider mapping/defaults)

---

## Appendix: Current vs Fixed Flow

### Current

```
reason()/tool continuation request
  → llm.complete(... max_tokens=768)
  → stop_reason = "max_tokens" or "length" or Responses "incomplete"
  → response treated as complete
  → truncated text reaches user
```

### After fix

```
reason()/tool continuation request
  → llm.complete(... max_tokens=4096)
  → provider normalizes truncation stop_reason (Responses "incomplete" → "max_tokens")
  → is_truncated() check
  → continue_truncated_response() up to 3 attempts
  → stitched text + merged usage + budget-recorded continuations
  → complete response reaches user
```

**Note:** tool synthesis fallback (`generate_streaming`) remains string-only in
#1057 and is mitigated by the raised 1024 token cap, not stop-reason-based
auto-continuation.
