# Spec: API Sprint 2 — Model Switching, Thinking Toggle, Auth & Skills

**Status**: Ready for implementation
**Crates touched**: `fx-api` (new handlers + router extension + AppEngine trait extension), `fx-cli` (AppEngine implementation)
**Estimated scope**: ~350 lines production code + ~200 lines tests
**Depends on**: PR #1393 (Sprint 1 — session endpoints) — merged ✅

---

## Problem

A Swift GUI client needs to switch models, toggle thinking mode, view
available models, list skills, and see auth/credential status — all
without a terminal. These are the remaining Phase 1 "daily driver"
gaps per the API audit (`docs/specs/gui-api-audit.md`).

---

## Endpoints

### 1. `GET /v1/models` — List available models

Returns all models registered in the `ModelRouter`, grouped by provider.

Response (200):
```json
{
    "active_model": "claude-sonnet-4-20250514",
    "models": [
        {
            "model_id": "claude-sonnet-4-20250514",
            "provider": "anthropic",
            "auth_method": "api_key"
        },
        {
            "model_id": "gpt-4o",
            "provider": "openai",
            "auth_method": "api_key"
        }
    ]
}
```

Implementation:
- Call `AppEngine::available_models()` (new trait method)
- Call `AppEngine::active_model()` for the current selection
- Serialize `Vec<ModelInfo>` → JSON array

### 2. `PUT /v1/model` — Switch active model

Request:
```json
{
    "model": "gpt-4o"
}
```

Response (200):
```json
{
    "previous_model": "claude-sonnet-4-20250514",
    "active_model": "gpt-4o"
}
```

Response (400) — model not found:
```json
{
    "error": "model not found: gpt-99"
}
```

Implementation:
- Record current `active_model()` as `previous`
- Call `AppEngine::set_active_model(selector)` (new trait method)
- Resolves aliases via existing `resolve_headless_model_selector()` logic
- Persists to config (same as `/model` slash command)
- Return previous + new model

### 3. `GET /v1/thinking` — Get current thinking mode

Response (200):
```json
{
    "level": "adaptive",
    "budget_tokens": 5000
}
```

### 4. `PUT /v1/thinking` — Set thinking mode

Request:
```json
{
    "level": "high"
}
```

Valid levels: `adaptive`, `high`, `low`, `off`

Response (200):
```json
{
    "previous_level": "adaptive",
    "level": "high",
    "budget_tokens": 10000
}
```

Response (400) — invalid level:
```json
{
    "error": "unknown thinking budget 'turbo'; expected adaptive, high, low, or off"
}
```

Implementation:
- Call `AppEngine::thinking_level()` (new trait method) for current
- Call `AppEngine::set_thinking_level(level)` (new trait method)
- Under the hood calls existing `apply_thinking_budget()` logic
- Persists to config (same as `/thinking` slash command)

### 5. `GET /v1/skills` — List loaded skills

Response (200):
```json
{
    "skills": [
        {
            "name": "brave-search",
            "tools": ["brave_search"]
        },
        {
            "name": "journal",
            "tools": ["journal_write", "journal_search"]
        }
    ],
    "total": 2
}
```

Implementation:
- Call `AppEngine::skill_summaries()` (new trait method)
- Returns `Vec<(String, Vec<String>)>` from `SkillRegistry::skill_summaries()`

### 6. `GET /v1/auth` — List configured providers (read-only)

Returns a redacted overview of which providers have credentials configured.
**No write operations** — credential management goes through `fawx setup`.

Response (200):
```json
{
    "providers": [
        {
            "provider": "anthropic",
            "auth_methods": ["api_key"],
            "model_count": 12,
            "status": "configured"
        },
        {
            "provider": "openai",
            "auth_methods": ["api_key"],
            "model_count": 8,
            "status": "configured"
        }
    ]
}
```

Implementation:
- Call `AppEngine::auth_provider_statuses()` (new trait method)
- Reuses existing `auth_provider_statuses()` logic from headless.rs
- Read-only; no credential writes via HTTP (encrypted store requires interactive setup)

---

## AppEngine Trait Extensions

Add these methods to `fx-api/src/engine.rs`:

