# Provider Model Catalog Sync (OpenAI / OpenRouter / Anthropic)

## Purpose
Keep in-app model options aligned with provider reality without shipping an app release for every model launch/deprecation.

## Runtime policy
Model options are resolved in this order:
1. Live provider catalog (`ModelCatalog.getModels(config)`) when available
2. Cached provider catalog (`ModelCatalog.getCachedModels(provider)`) if live fetch is unavailable
3. Static curated fallback (`ModelConfig.chatModelsForProvider/actionModelsForProvider`)

Wallet/backend build intentionally does **not** await catalog refresh. The app resolves immediately from
cached/static models for fast startup, while refresh runs asynchronously and updates subsequent UI state.

## Safety guardrails
- Chat models prioritize curated IDs first, then append runtime catalog IDs with provider-specific caps (12 direct providers, 24 OpenRouter)
- Action models must pass model floor (`ModelConfig.isModelAboveFloor(...)`)
- If saved chat/action model is unavailable, app falls back to provider default (or first available) and logs adjustment
- Unknown/unsupported IDs are validated against runtime-known models with fallback suggestions
- Action floor is checked both when building runtime action lists and again when validating saved selections (defense-in-depth)

## Known limits
- Catalog fetch depends on current API key entitlement and network availability
- Some providers may expose models that are technically listed but not practically usable for tool-heavy loops
- Action-model floor filtering may hide catalog models that are chat-capable but below security threshold

## UX behavior
- Settings and quick switcher consume runtime-resolved model lists (cache-backed)
- On catalog refresh, options update without app restart
- If a selected model disappears, selection auto-corrects to safe fallback

## Future enhancements
- Surface model-source badge in UI (`live`, `cached`, `fallback`)
- Explicit user-facing warning banner when fallback path is active
- Persist per-provider known-good model telemetry for diagnostics
