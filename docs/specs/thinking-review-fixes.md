# Thinking Review Fixes — PR #1500 Follow-up

Branch: `fix/thinking-review-findings` (from `origin/dev`)

Fix ALL 6 findings from the retroactive review of PR #1500.

## Finding 1 (BLOCKING): `adaptive` on OpenAI maps to `"none"` effort

**File:** `engine/crates/fx-llm/src/thinking/defaults.rs`

`thinking_config_for_model("gpt-5.4", "adaptive")` calls `default_thinking_level("gpt-5.4")` which returns `"none"`, producing `Reasoning { effort: "none" }`. This disables reasoning entirely.

**Fix:** When `level == "adaptive"` and the model is OpenAI, map to `Reasoning { effort: "medium" }` instead of using `default_thinking_level()`. "Adaptive" means "reasonable default", not "off".

Also fix the test `adaptive_alias_maps_to_model_default_effort` which currently asserts the wrong behavior.

## Finding 2 (BLOCKING): No way to disable reasoning for o1/o3

**File:** `engine/crates/fx-llm/src/thinking/defaults.rs`

`valid_thinking_levels` for o1/o3 returns `["low", "medium", "high"]` with no `"off"` or `"none"`.

**Fix:** Add `"off"` to valid levels for o1/o3. When `"off"` is selected for these models, `thinking_config_for_model` should return `ThinkingConfig::Off`.

## Finding 3 (NON-BLOCKING): Extra blank lines in startup.rs

**File:** `engine/crates/fx-cli/src/startup.rs`

Run `cargo fmt --all` and verify no consecutive blank lines remain between test functions.

## Finding 4 (NON-BLOCKING): Duplicated model-matching logic

**Files:** `engine/crates/fx-llm/src/thinking/defaults.rs`

`valid_thinking_levels` and `thinking_config_for_model` both have separate `if` chains doing model-family classification.

**Fix:** Extract a private `ModelFamily` enum:
```rust
enum ModelFamily {
    ClaudeOpus46,
    ClaudeSonnet46,
    Claude45,
    ClaudeLegacy,
    Gpt54,
    Gpt5,
    Gpt52,
    O1O3,
    Unknown,
}

fn classify_model(model_id: &str) -> ModelFamily { ... }
```

Then both `valid_thinking_levels` and `thinking_config_for_model` use `classify_model()` instead of duplicated if-chains.

## Finding 5 (NON-BLOCKING): Runtime rejection cache removed

The old `RejectionCache` handled provider-side runtime rejections. It was removed with the ThinkingRegistry.

**Fix:** Add a simple `tracing::warn!` in the provider code paths (anthropic.rs, openai_responses.rs) when a thinking/reasoning config is sent but the provider returns an error indicating the level isn't supported. This provides visibility without the complexity of the old cache. A proper rejection cache can be added post-ship if needed.

## Finding 6 (NICE-TO-HAVE): Silent fallback for unknown Claude levels

**File:** `engine/crates/fx-llm/src/thinking/defaults.rs`

The catch-all `_ => 4_096` in `thinking_config_for_model` for older Claude models silently accepts any level string.

**Fix:** Add `tracing::warn!(level, model, "unexpected thinking level for model, using default budget")` before the fallback.

## Build Requirements

After all fixes:
```bash
cargo fmt --all
cargo clippy --workspace --tests -- -D warnings
cargo test --workspace --exclude fx-skills
```

All must pass. The only allowed failure is the pre-existing `auth_sign_and_keys_commands_work_in_headless_mode` test.