```rust
#[async_trait]
pub trait AppEngine: Send + Sync {
    // ... existing methods ...

    /// List all available models from the router.
    fn available_models(&self) -> Vec<ModelInfoDto>;

    /// Switch the active model. Returns the resolved model ID.
    fn set_active_model(&mut self, selector: &str) -> Result<String, anyhow::Error>;

    /// Get the current thinking budget level.
    fn thinking_level(&self) -> ThinkingLevelDto;

    /// Set the thinking budget. Returns the resolved level string.
    fn set_thinking_level(&mut self, level: &str) -> Result<ThinkingLevelDto, anyhow::Error>;

    /// List loaded skills with their tool names.
    fn skill_summaries(&self) -> Vec<SkillSummaryDto>;

    /// List auth provider statuses (redacted, read-only).
    fn auth_provider_statuses(&self) -> Vec<AuthProviderDto>;
}
```

### DTO types (in `fx-api/src/types.rs`)

```rust
#[derive(Debug, Clone, Serialize)]
pub struct ModelInfoDto {
    pub model_id: String,
    pub provider: String,
    pub auth_method: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ThinkingLevelDto {
    pub level: String,
    pub budget_tokens: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SkillSummaryDto {
    pub name: String,
    pub tools: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AuthProviderDto {
    pub provider: String,
    pub auth_methods: Vec<String>,
    pub model_count: usize,
    pub status: String,
}
```

These DTOs keep API serialization concerns in `fx-api`, not in the
engine crates. The `HeadlessApp` implementation converts from the
engine's native types.

---

## HeadlessApp Implementation (`fx-cli/src/headless.rs` — in `tests.rs` AppEngine impl)

The `AppEngine` impl for `HeadlessApp` already lives in
`fx-api/src/tests.rs`. The new trait methods delegate to existing
`HeadlessApp` methods:

```rust
fn available_models(&self) -> Vec<ModelInfoDto> {
    self.router.available_models()
        .into_iter()
        .map(|m| ModelInfoDto {
            model_id: m.model_id,
            provider: m.provider_name,
            auth_method: m.auth_method,
        })
        .collect()
}

fn set_active_model(&mut self, selector: &str) -> Result<String, anyhow::Error> {
    HeadlessApp::set_active_model(self, selector)
}

fn thinking_level(&self) -> ThinkingLevelDto {
    let budget = self.config.general.thinking.unwrap_or_default();
    ThinkingLevelDto {
        level: budget.to_string(),
        budget_tokens: budget.budget_tokens(),
    }
}

fn set_thinking_level(&mut self, level: &str) -> Result<ThinkingLevelDto, anyhow::Error> {
    HeadlessApp::handle_thinking(self, Some(level))?;
    Ok(self.thinking_level())
}

fn skill_summaries(&self) -> Vec<SkillSummaryDto> {
    self.loop_engine.skill_registry()
        .skill_summaries()
        .into_iter()
        .map(|(name, tools)| SkillSummaryDto { name, tools })
        .collect()
}

fn auth_provider_statuses(&self) -> Vec<AuthProviderDto> {
    let statuses = auth_provider_statuses(self.router.available_models());
    statuses.into_iter().map(|s| AuthProviderDto {
        provider: s.provider,
        auth_methods: s.auth_methods.into_iter().collect(),
        model_count: s.model_count,
        status: "configured".to_string(),
    }).collect()
}
```

### Exposing `HeadlessApp` internals

The methods `set_active_model`, `handle_thinking`, and `auth_provider_statuses`
are currently `fn` (non-pub or pub(crate)) on `HeadlessApp`. They need to
be accessible from the `AppEngine` impl in `tests.rs`.

**Key constraint**: The `AppEngine` impl for `HeadlessApp` lives in
`fx-api/src/tests.rs`, which can access `pub` methods on `HeadlessApp`
(since `fx-api` depends on `fx-cli` in `[dev-dependencies]`). But
`fx-api` production code cannot depend on `fx-cli` (it's the other way
around — `fx-cli` depends on `fx-api`).

**Solution**: The new `AppEngine` trait methods are defined in `fx-api`
with generic return types (the DTOs). The `HeadlessApp` impl in
`fx-api/src/tests.rs` delegates to `HeadlessApp`'s existing pub methods.
The handlers only depend on the `AppEngine` trait.

For production: the `HeadlessApp` implements the full `AppEngine` trait
in `fx-api/src/tests.rs` (same file where the existing impl block is).
This is the same pattern used for `process_message` and
`process_message_with_context`. The test file IS the integration
boundary.

