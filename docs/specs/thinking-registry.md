# Thinking Registry Spec

**Crate:** `fx-llm` (replace `supported_thinking_levels()`)  
**Difficulty:** Medium  
**Dependencies:** None — self-contained in fx-llm  
**Impact:** Swift app settings screen, correct per-model thinking options, forward-compatible with new models  

---

## Problem

The current thinking level implementation is a hardcoded match statement:

```rust
fn supported_thinking_levels(provider: &str) -> Vec<String> {
    match provider {
        "anthropic" => vec!["off", "low", "adaptive", "high"],
        "openai"    => vec!["off", "low", "high"],
        _           => vec!["off"],
    }
}
```

This is wrong in three ways:
1. **Provider-level is too coarse** — Opus 4.6 and Sonnet 4.5 need different API parameters
2. **Hardcoded levels** — every new model requires a code change
3. **No mapping to API parameters** — "high" means different things to different providers (`effort: "high"` vs `budget_tokens: 10000` vs `reasoning_effort: "high"`)

---

## Architecture: Two Layers

### Layer 1: Thinking Registry (primary)

A data-driven registry that maps `model_id → ThinkingProfile`. Bundled in the binary with config overrides.

#### ThinkingProfile

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThinkingProfile {
    /// User-facing levels available in the picker.
    pub levels: Vec<String>,
    /// Default level when user hasn't chosen.
    pub default: String,
    /// How to translate level names into provider API parameters.
    pub api_style: ApiStyle,
    /// Optional: per-level parameter overrides.
    pub level_params: BTreeMap<String, LevelParams>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ApiStyle {
    /// Anthropic new-style: `thinking: { type: "enabled", budget_tokens: N }`
    /// with effort-based levels
    AdaptiveEffort,
    /// Anthropic legacy: `thinking: { type: "enabled", budget_tokens: N }`
    /// with fixed token budgets per level
    BudgetTokens,
    /// OpenAI: `reasoning_effort: "low"|"medium"|"high"`
    ReasoningEffort,
    /// No thinking support
    Disabled,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LevelParams {
    /// For BudgetTokens style: token count for this level.
    pub budget_tokens: Option<u32>,
    /// For AdaptiveEffort style: effort string sent to API.
    pub effort: Option<String>,
    /// For ReasoningEffort style: effort string for OpenAI.
    pub reasoning_effort: Option<String>,
}
```

#### ThinkingRegistry

```rust
pub struct ThinkingRegistry {
    /// Named profiles.
    profiles: BTreeMap<String, ThinkingProfile>,
    /// Model ID → profile name mappings (exact match + glob patterns).
    model_mappings: Vec<ModelMapping>,
    /// Session-scoped rejection cache (Layer 2).
    rejections: Mutex<HashSet<(String, String)>>,  // (model_id, level)
}

#[derive(Debug, Clone)]
struct ModelMapping {
    pattern: String,        // exact model ID or glob like "claude-opus-*"
    profile_name: String,
    priority: MatchPriority,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum MatchPriority {
    Exact = 0,      // "claude-opus-4-6"
    Prefix = 1,     // "claude-opus-4*"
    Wildcard = 2,   // "claude-*"
}
```

#### Registry API

```rust
impl ThinkingRegistry {
    /// Create registry with bundled defaults.
    pub fn with_defaults() -> Self;
    
    /// Look up the thinking profile for a model.
    /// Returns the best-matching profile, or the "none" profile if no match.
    pub fn profile_for_model(&self, model_id: &str) -> &ThinkingProfile;
    
    /// Get available levels for a model, excluding session-rejected levels.
    pub fn available_levels(&self, model_id: &str) -> Vec<String>;
    
    /// Get the default level for a model.
    pub fn default_level(&self, model_id: &str) -> String;
    
    /// Translate a user-facing level into provider API parameters.
    pub fn translate(&self, model_id: &str, level: &str) -> ThinkingParams;
    
    /// Record a runtime rejection (Level 2 — session-scoped).
    pub fn record_rejection(&self, model_id: &str, level: &str);
    
    /// Check if a level has been rejected at runtime for this model.
    pub fn is_rejected(&self, model_id: &str, level: &str) -> bool;
    
    /// Apply config overrides (from models.toml or config.toml).
    pub fn apply_overrides(&mut self, overrides: &ThinkingOverrides);
}

/// The output of translate() — what gets sent to the provider.
#[derive(Debug, Clone)]
pub enum ThinkingParams {
    Disabled,
    Anthropic { budget_tokens: Option<u32>, effort: Option<String> },
    OpenAi { reasoning_effort: String },
}
```

#### Bundled Defaults

```rust
fn default_profiles() -> BTreeMap<String, ThinkingProfile> {
    // anthropic-adaptive: Opus 4.6, Sonnet 4.6
    // levels: off, low, medium, high, max
    // api_style: AdaptiveEffort
    // default: high
    
    // anthropic-adaptive-no-max: Sonnet 4.6 (no max)
    // levels: off, low, medium, high
    // api_style: AdaptiveEffort
    // default: high
    
    // anthropic-legacy: Opus 4.5, Sonnet 4.5, Haiku 4.5
    // levels: off, low, high
    // api_style: BudgetTokens
    // budget_map: low=1024, high=10000
    // default: high
    
    // openai-reasoning: GPT-5.x, Codex, o-series
    // levels: off, low, medium, high, xhigh
    // api_style: ReasoningEffort
    // default: medium
    
    // none: unknown models
    // levels: off
    // api_style: Disabled
    // default: off
}

fn default_model_mappings() -> Vec<ModelMapping> {
    // Exact matches (highest priority):
    // "claude-opus-4-6"     → anthropic-adaptive
    // "claude-sonnet-4-6"   → anthropic-adaptive-no-max
    
    // Prefix globs:
    // "claude-opus-4-5*"    → anthropic-legacy
    // "claude-sonnet-4-5*"  → anthropic-legacy
    // "claude-haiku-4-5*"   → anthropic-legacy
    // "gpt-5*"              → openai-reasoning
    // "codex-*"             → openai-reasoning
    // "o1*"                 → openai-reasoning
    // "o3*"                 → openai-reasoning
    
    // Wildcard fallbacks:
    // "claude-*"            → anthropic-adaptive-no-max
    // "gpt-*"              → openai-reasoning
}
```

### Layer 2: Runtime Rejection Cache (safety net)

When a thinking level returns a 400/422 from the provider:

1. `record_rejection(model_id, level)` — adds to session-scoped `HashSet`
2. `available_levels()` filters out rejected levels
3. Engine auto-downgrades to next-best available level
4. Optionally logs a warning for observability

**No disk persistence.** Restart clears the slate. This avoids staleness — if a provider re-enables a level or the user's account gets upgraded, they get access again on next restart.

### Layer 3: Graceful Degradation (always present)

If all levels are rejected or the model has no profile, fall back to `"off"`. The system always works — it just might not have thinking enabled.

---

## Integration Points

### 1. Replace `supported_thinking_levels()`

The existing function in `fx-llm/src/lib.rs` gets replaced:

```rust
// Before:
pub fn supported_thinking_levels(provider: &str) -> Vec<String> { ... }

// After:
// Deleted. Callers use ThinkingRegistry::available_levels(model_id) instead.
```

### 2. Provider completion code

Each provider's `complete()` method currently builds thinking parameters ad-hoc. Replace with:

```rust
let params = registry.translate(model_id, current_level);
match params {
    ThinkingParams::Disabled => { /* no thinking block */ },
    ThinkingParams::Anthropic { budget_tokens, effort } => { /* build Anthropic thinking block */ },
    ThinkingParams::OpenAi { reasoning_effort } => { /* set reasoning_effort field */ },
}
```

### 3. HeadlessApp

`thinking_available_levels()` currently calls `supported_thinking_levels(provider)`. Change to:

```rust
pub fn thinking_available_levels(&self) -> Vec<String> {
    self.thinking_registry.available_levels(&self.active_model)
}
```

### 4. GET /v1/thinking endpoint

No change needed — it already returns `ThinkingLevelDto` with the `available` field. The underlying data just comes from the registry now instead of the hardcoded function.

### 5. Config overrides (optional, can be follow-up PR)

```toml
# ~/.fawx/config.toml
[thinking.overrides.profiles.my-custom-profile]
levels = ["off", "low", "high"]
default = "low"
api_style = "AdaptiveEffort"

[thinking.overrides.models]
"my-custom-model" = "my-custom-profile"
```

---

## File Layout

All changes in `fx-llm`:

```
fx-llm/src/
├── lib.rs              ← remove supported_thinking_levels(), add registry re-export
├── thinking/
│   ├── mod.rs          ← ThinkingRegistry, public API
│   ├── profile.rs      ← ThinkingProfile, ApiStyle, LevelParams, ThinkingParams
│   ├── defaults.rs     ← default_profiles(), default_model_mappings()
│   ├── matching.rs     ← ModelMapping, MatchPriority, glob matching logic
│   └── rejection.rs    ← Session-scoped rejection cache
└── router.rs           ← update to use ThinkingRegistry
```

---

## Test Plan

### Unit Tests

1. **Profile lookup** — exact match wins over glob, glob wins over wildcard, unknown model gets "none" profile
2. **Available levels** — returns profile levels minus rejected ones
3. **Default level** — correct per profile
4. **Translate** — each ApiStyle produces correct ThinkingParams
5. **Rejection cache** — record + check, doesn't persist across new registry instances
6. **Config overrides** — override profile, override model mapping, override doesn't break defaults
7. **Glob matching** — `"claude-opus-4-5*"` matches `"claude-opus-4-5-20250101"`, doesn't match `"claude-opus-4-6"`
8. **Priority ordering** — exact > prefix > wildcard, verified with overlapping patterns

### Integration Tests

1. **End-to-end** — create registry with defaults, look up known model, translate level, verify params
2. **Unknown model** — returns "off" only, translate returns Disabled
3. **Rejection flow** — look up model, record rejection for "high", verify "high" removed from available, verify "low" still available

---

## Implementation Notes

- **Glob matching** is simple: if pattern ends with `*`, match the prefix. No full glob library needed.
- **The registry is immutable after construction** (except the rejection cache behind a Mutex). Thread-safe by design.
- **Model ID normalization**: strip provider prefix if present. `"anthropic/claude-opus-4-6"` → lookup as `"claude-opus-4-6"`. This way the registry works regardless of whether the caller passes the full `provider/model` path or just the model name.
- **The `ThinkingRegistry` lives on `ModelRouter`** (or alongside it). It's constructed once at startup, queried on every completion request.
- **Budget token values for AdaptiveEffort**: low=1024, medium=4096, high=10000, max=32000. These are Anthropic's current recommendations.
