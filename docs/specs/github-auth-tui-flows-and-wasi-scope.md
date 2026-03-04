# Spec: GitHub TUI Auth Flows + WASI Runtime Compatibility Scope

**Status:** Proposed
**Date:** 2026-03-04
**Owner:** Fawx Core
**Related:** GitHubSkill WASM integration (`skills/github-skill`), `fx-cli`, `fx-skills`, `fx-auth`

---

## 1) Problem Statement

We hit a runtime failure when loading `github-skill.wasm`:

- `failed to instantiate "github-skill": unknown import: wasi_snapshot_preview1::fd_write has not been defined`

This indicates a **WASI import mismatch**: the skill was built for `wasm32-wasip1`, but the current host runtime only links `host_api_v1` functions and does not provide `wasi_snapshot_preview1` imports.

At the same time, even once runtime compatibility is fixed, Fawx needs first-class TUI auth flows to securely accept/store GitHub credentials (PAT first), so users can run GitHubSkill without manual KV plumbing.

---

## 2) Goals

1. Add secure TUI auth commands for GitHub credentials.
2. Store credentials in secure platform storage (keychain/credential manager), not plaintext config.
3. Provide safe UX for token entry (masked input, no terminal echo, no secret logs).
4. Validate token and permissions on entry.
5. Scope and sequence WASI runtime compatibility fix after auth flow spec is in place.

---

## 3) Non-Goals (this PR/spec)

- Implementing full GitHub App OAuth device flow in this first pass.
- Implementing SSH key management in this first pass.
- Solving all future skill runtime ABI/versioning concerns.

---

## 4) Proposed User-Facing Commands

```text
/auth github set-token
/auth github show-status
/auth github clear-token
/auth list-providers
```

### `/auth github set-token`
- Prompt: `Enter GitHub Personal Access Token:`
- Input is masked (no plaintext echo).
- Validate token by calling GitHub API.
- Inspect scopes/permissions and show sufficiency for Fawx GitHub operations.
- Store securely if valid.

### `/auth github show-status`
- Show provider setup status only (configured/not configured, last validated time, permission summary).
- Never show token values.

### `/auth github clear-token`
- Remove stored credential from secure storage.
- Confirm action.
- Audit-log event.

### `/auth list-providers`
- Show supported providers and auth methods:
  - GitHub PAT (Phase 1)
  - GitHub App (planned)
  - SSH key path integration (planned)

---

## 5) Security Requirements

### 5.1 Masked Input
- Token entry must disable terminal echo and render masked placeholders only (or nothing).
- No token in stdout/stderr logs.

### 5.2 Encrypted Storage
- Primary: system secure storage
  - macOS Keychain
  - Windows Credential Manager
  - Linux Secret Service
- Fallback: encrypted file using existing Fawx encrypted credential primitives (`fx-auth`) if secure storage is unavailable.

