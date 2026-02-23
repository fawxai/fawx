# Tier 3 Pressure Test: WalletManager / Key Auth Management

**Date:** 2026-02-16
**Audit scope:** Key storage, credential lifecycle, thread safety, backup exposure, migration
**Reference:** OpenClaw `auth-profiles-BWqNRMgG.js` (auth-profiles store, file-lock, json-file), `auth-CjzSe-Vc.js` (rate limiting, timing-safe comparison)
**Citros files:** `WalletManager.kt`, `WalletStorage.kt`, `KeyStore.kt`, `WalletKey.kt`, `WalletState.kt`, `Provider.kt`, `SharedPreferencesWalletStorage.kt`, `EncryptedKeyStore.kt`, `SettingsScreen.kt` (key masking/health), `network_security_config.xml`

---

## 1. Architecture Comparison

### OpenClaw Auth Model
- **Storage:** `auth-profiles.json` — plaintext JSON on disk
- **File permissions:** Directory `0700`, files `0600` (owner-only via `fs.mkdirSync({mode:448})` + `fs.chmodSync(384)`)
- **Encryption at rest:** None — relies on OS file permissions
- **File locking:** `acquireFileLock()` — `.lock` file with PID + timestamp, stale detection (`isAlive(pid)` + age check), retry with exponential backoff (10 retries, 100ms-10s delays, randomized jitter)
- **Mutation pattern:** `updateAuthProfileStoreWithLock()` wraps all writes in `withFileLock()` → atomic read-modify-write
- **Legacy migration:** Reads old `auth.json` → migrates to `auth-profiles.json` → **deletes** legacy file → idempotent (checks `profiles` key existence)
- **Auth rate limiting:** Sliding-window rate limiter (`createAuthRateLimiter`) — per-scope (shared-secret vs device-token), per-IP, 10 attempts / 60s window / 5-minute lockout, loopback exempt
- **Token comparison:** `timingSafeEqual` via Node.js `crypto` — constant-time comparison prevents timing attacks
- **Config backup:** `rotateConfigBackups()` keeps last 5 numbered `.bak.{1-5}` files
- **Profile types:** `api_key` (raw key), `oauth` (access+refresh+expires), `token` (bearer token)
- **Profile ordering:** Explicit `order` record per provider — deterministic priority
- **Usage tracking:** `usageStats` per profile — tracks last-used, success count
- **Last-known-good:** `lastGood` record — can fall back to last working profile

### Citros WalletManager Model
- **Storage split:**
  - `WalletStorage` interface (`loadState()`/`saveState()`) → `SharedPreferencesWalletStorage` (Android SharedPrefs + kotlinx.serialization JSON)
  - `KeyStore` interface (`put()`/`get()`/`delete()`) → `EncryptedKeyStore` (Android EncryptedSharedPreferences, backed by Android Keystore)
