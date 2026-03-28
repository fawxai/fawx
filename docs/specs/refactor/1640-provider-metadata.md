# Spec: #1640 — Move Provider Metadata onto LlmProvider Trait

## Status
Not started. Provider metadata is scattered across free functions and string-matching dispatchers.

## Goal
All provider-specific metadata (thinking levels, catalog endpoints, auth headers, base URLs) should be declared by the provider implementation through trait methods, not matched externally on provider name strings.

## Current State (codex/provider-owned-loop-refactor branch)

### Two LlmProvider traits exist

1. **Legacy** — `engine/crates/fx-llm/src/lib.rs:142`
```rust
pub trait LlmProvider: Send + Sync + std::fmt::Debug {
    async fn generate(&self, prompt: &str, max_tokens: u32) -> Result<String, LlmError>;
    async fn generate_streaming(&self, ...);
    fn model_name(&self) -> &str;
}
```

2. **Newer structured** — `engine/crates/fx-llm/src/provider.rs:285`
```rust
pub trait LlmProvider: Send + Sync {
    async fn complete(&self, request: CompletionRequest) -> Result<CompletionResponse, LlmError>;
    async fn complete_stream(&self, ...) -> Result<CompletionStream, LlmError>;
    async fn stream(&self, ...) -> Result<CompletionResponse, LlmError>;
    fn name(&self) -> &str;
    fn supported_models(&self) -> Vec<String>;
    async fn list_models(&self) -> Result<Vec<String>, LlmError>;
    fn capabilities(&self) -> ProviderCapabilities;
    fn loop_harness(&self, _model: &str) -> &'static dyn LoopHarness;
}
```

The newer trait already has `capabilities()` returning `ProviderCapabilities` and `loop_harness()`. But `ProviderCapabilities` is minimal:
```rust
pub struct ProviderCapabilities {
    pub supports_temperature: bool,
    pub requires_streaming: bool,
}
```

### String-matching instances to eliminate

**1. `supported_thinking_levels()` — `fx-llm/src/lib.rs:127`**
```rust
pub fn supported_thinking_levels(provider: &str) -> Vec<String> {
    match provider.trim().to_ascii_lowercase().as_str() {
        "anthropic" => vec!["off", "low", "adaptive", "high"],
        "openai" => vec!["off", "low", "high"],
        _ => vec!["off"],
    }
}
```
Move to: trait method `fn supported_thinking_levels(&self) -> &[&str]` with default `&["off"]`.

**2. Catalog request builder — `fx-llm/src/model_catalog.rs:166`**
```rust
match provider.as_str() {
    "anthropic" => { headers.insert("anthropic-version", ...); match auth_mode { ... } }
    "openai" | "openrouter" => { match auth_mode { ... } }
    _ => return Err(...)
}
```
Move to: trait method `fn catalog_request(&self, api_key: &str, auth_mode: &str) -> Result<reqwest::Request, String>`.

**3. Models endpoint — `fx-llm/src/model_catalog.rs:383`**
```rust
fn models_endpoint(provider: &str) -> Result<&'static str, String> {
    match provider {
        "anthropic" => Ok(ANTHROPIC_MODELS_ENDPOINT),
        "openai" => Ok(OPENAI_MODELS_ENDPOINT),
        "openrouter" => Ok(OPENROUTER_MODELS_ENDPOINT),
        _ => Err(...)
    }
}
```
Move to: trait method `fn models_endpoint(&self) -> Option<&str>` (None for providers without catalog).

**4. `is_chat_capable` — `fx-llm/src/model_catalog.rs:273`**
```rust
match provider {
    "openai" => { ... }
    "openrouter" => { ... }
    _ => true
}
```
Move to: trait method `fn is_chat_capable(&self, model_id: &str) -> bool` with default `true`.

**5. `hardcoded_fallback` — `fx-llm/src/model_catalog.rs:315`**
```rust
let ids: Vec<&str> = match provider.as_str() {
    "anthropic" => vec![...],
    "openai" => vec![...],
    "openrouter" => vec![...],
    _ => vec![],
};
```
Move to: trait method `fn fallback_models(&self) -> Vec<&str>` with default `vec![]`.

## Deliverables

1. Extend `ProviderCapabilities` or add new trait methods to `LlmProvider` (the newer one in `provider.rs`):
   - `fn supported_thinking_levels(&self) -> &[&str]` (default: `&["off"]`)
   - `fn models_endpoint(&self) -> Option<&str>` (default: `None`)
   - `fn catalog_auth_headers(&self, api_key: &str, auth_mode: &str) -> Result<HeaderMap, String>` (default: bearer token)
   - `fn is_chat_capable(&self, model_id: &str) -> bool` (default: `true`)
   - `fn fallback_models(&self) -> Vec<&str>` (default: `vec![]`)

2. Implement these methods on each concrete provider:
   - `AnthropicProvider` (in `fx-llm/src/anthropic.rs`)
   - `OpenAiProvider` (in `fx-llm/src/openai.rs`, `fx-llm/src/openai_responses.rs`)
   - `OpenRouterProvider` (if separate, or shared with OpenAI)

3. Delete the free functions: `supported_thinking_levels()`, `models_endpoint()`, `hardcoded_fallback()`.

4. Update `ModelCatalog` to use trait methods instead of string matching.

5. All existing tests pass. Add tests verifying each provider returns correct metadata.

## Files to modify
- `engine/crates/fx-llm/src/provider.rs` (trait extension)
- `engine/crates/fx-llm/src/lib.rs` (delete `supported_thinking_levels`)
- `engine/crates/fx-llm/src/model_catalog.rs` (replace 4 match blocks)
- `engine/crates/fx-llm/src/anthropic.rs` (implement methods)
- `engine/crates/fx-llm/src/openai.rs` (implement methods)
- `engine/crates/fx-llm/src/openai_responses.rs` (implement methods if provider lives here)

## Not in scope
- Unifying the two `LlmProvider` traits (legacy in lib.rs vs newer in provider.rs)
- Provider registration/discovery system
- Changes to fx-kernel or fx-tools
