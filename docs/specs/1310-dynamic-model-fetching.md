# Spec: Dynamic model fetching from provider APIs (#1310)

## Summary

Add `list_models()` to the `LlmProvider` trait so the setup wizard and `/model` command can show real-time available models instead of stale hardcoded lists. Hardcoded lists become fallback when API calls fail.

## Files to touch

- `engine/crates/fx-llm/src/provider.rs` — add `list_models()` to `LlmProvider` trait with default fallback impl
- `engine/crates/fx-llm/src/openai.rs` — implement `list_models()` via `GET /v1/models`
- `engine/crates/fx-llm/src/anthropic.rs` — implement `list_models()` via `GET /v1/models`
- `engine/crates/fx-llm/src/router.rs` — add `fetch_available_models()` that calls providers and merges results
- `engine/crates/fx-cli/src/headless.rs` — use `fetch_available_models()` in model selection, fall back to `available_models()`

## Design

### Trait addition

```rust
// In provider.rs, add to LlmProvider trait:

/// Fetch available models dynamically from the provider API.
///
/// Returns model IDs the current API key/token has access to.
/// Default implementation returns the static `supported_models()` list.
async fn list_models(&self) -> Result<Vec<String>, LlmError> {
    Ok(self.supported_models())
}
```

Using a default implementation means existing providers (fallback, mock, any future local) don't need changes — they'll return their static lists.

### OpenAI implementation

```
GET https://api.openai.com/v1/models
Authorization: Bearer <api_key>
```

Response filtering:
- Only include models whose `id` contains `gpt` or `o1` or `o3` or `o4` (filter out embeddings, whisper, dall-e, tts, etc.)
- Sort alphabetically
- On error (network, auth, 4xx/5xx): log warning, return `self.supported_models()` fallback

### Anthropic implementation

```
GET https://api.anthropic.com/v1/models
x-api-key: <api_key>
anthropic-version: 2023-06-01
```

Response filtering:
- Only include models whose `type` is `"model"` (not deprecated aliases)
- Extract `id` field from each model object
- Sort alphabetically
- On error: log warning, return `self.supported_models()` fallback

### Router integration

Add to `ModelRouter`:

```rust
/// Fetch available models from all registered providers dynamically.
///
/// Calls `list_models()` on each provider in parallel, merges results.
/// Falls back to `available_models()` (static) on per-provider failure.
pub async fn fetch_available_models(&self) -> Vec<ModelInfo> {
    // For each registered provider:
    //   1. Call provider.list_models()
    //   2. On success: map to ModelInfo with provider name + auth method
    //   3. On failure: fall back to static supported_models() for that provider
    // Merge, dedupe by model_id, sort
}
```

### CLI integration

In `headless.rs`, the model menu and setup wizard currently call `router.available_models()`. Change to:

```rust
// Try dynamic first, fall back to static
let models = match router.fetch_available_models().await {
    models if !models.is_empty() => models,
    _ => router.available_models(),
};
```

This applies to:
- `render_model_menu_text()` calls (around line 574)
- Setup wizard model selection
- `/model` command in headless mode

### Caching

Do NOT cache in this first pass. `list_models()` is called rarely (setup wizard, explicit `/model` command) — not on every completion. Caching adds complexity for minimal benefit at current call frequency.

If performance becomes an issue later, add a simple TTL cache (5 minutes) in `ModelRouter`.

## Error handling

| Scenario | Behavior |
|----------|----------|
| Network timeout | Log warn, return static fallback |
| Auth error (401/403) | Log warn, return static fallback |
| Malformed response | Log warn, return static fallback |
| Empty response | Log warn, return static fallback |
| Provider has no API key configured | Skip API call, return static fallback |

The key principle: **dynamic fetch never breaks the setup flow**. If it fails for any reason, the user still sees the hardcoded list and can proceed.

## Testing

### Unit tests

1. **`openai_list_models_parses_response`** — mock HTTP response with sample OpenAI `/v1/models` JSON, verify parsed model IDs, verify non-chat models filtered out
2. **`openai_list_models_filters_non_chat`** — response includes `text-embedding-ada-002`, `dall-e-3`, `gpt-4o` — only `gpt-4o` returned
3. **`openai_list_models_falls_back_on_error`** — mock 500 response, verify returns `supported_models()` static list
4. **`anthropic_list_models_parses_response`** — mock Anthropic `/v1/models` JSON, verify parsed model IDs
5. **`anthropic_list_models_falls_back_on_error`** — mock network error, verify returns `supported_models()` static list
6. **`router_fetch_merges_providers`** — two mock providers, verify merged + sorted + deduped result
7. **`router_fetch_partial_failure`** — one provider succeeds, one fails — verify merged result includes dynamic from success + static from failure
8. **`list_models_default_impl_returns_supported`** — provider with no override returns `supported_models()` via default trait impl
9. **`list_models_skips_unconfigured_provider`** — provider with no API key returns static fallback without attempting HTTP

### Integration test

10. **`headless_model_menu_uses_dynamic_when_available`** — mock router with dynamic results, verify menu text includes dynamic models

## Complexity estimate

~250 lines of new code across 4 files + ~200 lines of tests.

Main risks:
- HTTP client: both providers already use `reqwest` for completions, so the client infrastructure exists. Reuse it.
- Response format changes: pin to documented API contracts, graceful fallback on parse failure.
- Auth: `list_models()` needs the API key. Both providers already store it — pass via `&self`.

No architectural changes. The trait extension is additive (default impl). Router change is additive. CLI change is a small async call + fallback.