### Required visibility changes in `fx-cli/src/headless.rs`

1. `set_active_model` — already `fn` on HeadlessApp, needs to be `pub fn`
2. `handle_thinking` — already `fn` on HeadlessApp, needs to be `pub fn`
3. `auth_provider_statuses` — currently a free function, needs to be `pub(crate)` → `pub`
4. `AuthProviderStatus` — currently private struct, needs `pub` + `Serialize`
5. `SkillRegistry` access — `HeadlessApp` needs a `pub fn skill_summaries()` that delegates to its `LoopEngine`'s registry. Check if `LoopEngine` exposes the registry; if not, add a `pub fn skill_registry(&self) -> &SkillRegistry` on `LoopEngine`.

---

## Route Wiring (`fx-api/src/router.rs`)

Extend the existing `v1_router`:

```rust
let v1_router = Router::new()
    // existing session routes...
    .route("/sessions", post(handle_create_session).get(handle_list_sessions))
    .route("/sessions/{id}", get(handle_get_session).delete(handle_delete_session))
    .route("/sessions/{id}/clear", post(handle_clear_session))
    .route("/sessions/{id}/messages", get(handle_get_messages).post(handle_send_message))
    // NEW Sprint 2 routes
    .route("/models", get(handle_list_models))
    .route("/model", put(handle_set_model))
    .route("/thinking", get(handle_get_thinking).put(handle_set_thinking))
    .route("/skills", get(handle_list_skills))
    .route("/auth", get(handle_list_auth));
```

Import `axum::routing::put` (not yet imported in router.rs).

---

## Handler Implementations (`fx-api/src/handlers/settings.rs` — NEW file)

All 6 handlers in one new file. Follows existing handler patterns
(same `State<HttpState>` extraction, same `ErrorBody` for errors).

```rust
// GET /v1/models
pub async fn handle_list_models(State(state): State<HttpState>) -> Json<Value> {
    let app = state.app.lock().await;
    let models = app.available_models();
    let active = app.active_model().to_string();
    Json(json!({ "active_model": active, "models": models }))
}

// PUT /v1/model
pub async fn handle_set_model(
    State(state): State<HttpState>,
    Json(req): Json<SetModelRequest>,
) -> Result<Json<Value>, (StatusCode, Json<ErrorBody>)> {
    let mut app = state.app.lock().await;
    let previous = app.active_model().to_string();
    let resolved = app.set_active_model(&req.model).map_err(|e| {
        (StatusCode::BAD_REQUEST, Json(ErrorBody { error: e.to_string() }))
    })?;
    Ok(Json(json!({ "previous_model": previous, "active_model": resolved })))
}

// GET /v1/thinking
pub async fn handle_get_thinking(State(state): State<HttpState>) -> Json<Value> {
    let app = state.app.lock().await;
    let level = app.thinking_level();
    Json(json!(level))
}

// PUT /v1/thinking
pub async fn handle_set_thinking(
    State(state): State<HttpState>,
    Json(req): Json<SetThinkingRequest>,
) -> Result<Json<Value>, (StatusCode, Json<ErrorBody>)> {
    let mut app = state.app.lock().await;
    let previous = app.thinking_level();
    let updated = app.set_thinking_level(&req.level).map_err(|e| {
        (StatusCode::BAD_REQUEST, Json(ErrorBody { error: e.to_string() }))
    })?;
    Ok(Json(json!({
        "previous_level": previous.level,
        "level": updated.level,
        "budget_tokens": updated.budget_tokens,
    })))
}

// GET /v1/skills
pub async fn handle_list_skills(State(state): State<HttpState>) -> Json<Value> {
    let app = state.app.lock().await;
    let skills = app.skill_summaries();
    let total = skills.len();
    Json(json!({ "skills": skills, "total": total }))
}

// GET /v1/auth
pub async fn handle_list_auth(State(state): State<HttpState>) -> Json<Value> {
    let app = state.app.lock().await;
    let providers = app.auth_provider_statuses();
    Json(json!({ "providers": providers }))
}
```

### Request types (in `fx-api/src/types.rs`)

```rust
#[derive(Debug, Deserialize)]
pub struct SetModelRequest {
    pub model: String,
}

#[derive(Debug, Deserialize)]
pub struct SetThinkingRequest {
    pub level: String,
}
```

---

## Edge Cases (MUST handle)

