# Thinking Levels Fix — Spec

Date: 2026-03-18
Status: Implementation-ready
Priority: Ship blocker

## Problem

Thinking levels are broken for both providers:
- **OpenAI**: `reasoning_effort` is never sent. All models use provider default.
- **Anthropic 4.6**: Uses deprecated `budget_tokens` instead of `adaptive` + `effort`.
- **User-facing levels** are incomplete: missing `medium`, `xhigh`, `none`, `max`.

## Correct API payloads (from docs)

### Anthropic Claude 4.6 (Opus/Sonnet)
```json
{
  "thinking": { "type": "adaptive" },
  "effort": "high",
  "max_tokens": 16000
}
```
Effort levels: low, medium, high (default), max (Opus 4.6 ONLY)
`budget_tokens` deprecated on 4.6.

### Anthropic Claude 4.5 and older
```json
{
  "thinking": { "type": "enabled", "budget_tokens": 10000 },
  "max_tokens": 16000
}
```
No effort parameter. No adaptive thinking.

### OpenAI Responses API
```json
{
  "reasoning": { "effort": "xhigh" },
  "model": "gpt-5.4",
  "input": [...]
}
```
GPT-5: minimal, low, medium (default), high
GPT-5.2: none (default), low, medium, high
GPT-5.4: none (default), low, medium, high, xhigh

## Design

### 1. Expand ThinkingConfig (fx-llm/src/types.rs)

```rust
pub enum ThinkingConfig {
    /// Anthropic 4.6 adaptive thinking.
    Adaptive { effort: String },
    /// Anthropic 4.5/older manual thinking.
    Enabled { budget_tokens: u32 },
    /// OpenAI reasoning effort.
    Reasoning { effort: String },
    /// Thinking disabled.
    Off,
}
```

### 2. Fix Anthropic provider (fx-llm/src/anthropic.rs)

Change `AnthropicThinking` to support both wire formats:

```rust
#[derive(Debug, Serialize)]
#[serde(untagged)]
enum AnthropicThinking {
    Adaptive { #[serde(rename = "type")] thinking_type: String },
    Manual { #[serde(rename = "type")] thinking_type: String, budget_tokens: u32 },
}
```

Add `effort` field to `AnthropicRequestBody`:
```rust
#[serde(skip_serializing_if = "Option::is_none")]
effort: Option<String>,
```

In `build_request_body`, detect model version:
```rust
fn is_claude_4_6(model: &str) -> bool {
    model.contains("opus-4-6") || model.contains("sonnet-4-6")
}
```

For 4.6: set `thinking = Adaptive { type: "adaptive" }` and `effort = Some(level)`.
For 4.5: set `thinking = Manual { type: "enabled", budget_tokens }`, no effort.

### 3. Fix OpenAI provider (fx-llm/src/openai_responses.rs)

Add to `ResponsesRequestBody`:
```rust
#[serde(skip_serializing_if = "Option::is_none")]
reasoning: Option<ReasoningConfig>,
```

```rust
#[derive(Debug, Serialize)]
struct ReasoningConfig {
    effort: String,
}
```

In `build_request_body`, read from `request.thinking`:
```rust
let reasoning = match &request.thinking {
    Some(ThinkingConfig::Reasoning { effort }) => Some(ReasoningConfig {
        effort: effort.clone(),
    }),
    _ => None,
};
```

### 4. Simplify valid levels (fx-llm/src/thinking/)

Replace the registry/profile/matching system with a single hardcoded function:

```rust
pub fn valid_thinking_levels(model_id: &str) -> &'static [&'static str] {
    let model = model_id.split('/').last().unwrap_or(model_id);
    match model {
        // Anthropic 4.6
        "claude-opus-4-6" => &["off", "low", "medium", "high", "max"],
        "claude-sonnet-4-6" => &["off", "low", "medium", "high"],
        // Anthropic 4.5/older
        m if m.starts_with("claude-opus-4-5")
            || m.starts_with("claude-sonnet-4-5")
            || m.starts_with("claude-haiku-4-5") => &["off", "low", "high"],
        // OpenAI
        m if m.starts_with("gpt-5.4") => &["none", "low", "medium", "high", "xhigh"],
        m if m.starts_with("gpt-5.2") => &["none", "low", "medium", "high"],
        m if m.starts_with("gpt-5") => &["minimal", "low", "medium", "high"],
        m if m.starts_with("o1") || m.starts_with("o3") => &["low", "medium", "high"],
        // Default
        _ => &["off"],
    }
}

pub fn default_thinking_level(model_id: &str) -> &'static str {
    let model = model_id.split('/').last().unwrap_or(model_id);
    match model {
        "claude-opus-4-6" | "claude-sonnet-4-6" => "high",
        m if m.starts_with("claude-") => "high",
        m if m.starts_with("gpt-5.4") || m.starts_with("gpt-5.2") => "none",
        m if m.starts_with("gpt-5") => "medium",
        _ => "off",
    }
}
```

### 5. Fix ThinkingConfig construction (fx-cli/src/helpers.rs)

Replace `thinking_config_from_budget` with a model-aware function:

```rust
pub fn thinking_config_for_model(model_id: &str, level: &str) -> Option<ThinkingConfig> {
    let model = model_id.split('/').last().unwrap_or(model_id);
    if level == "off" || level == "none" {
        return Some(ThinkingConfig::Off);
    }
    // Anthropic 4.6
    if model.contains("opus-4-6") || model.contains("sonnet-4-6") {
        return Some(ThinkingConfig::Adaptive { effort: level.to_string() });
    }
    // Anthropic older
    if model.starts_with("claude-") {
        let budget = match level {
            "low" => 1_024,
            "medium" => 4_096,
            "high" => 10_000,
            _ => 4_096,
        };
        return Some(ThinkingConfig::Enabled { budget_tokens: budget });
    }
    // OpenAI
    if model.starts_with("gpt-") || model.starts_with("o1") || model.starts_with("o3") {
        return Some(ThinkingConfig::Reasoning { effort: level.to_string() });
    }
    None
}
```

### 6. Update ThinkingBudget (fx-config)

Expand to include all levels:
```rust
pub enum ThinkingBudget {
    Off,
    None,      // OpenAI "none"
    Minimal,   // OpenAI GPT-5 only
    Low,
    Medium,
    High,
    Max,       // Anthropic Opus 4.6 only
    Xhigh,     // OpenAI GPT-5.4 only
    Adaptive,  // Legacy alias → "high" for 4.6
}
```

OR simpler: just use String and validate against `valid_thinking_levels()`.

## File changes

| File | Change |
|------|--------|
| `engine/crates/fx-llm/src/types.rs` | Expand `ThinkingConfig` enum |
| `engine/crates/fx-llm/src/anthropic.rs` | `AnthropicThinking` variants, `effort` field, model detection |
| `engine/crates/fx-llm/src/openai_responses.rs` | `ReasoningConfig`, `reasoning` field |
| `engine/crates/fx-llm/src/thinking/` | Replace with `valid_thinking_levels()` + `default_thinking_level()` |
| `engine/crates/fx-cli/src/helpers.rs` | `thinking_config_for_model()` replaces `thinking_config_from_budget()` |
| `engine/crates/fx-config/src/lib.rs` | Expand or simplify `ThinkingBudget` |

## Estimated scope

~400-500 lines of changes. Multiple files but each change is surgical.