- **Encryption at rest:** ✅ API keys encrypted via Android Keystore (hardware-backed on most devices) — **BETTER than OpenClaw**
- **Locking:** ❌ None — source comment: "NOT thread-safe"
- **Mutation pattern:** In-memory state modified, then separate `storage.saveState()` + `keyStore.put()` calls — NOT atomic
- **Legacy migration:** `migrateFromLegacy()` — source comment: "NOT idempotent"
- **Rate limiting:** ❌ None
- **Token comparison:** N/A (phone agent doesn't accept inbound auth)
- **Config backup:** ❌ None
- **Key types:** `WalletKey.Type` enum: `ANTHROPIC`, `OPENAI`, `OPENROUTER`, `BRAVE_SEARCH`
- **Active selection:** Single `activeKeyId` in `WalletState` + per-use `chatModel`/`actionModel` strings
- **Usage tracking:** ❌ None
- **Last-known-good:** ❌ None

---

## 2. Findings

### CRITICAL — Must fix before more H2 features

#### C1: Non-atomic key operations → orphaned credentials
**What:** `addKey()` does `keyStore.put(id, apiKey)` then `storage.saveState(state)` as separate calls. If `saveState()` fails (disk full, serialization error, app killed between calls), the API key exists in `EncryptedKeyStore` but no `WalletKey` entry references it. The key is unreachable but persists forever.

**OpenClaw comparison:** `updateAuthProfileStoreWithLock()` wraps the entire mutation in a file lock. The store file is the single source of truth (keys stored inline, not in a separate store).

**Impact:** Key leakage (orphaned encrypted keys), impossible-to-debug "ghost" entries.

**Fix:** Two options:
1. **Transaction pattern:** Write state first (with key ID), then store key. On next `loadOrDefault()`, detect keys referenced in state but missing from KeyStore and clean them up. Reverse orphan (missing key) is recoverable; forward orphan (missing state entry) is not.
2. **Unified write:** Store key material inline in WalletState (encrypted at the WalletStorage layer). Eliminates two-phase write entirely. More invasive but fundamentally sound.

**Recommendation:** Option 1 (transaction + cleanup) — less invasive, addresses the immediate bug.

#### C2: Thread safety — concurrent mutations corrupt state
**What:** `WalletManager` has no synchronization. Multiple coroutines calling `addKey()`/`removeKey()`/`setActiveKey()` concurrently can interleave reads and writes, producing lost updates or inconsistent state.

**OpenClaw comparison:** File locking with `acquireFileLock()` — PID-based stale detection, retry with exponential backoff. All mutations go through `withFileLock()`.

**Impact:** On Android, this is triggered by: (1) onboarding flow adding key while agent loop reads config, (2) settings screen updating model while agent loop is active, (3) any background task touching wallet.

**Fix:** Add `@Synchronized` on all mutating methods OR use a `Mutex` (kotlinx.coroutines) for suspend-friendly locking. `Mutex` is preferred since WalletManager is used from coroutine contexts.

#### C3: `migrateFromLegacy()` NOT idempotent — double migration duplicates keys
**What:** Source documents this. If called twice (e.g., app restart race, or migration flag not persisted atomically), legacy keys get added again with new UUIDs, creating duplicates.

**OpenClaw comparison:** Legacy migration checks for `profiles` key in existing store — if present, skips entirely. After migration, deletes the legacy file. Clean and idempotent.

**Fix:** 
1. Check if migration already completed (flag in WalletState or check if legacy store is empty)
2. Delete/clear legacy store after successful migration
3. Use key fingerprinting (hash of provider+prefix) to detect duplicates

### HIGH — Should fix before shipping

#### H1: `allowBackup="true"` in AndroidManifest → key state exfiltrated via ADB backup
**What:** `chat/src/main/AndroidManifest.xml` has `android:allowBackup="true"`. This allows `adb backup` to extract SharedPreferences (including `WalletStorage` data). While `EncryptedKeyStore` uses Android Keystore (keys not extractable), the `WalletState` in regular SharedPreferences reveals key IDs, labels, providers, model selections, and `addedAt` timestamps — metadata useful for targeting.

**OpenClaw comparison:** N/A (server-side, no backup vector). But OpenClaw sets file perms `0600` — principle of least exposure.

**Fix:** Set `android:allowBackup="false"` and `android:dataExtractionRules` for Android 12+ (API 31). Or use `android:fullBackupContent` to exclude sensitive SharedPreferences files.

#### H2: No key validation on add — invalid keys waste agent turns
**What:** `addKey()` stores whatever string is provided. No validation against the provider's API (e.g., Anthropic's `/v1/messages` with a minimal request). An invalid key is only discovered when the agent tries to use it and gets a 401.

**OpenClaw comparison:** OpenClaw tracks `lastGood` per profile and `KeyHealth` (VALID/INVALID/UNKNOWN). The settings screen shows `maskApiKey` + `KeyHealth` status. But OpenClaw also doesn't validate at add-time in the store layer.

**Citros does have:** `KeyHealth` enum and `maskApiKey()` in `SettingsScreen.kt` — so UI health display exists. But the health check appears to be UI-level only, not wired to actual validation.