### Model not found
`PUT /v1/model` with unknown model → 400 with error message from
`resolve_headless_model_selector`.

### Ambiguous model selector
`PUT /v1/model` with ambiguous prefix → 400 with "ambiguous model
selector: {prefix}".

### Empty model selector
`PUT /v1/model` with empty string → 400.

### Invalid thinking level
`PUT /v1/thinking` with level other than adaptive/high/low/off → 400
with the parse error message from `ThinkingBudget::from_str`.

### No skills loaded
`GET /v1/skills` returns `{ "skills": [], "total": 0 }`.

### No credentials configured
`GET /v1/auth` returns `{ "providers": [] }`.

---

## Testing

### Unit tests in `fx-api/src/tests.rs` (extend existing test module)

Tests use the existing `MockAppEngine` pattern or extend it. Since the
new trait methods are simple getters/setters, mock implementations are
trivial.

1. **`list_models_returns_active_and_catalog`** — GET /v1/models returns
   `active_model` string + `models` array. Mock returns 2 models.

2. **`set_model_switches_and_returns_previous`** — PUT /v1/model with
   valid model → 200 with previous + new model.

3. **`set_model_invalid_returns_400`** — PUT /v1/model with "nonexistent"
   → 400.

4. **`get_thinking_returns_current_level`** — GET /v1/thinking returns
   level + budget_tokens.

5. **`set_thinking_valid_level_returns_200`** — PUT /v1/thinking with
   "high" → 200 with updated level.

6. **`set_thinking_invalid_level_returns_400`** — PUT /v1/thinking with
   "turbo" → 400.

7. **`list_skills_returns_summaries`** — GET /v1/skills returns skills
   array with names and tools.

8. **`list_skills_empty_returns_zero`** — GET /v1/skills when no skills
   loaded → `{ "skills": [], "total": 0 }`.

9. **`list_auth_returns_provider_statuses`** — GET /v1/auth returns
   providers array with redacted info.

10. **`sprint2_endpoints_require_auth`** — All 6 endpoints return 401
    without bearer token.

---

## File Changes Summary

| File | Change |
|------|--------|
| `fx-api/src/handlers/settings.rs` | **NEW** — 6 handlers (models, model, thinking×2, skills, auth) |
| `fx-api/src/handlers/mod.rs` | Add `pub(crate) mod settings;` |
| `fx-api/src/types.rs` | Add `SetModelRequest`, `SetThinkingRequest`, `ModelInfoDto`, `ThinkingLevelDto`, `SkillSummaryDto`, `AuthProviderDto` |
| `fx-api/src/engine.rs` | Add 6 new trait methods to `AppEngine` |
| `fx-api/src/router.rs` | Add 5 new routes under `/v1/`, import `put` |
| `fx-api/src/tests.rs` | Add 10 tests, extend `AppEngine` impl for `HeadlessApp` with new methods |
| `fx-cli/src/headless.rs` | Make `set_active_model`, `handle_thinking`, `auth_provider_statuses`, `AuthProviderStatus` pub |

---

## What NOT to build in Sprint 2

- **Credential writes** (`POST /auth/token`, `DELETE /auth/token`) — credential store is encrypted, requires interactive `fawx setup`. Read-only auth status is sufficient for the GUI.
- **Dynamic model catalog** (`fetch_available_models`) — uses the static router registry. Live fetching from providers is a follow-up.
- **Skill install/remove/search** — Phase 2 skill management endpoints.
- **WebSocket** — SSE is sufficient.

---

## Route Summary (after Sprint 2)

```
# Sprint 1 (existing)
POST   /v1/sessions                → create session
GET    /v1/sessions                → list sessions
GET    /v1/sessions/{id}           → get session info
DELETE /v1/sessions/{id}           → delete session
POST   /v1/sessions/{id}/clear     → clear history
GET    /v1/sessions/{id}/messages  → get history
POST   /v1/sessions/{id}/messages  → send message (SSE)

# Sprint 2 (new)
GET    /v1/models                  → list available models
PUT    /v1/model                   → switch active model
GET    /v1/thinking                → get thinking level
PUT    /v1/thinking                → set thinking level
GET    /v1/skills                  → list loaded skills
GET    /v1/auth                    → list auth provider statuses
```

Total: 13 versioned endpoints + 8 legacy endpoints = **21 HTTP endpoints**.
