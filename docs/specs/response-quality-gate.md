# Spec: Response Quality Gate (PR B)

## Problem

When the agentic loop ends with `LoopResult::NeedsInput`, the raw prompt text surfaces as the user-visible response. Users see things like:

- "What's your question?"
- "I need a bit more detail to continue safely. Could you clarify your goal?"
- "Answer the user's question using these tool results"

Similarly, `LoopResult::Error` surfaces raw error text (git errors, tool block messages, network failures) directly to the user.

These are internal engine prompts and debug messages, not user-facing responses. They should never reach the UI.

## Root Cause

`extract_response_text` in `headless.rs` passes through internal text verbatim:

```rust
fn extract_response_text(result: &LoopResult) -> String {
    match result {
        LoopResult::Complete { response, .. } => response.clone(),
        LoopResult::BudgetExhausted { partial_response, .. } => partial_response.clone().unwrap_or_default(),
        LoopResult::NeedsInput { prompt, .. } => prompt.clone(),      // ← bug
        LoopResult::UserStopped { partial_response, .. } => partial_response.clone().unwrap_or_default(),
        LoopResult::Error { message, .. } => format!("error: {message}"),  // ← bug
    }
}
```

The same pattern exists in the HTTP handler path via `CycleResult.response`.

## Solution

### 1. Classify response quality in `extract_response_text`

Replace the passthrough with quality-aware extraction:

```rust
fn extract_response_text(result: &LoopResult) -> String {
    match result {
        LoopResult::Complete { response, .. } => response.clone(),
        LoopResult::BudgetExhausted { partial_response, .. } => {
            partial_response.clone().unwrap_or_else(|| 
                "I ran out of processing budget before finishing. Could you try again or simplify the request?".to_string()
            )
        }
        LoopResult::NeedsInput { .. } => {
            "I wasn't able to produce a complete answer. Could you rephrase or provide more context?".to_string()
        }
        LoopResult::UserStopped { partial_response, .. } => {
            partial_response.clone().unwrap_or_default()
        }
        LoopResult::Error { message, .. } => {
            classify_error_response(message)
        }
    }
}
```

### 2. Error classification function

Create `classify_error_response` that converts internal errors into user-friendly messages:

```rust
fn classify_error_response(message: &str) -> String {
    let lower = message.to_lowercase();
    
    if lower.contains("timeout") || lower.contains("timed out") {
        return "The request timed out. The operation may still be running. You can try again.".to_string();
    }
    if lower.contains("blocked") || lower.contains("denied") || lower.contains("not permitted") {
        return "That action isn't allowed under the current permission settings.".to_string();
    }
    if lower.contains("rate limit") || lower.contains("429") {
        return "The API rate limit was hit. Please wait a moment and try again.".to_string();
    }
    if lower.contains("authentication") || lower.contains("unauthorized") || lower.contains("401") {
        return "There was an authentication issue with the API provider. Check your credentials in settings.".to_string();
    }
    if lower.contains("network") || lower.contains("connection") || lower.contains("dns") {
        return "A network error occurred. Check your internet connection and try again.".to_string();
    }
    
    // Fallback: don't leak raw error text, but acknowledge the failure
    format!("Something went wrong while processing your request. Please try again.")
}
```

### 3. Add `result_kind` to CycleResult

The `CycleResult` struct should indicate what kind of result it is, so the Swift UI can differentiate:

In `headless.rs`, update `CycleResult`:

```rust
pub struct CycleResult {
    pub response: String,
    pub model: String,
    pub iterations: u32,
    pub tokens_used: TokenUsage,
    pub result_kind: ResultKind,  // NEW
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ResultKind {
    Complete,
    Partial,        // BudgetExhausted or UserStopped with content
    NeedsInput,     
    Error,
    Empty,          // No meaningful response produced
}
```

Set `result_kind` in `finalize_cycle`:

```rust
fn finalize_cycle(&mut self, input: &str, result: &LoopResult) -> CycleResult {
    let response = extract_response_text(result);
    let result_kind = match result {
        LoopResult::Complete { .. } => ResultKind::Complete,
        LoopResult::BudgetExhausted { partial_response, .. } => {
            if partial_response.as_ref().map_or(true, |s| s.trim().is_empty()) {
                ResultKind::Empty
            } else {
                ResultKind::Partial
            }
        }
        LoopResult::NeedsInput { .. } => ResultKind::NeedsInput,
        LoopResult::UserStopped { partial_response, .. } => {
            if partial_response.as_ref().map_or(true, |s| s.trim().is_empty()) {
                ResultKind::Empty
            } else {
                ResultKind::Partial
            }
        }
        LoopResult::Error { .. } => ResultKind::Error,
    };
    // ... rest of finalize_cycle
    CycleResult {
        response,
        model: self.active_model.clone(),
        iterations,
        tokens_used,
        result_kind,
    }
}
```

### 4. HTTP response includes result_kind

In the API response JSON, include `result_kind` so Swift can style error/partial/needs-input responses differently (e.g., muted text, warning icon, retry button).

The existing `ApiCycleResult` in `engine.rs` needs the same field added.

## Files Changed

| File | Change |
|------|--------|
| `engine/crates/fx-cli/src/headless.rs` | `extract_response_text` rewrite, `classify_error_response` new fn, `ResultKind` enum, `CycleResult` updated |
| `engine/crates/fx-api/src/engine.rs` | `ApiCycleResult` gets `result_kind` field |
| `engine/crates/fx-api/src/handlers/sessions.rs` | Pass `result_kind` through to HTTP response |

## Testing

### Unit Tests

1. **`NeedsInput` produces friendly message:** `LoopResult::NeedsInput { prompt: "What's your question?".into(), .. }` → response should NOT contain "What's your question?"
2. **`Error` with timeout:** `LoopResult::Error { message: "request timed out".into() }` → response mentions timeout, not raw error
3. **`Error` with blocked tool:** `LoopResult::Error { message: "Tool 'run_command' blocked by policy".into() }` → response mentions permissions, not raw message
4. **`Error` with auth failure:** message containing "401" or "unauthorized" → auth-related user message
5. **`Error` with network failure:** message containing "connection refused" → network error user message
6. **`Error` with unknown error:** arbitrary error string → generic fallback, no raw error leaked
7. **`Complete` unchanged:** normal completion response passes through as-is
8. **`BudgetExhausted` with content:** partial response preserved
9. **`BudgetExhausted` without content:** friendly budget message
10. **`UserStopped` passthrough:** partial response preserved (user chose to stop)
11. **`ResultKind` correct for each variant:** verify `finalize_cycle` sets the right kind

### Manual Testing
1. Ask a question that triggers tool calls → verify no "What's your question?" responses
2. Trigger a permission denial → verify user sees friendly message, not raw block text
3. Disconnect network mid-request → verify user sees network error message
4. Let a request hit budget limit → verify friendly budget message

## Notes

- The `NeedsInput` prompt text is an internal engine construct. The user already provided input; the engine just failed to produce a satisfying response. Surfacing the internal prompt as a response is always wrong.
- `classify_error_response` intentionally does NOT include the raw error text. Leaking internal details (file paths, API endpoints, stack traces) is a security/UX concern.
- `ResultKind` enables future Swift UI improvements: retry buttons for errors, warning styling for partial responses, different animation for needs-input.
- This does NOT change the loop engine behavior. It only changes how results are presented to the user at the output boundary.