**Fix:** Add optional async validation in `addKey()` — fire a lightweight API request (e.g., Anthropic: `POST /v1/messages` with 1 token maxTokens). Return `KeyHealth.VALID`/`INVALID`. Don't block on it — validate async and update UI.

#### H3: No key rotation / expiry tracking
**What:** Keys are static. No concept of rotation, expiry, or refresh. If a key is revoked server-side, the only signal is a 401 error during agent execution.

**OpenClaw comparison:** OAuth profiles have `expires` field + `refresh` token. `syncExternalCliCredentials` periodically refreshes CLI tokens (Claude CLI, Codex CLI, Qwen CLI). `EXTERNAL_CLI_SYNC_TTL_MS` = 900s, `EXTERNAL_CLI_NEAR_EXPIRY_MS` = 600s.

**Impact:** Low for API-key-only providers (Anthropic, OpenAI keys don't expire). Higher if we add OAuth providers later.

**Fix:** Deferred — file issue. Add `expiresAt: Long?` to `WalletKey` for future OAuth support. For now, handle 401s gracefully with user-facing "key may be invalid" message.

### MEDIUM — Good to fix, not blocking

#### M1: No config backup / state recovery
**What:** If `SharedPreferencesWalletStorage` gets corrupted (malformed JSON, partial write on crash), all wallet configuration is lost. User must re-enter all keys and re-select models.

**OpenClaw comparison:** `rotateConfigBackups()` keeps 5 numbered backups. Every config write rotates the backup chain.

**Fix:** Keep last-known-good state in a separate SharedPreferences key (e.g., `wallet_state_backup`). On `loadState()` parse failure, fall back to backup.

#### M2: `network_security_config.xml` allows cleartext for localhost
**What:** `<domain-config cleartextTrafficPermitted="true">` for `localhost` and `127.0.0.1`. This is intentional (development bridge), but should be debug-only.

**Impact:** Low — localhost traffic stays on-device. But it's a lint/review flag.

**Fix:** Use `debugOverrides` in network security config instead of base config, or gate with build variant.

#### M3: No usage statistics per key
**What:** No tracking of which key was last used, how many requests succeeded/failed, or cost.

**OpenClaw comparison:** `usageStats` per profile, `lastGood` record for fallback.

**Fix:** Deferred — aligns with #490 (token usage tracking). Add `lastUsedAt`, `requestCount`, `errorCount` to `WalletKey` or a separate stats table.

### INTENTIONAL DIVERGENCE — Documented, acceptable

#### D1: Encrypted key storage (Citros) vs plaintext (OpenClaw)
Citros uses `EncryptedSharedPreferences` backed by Android Keystore — hardware-backed encryption on most devices. This is **strictly better** than OpenClaw's plaintext JSON with file permissions. On a phone, the threat model (device theft, app extraction) justifies encryption. On a server (OpenClaw), file permissions + OS-level access control is standard.

#### D2: Split storage (KeyStore + WalletStorage) vs unified (auth-profiles.json)
Citros separates key material (encrypted) from metadata (serializable state). This is a **better security design** — metadata can be backed up/synced without exposing keys. The downside is the atomicity gap (C1), which should be fixed.

#### D3: No rate limiting (Citros) vs rate limiter (OpenClaw)
OpenClaw rate-limits authentication because it accepts inbound network connections (gateway). Citros is a local phone agent — no inbound auth. Rate limiting is N/A.

#### D4: No timing-safe comparison (Citros) vs timingSafeEqual (OpenClaw)
Same reasoning as D3 — no inbound auth means no timing attack vector.

---

## 3. Summary

| Category | Count | Issues |
|----------|-------|--------|
| CRITICAL | 3 | C1 (atomicity), C2 (thread safety), C3 (migration idempotency) |
| HIGH | 3 | H1 (allowBackup), H2 (key validation), H3 (key rotation) |
| MEDIUM | 3 | M1 (backup/recovery), M2 (cleartext localhost), M3 (usage stats) |
| INTENTIONAL | 4 | D1-D4 (all justified, documented) |

### Citros vs OpenClaw Scorecard

| Aspect | OpenClaw | Citros | Winner |
|--------|----------|--------|--------|
| Encryption at rest | ❌ Plaintext | ✅ Android Keystore | **Citros** |
| Thread safety | ✅ File locking | ❌ None | **OpenClaw** |
| Atomicity | ✅ Single-file atomic | ❌ Two-phase non-atomic | **OpenClaw** |
| Migration safety | ✅ Idempotent + cleanup | ❌ Not idempotent | **OpenClaw** |
| Backup exposure | N/A (server) | ❌ allowBackup=true | **OpenClaw** |
| Key metadata separation | ❌ Keys inline | ✅ Separate encrypted store | **Citros** |
| Rate limiting | ✅ Sliding window | N/A (no inbound) | Tie |
| Recovery | ✅ 5 backup rotation | ❌ None | **OpenClaw** |
| Usage tracking | ✅ Per-profile stats | ❌ None | **OpenClaw** |

**Verdict:** Citros wins on encryption design (D1, D2) but loses on operational robustness (thread safety, atomicity, migration, backup, recovery). The critical fixes (C1-C3) are foundational — they affect correctness, not just polish.

---

## 4. Recommended Issue Breakdown

| Issue | Priority | Title | Effort |
|-------|----------|-------|--------|
| New | CRITICAL | WalletManager: atomic key operations (orphan prevention) | 2-3h |
| New | CRITICAL | WalletManager: thread safety via Mutex | 1-2h |
| New | CRITICAL | WalletManager: idempotent legacy migration | 1h |
| New | HIGH | Set allowBackup=false in AndroidManifest | 15min |
| New | HIGH | Key health validation on add (async) | 2h |
| New | HIGH | Key rotation/expiry tracking (WalletKey.expiresAt) | 1h |
| New | MEDIUM | WalletState backup/recovery on parse failure | 1h |
| New | MEDIUM | Move cleartext localhost to debugOverrides | 15min |
| #490 | MEDIUM | Token usage tracking (extends to per-key stats) | H2 |

---

## 5. Comparison Detail: File Locking (OpenClaw) vs Needed Mutex (Citros)

### OpenClaw's `acquireFileLock()`
```
1. Resolve normalized file path
2. Check HELD_LOCKS map (re-entrant — increment count)
3. If not held: try `fs.open(lockPath, 'wx')` (exclusive create)
4. On EEXIST: check stale (PID alive? age < staleMs?)
   - Stale → delete lock, retry
   - Not stale → wait (exponential backoff with jitter)
5. Write PID + createdAt to lock file
6. Return release function (decrement count → delete lock file)
7. 10 retries, 100ms min / 10s max timeout, randomized
```

### What Citros needs
```kotlin
class WalletManager(
    private val storage: WalletStorage,
    private val keyStore: KeyStore,
) {
    private val mutex = Mutex()
    
    suspend fun addKey(key: WalletKey, apiKey: String) = mutex.withLock {
        keyStore.put(key.id, apiKey)
        val state = storage.loadState() ?: WalletState.DEFAULT
        val updated = state.copy(keys = state.keys + key)
        storage.saveState(updated)
    }
    
    // cleanup on load:
    suspend fun loadOrDefault(): WalletState = mutex.withLock {
        val state = storage.loadState() ?: WalletState.DEFAULT
        // Remove keys whose ID isn't in keyStore (orphan cleanup)
        val valid = state.keys.filter { keyStore.get(it.id) != null }
        if (valid.size != state.keys.size) {
            storage.saveState(state.copy(keys = valid))
        }
        state.copy(keys = valid)
    }
}
```

This is simpler than OpenClaw's file locking because:
1. Citros is single-process (Android app) — no cross-process contention
2. `Mutex` is coroutine-native — no file system overhead
3. No stale-lock detection needed (no PID coordination)