### 5.3 Memory Protection
- Keep plaintext token lifetime minimal.
- Use the [`zeroize`](https://crates.io/crates/zeroize) crate to clear temporary buffers containing secrets after use (e.g., `Zeroizing<String>` wrapper for token values).
- Avoid cloning token strings unnecessarily.

### 5.4 Token Expiration Handling
- On each use, check the GitHub API response for `401 Unauthorized` indicating an expired or revoked token.
- When detected, surface a clear TUI message: `⚠ GitHub token expired or revoked. Run /auth github set-token to re-authenticate.`
- Do not silently retry with a stale token. Fail the operation and prompt for re-auth.
- Store `last_validated` timestamp in credential metadata; optionally warn if token hasn't been validated in >30 days.

### 5.5 Audit Logging
- Emit security/audit events for set/validate/use/clear actions.
- Never include raw token value, token prefix, or secret-derived hashes in logs.

---

## 6) WASI Issue Scope (Root Cause + Fix Options)

### Observed Root Cause
- `github-skill.wasm` imports `wasi_snapshot_preview1::fd_write`.
- `fx-skills::runtime` currently links only `host_api_v1` imports.
- No WASI context/linker configured in runtime.

### Option A (recommended): Host runtime supports WASI preview1
- Add WASI context (`WasiCtx`) to runtime state.
- Link `wasi_snapshot_preview1` imports via wasmtime WASI support.
- Keep `wasm32-wasip1` target for skills.

### Option B: Rebuild skills for no-WASI target (`wasm32-unknown-unknown`)
- Requires ensuring skill code + deps do not require WASI imports.
- Can be fragile across crates/dependencies and lose useful std/WASI behavior.

### Decision
- Use **Option A** for robust compatibility with current skill output and future third-party skills.

---

## 7) Architecture

### 7.1 New Auth Service Boundary
Create an auth abstraction in `fx-auth` (or new crate if needed) with provider/method model:

- `AuthProvider::GitHub`
- `AuthMethod::Pat`
- `CredentialStore` trait:
  - `set(provider, method, secret)`
  - `get(provider, method)`
  - `clear(provider, method)`
  - `status(provider)`

Backends:
- `SystemKeychainStore` (primary)
- `EncryptedFileStore` (fallback)

### 7.2 TUI Command Wiring
`fx-cli` command parser adds `/auth ...` command tree, invoking auth service:

- `set-token` → masked prompt → validate → store
- `show-status` → non-secret status
- `clear-token` → secure delete
- `list-providers` → capability table

### 7.3 Skill Integration
GitHubSkill token retrieval path should resolve through host KV adapter backed by auth service:

- Keep skill expecting `github_token` in host KV for backward compatibility.
- Host implementation maps `kv_get("github_token")` to auth service retrieval.
- This avoids putting tokens in plaintext config.

### 7.4 WASI Runtime Integration
In `fx-skills::runtime`:

- Add WASI context to store state.
- Configure linker for `wasi_snapshot_preview1` imports.
- Preserve existing `host_api_v1` imports and capability checks.

---

## 8) UX Flow (Phase 1)

```text
> /auth github set-token
Enter GitHub Personal Access Token: [masked input]
✓ Token validated and stored securely
✓ Permissions: repo, workflow (sufficient for Fawx operations)
```

Validation failure example:

```text
✗ Token invalid or insufficient permissions
Required: repo, workflow
Detected: repo
Use /auth github set-token to try again
```

---

## 9) Delivery Plan

### PR A (this spec)
- Land design + DoD + sequencing.

### PR B (Auth flows)
- Implement `/auth` commands, secure store abstraction, PAT validation, audit events.
- No WASI runtime changes yet.

### PR C (WASI runtime compatibility)
- Add WASI linking support in `fx-skills` runtime.
- Validate `github-skill.wasm` instantiates and executes.

### PR D (Integration polish)
- Wire auth-backed KV bridge, docs, migration guidance.

---

## 10) Definition of Done

Checkboxes are scoped to the PR that delivers them (see Section 9 Delivery Plan).

### PR B — Auth Flows
- [ ] `/auth github set-token` exists and token entry is masked.
- [ ] Token value is never echoed to terminal or logs.
- [ ] `/auth github show-status` reports safe metadata only.
- [ ] `/auth github clear-token` securely removes credential.
- [ ] `/auth list-providers` lists implemented and planned providers/methods.
- [ ] Credential storage uses system secure store when available.
- [ ] Fallback encrypted storage is used only when secure store unavailable.
- [ ] Audit logs emitted for set/validate/use/clear without secret leakage.
- [ ] Token validation checks required permissions (`repo`, `workflow`).
- [ ] `/auth` commands accepting secrets are TTY-gated (see Open Question #3 resolution).
- [ ] Regression tests for masked input behavior and no-secret logging.
- [ ] Unit tests for credential store backends and fallback behavior.
- [ ] Integration tests for auth command flows.
- [ ] `cargo fmt --all`, `cargo clippy --all-targets -D warnings`, and workspace tests pass.

### PR C — WASI Runtime Compatibility
- [ ] `github-skill.wasm` loads without `wasi_snapshot_preview1::fd_write` import errors.
- [ ] WASM runtime supports required WASI imports while preserving `host_api_v1` behavior.
- [ ] Integration test proving WASM skill instantiation succeeds with WASI imports.
- [ ] `cargo fmt --all`, `cargo clippy --all-targets -D warnings`, and workspace tests pass.

### PR D — Integration Polish
- [ ] GitHubSkill can perform a validated API operation end-to-end after auth setup.
- [ ] Auth-backed KV bridge wired (`kv_get("github_token")` → auth service).
- [ ] Expired/revoked token detection surfaces re-auth prompt (see Section 5.4).
- [ ] `cargo fmt --all`, `cargo clippy --all-targets -D warnings`, and workspace tests pass.

---

## 11) Risks / Mitigations

- **Risk:** platform keychain APIs differ and may fail in headless envs.
  - **Mitigation:** explicit backend detection + encrypted fallback + clear operator messaging.

- **Risk:** logging paths accidentally include secrets.
  - **Mitigation:** centralized redaction helpers + tests asserting no token leakage.

- **Risk:** WASI linking broadens runtime surface area.
  - **Mitigation:** keep capability checks in host API; do not grant FS/network WASI rights by default.

---

## 12) Open Questions

1. Should `workflow` permission be hard-required for initial create/comment PR scope, or optional with warning?
2. Should auth status track per-repo permission checks or global token scopes only?
3. ~~Do we gate `/auth` commands behind interactive TTY only?~~ **Resolved:** Yes. `/auth` commands that accept secrets (e.g., `set-token`) require an interactive TTY — masked input depends on terminal echo control, and non-interactive contexts cannot guarantee secret safety. Commands that only read status (`show-status`, `list-providers`) may work outside a TTY.
