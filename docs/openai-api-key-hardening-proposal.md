# OpenAI API Key Hardening and Reliability Proposal (Android)

Status: Proposal  
Owner: Citros Android  
Last updated: 2026-02-11

## 1. Context

Citros Android currently supports OpenAI API key and OAuth-based sign-in paths. Recent device testing showed a common failure mode:

- OAuth login can succeed
- Token persists locally
- API calls fail with `401` due to missing `model.request` scope

Given current platform constraints, the most reliable path for production usage is OpenAI API keys. This proposal scopes improvements to make API-key usage secure, clear, and operable on-device.

## 2. Goals

1. Secure OpenAI credentials at rest using Android hardware-backed primitives where available.
2. Make key setup safer with clear guidance for restricted keys and project scoping.
3. Improve UX for OAuth permission failures with actionable error text.
4. Add an in-app credential health check that validates configuration before first prompt.

## 3. Non-Goals

1. No server-side relay/proxy requirement.
2. No inbound phone ports or background bridge service exposed to the internet.
3. No attempt to bypass OpenAI OAuth scope policy.
4. No migration to a new backend provider model in this proposal.

## 4. Proposal Summary

### Track A: Keystore-backed credential protection

Implement encrypted local credential storage for OpenAI keys/tokens.

- Add `CredentialVault` abstraction in Android app layer.
- Generate per-install AES key via Android Keystore (`KeyGenParameterSpec`, AES/GCM, `PURPOSE_ENCRYPT|DECRYPT`).
- Store encrypted blob + IV + metadata in SharedPreferences (or DataStore).
- Keep plaintext key only in memory for active session, never logged.
- Add migration:
  - Read legacy `cloud_token` plaintext once.
  - Encrypt into vault format.
  - Delete plaintext key from prefs after successful write.
- Fallback behavior:
  - If hardware-backed unavailable, still use Keystore-managed software key.

Deliverables:

- `android/chat/.../security/CredentialVault.kt` (or equivalent)
- Migration logic integrated in startup/auth restoration path
- Tests for encryption/decryption + migration idempotency

### Track B: Safer key setup UX + docs

Add explicit guidance in-app and docs for API key best practices.

- Update auth/setup copy:
  - Recommend per-project key
  - Recommend restricted key where supported
  - Clarify that ChatGPT subscription login != API authorization
- Add docs page (or section in existing docs):
  - Key creation
  - Suggested restrictions
  - Rotation plan
  - Revocation flow
- Add warning if pasted value appears to be OAuth JWT/session token when `OpenAI API Key` mode is selected.

Deliverables:

- New doc: `docs/openai-api-key-setup.md`
- Updated strings in `ChatActivity` auth UI

### Track C: OAuth failure UX mapping

Strengthen error mapping for common OpenAI auth failures in UI.

- Keep raw provider error internally, but surface user-focused messages.
- Canonical mapped cases:
  - Missing `model.request` scope
  - Invalid/expired credential
  - Project/org permission mismatch
  - Network timeout/offline
- Include “What to do next” per case.

Deliverables:

- Error mapping helper (`core` or `chat` layer)
- UI copy updates in chat error card and auth screens
- Unit tests for mapping rules

### Track D: Credential health check screen

Add a preflight check accessible from auth/setup.

- New UI action: `Test OpenAI Connection`
- Runs checks and returns structured status:
  1. Credential parse sanity (format class)
  2. Network reachability to OpenAI API
  3. Auth check (`401/403` handling)
  4. Model access check for configured chat/action models (non-generative endpoint)
- Show actionable result states:
  - `Healthy`
  - `Auth failed`
  - `Missing model permission`
  - `Network error`
  - `Unknown provider error`

Suggested endpoint strategy:

- Use lightweight model metadata check (`GET /v1/models/{id}`) for configured model IDs.
- Avoid billable generation call for health preflight.

Deliverables:

- `OpenAiHealthChecker` service
- UI panel/dialog in auth flow
- Unit tests with `MockWebServer`

## 5. Architecture and Data Changes

### 5.1 Storage format

Add versioned encrypted credential payload:

- `credential_vault_version`
- `credential_ciphertext_b64`
- `credential_iv_b64`
- `credential_provider`
- `credential_auth_kind`

Legacy keys (`cloud_token`) retained read-only for migration window, then removed.

### 5.2 New components

- `CredentialVault`
  - `saveCredential(provider, authKind, plaintext)`
  - `loadCredential(): Credential?`
  - `clearCredential()`
- `OpenAiHealthChecker`
  - `runCheck(credential, modelIds): HealthCheckResult`
- `OpenAiErrorMapper`
  - Converts raw API errors to user-facing remediation text

### 5.3 Existing touch points

- `android/chat/src/main/kotlin/ai/citros/chat/ChatActivity.kt`
- `android/chat/src/main/kotlin/ai/citros/chat/ChatViewModel.kt`
- `android/core/src/main/kotlin/ai/citros/core/BaseProviderClient.kt`
- Shared prefs auth keys + restore path

## 6. Implementation Plan

Phase 1: Storage hardening + migration

- Add vault abstraction
- Add migration from plaintext prefs
- Add tests and regression checks

Phase 2: UX/docs hardening

- Update setup copy and warnings
- Publish key setup and rotation docs
- Add mapped remediation text

Phase 3: Health check feature

- Build checker service and UI
- Add endpoint mocking tests
- Validate on physical device

## 7. Testing Strategy

1. Unit tests
- Vault encryption/decryption
- Migration from legacy plaintext
- Error mapping classification
- Health checker response handling

2. Integration tests
- `MockWebServer` for OpenAI error/status variants
- Model permission denied vs auth denied vs timeout

3. Manual/device validation
- Fresh install key setup
- Upgrade install migration
- Sign-out clears encrypted data
- Health check behavior in offline mode

## 8. Acceptance Criteria

1. No plaintext API key remains in SharedPreferences after migration.
2. User sees actionable guidance for missing `model.request`/permission failures.
3. Health check can distinguish at least: auth failure, network failure, model permission failure, healthy.
4. Existing chat send flow continues to work with valid API keys.
5. Unit/integration tests pass for new code paths.

## 9. Risks and Mitigations

1. Keystore edge cases on OEM devices
- Mitigation: robust fallback + explicit recovery path

2. Migration failures causing forced re-login
- Mitigation: transactional migration and safe rollback behavior

3. Endpoint behavior differences for model checks
- Mitigation: classify unknown responses conservatively and show raw support code in diagnostics

## 10. Open Questions

1. Should health check run automatically on every app start, or only on demand?
2. Should we gate chat send if health check fails hard, or allow best-effort send?
3. Do we want optional biometric gate before decrypting stored credential?
4. Should we migrate from SharedPreferences to Encrypted DataStore in a later iteration?

## 11. Effort Estimate

- Phase 1: 1.5-2.5 days
- Phase 2: 0.5-1 day
- Phase 3: 1-2 days

Total: ~3-5.5 engineering days excluding design review and QA signoff.
