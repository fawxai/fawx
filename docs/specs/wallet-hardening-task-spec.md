# Task Spec: Wallet Hardening + Retry on 529

**Issues:** #505, #506, #507, #508, #511
**Branch:** `fix/wallet-hardening` from `feat/android-mvp`
**PR title:** `[Clawdio] Wallet hardening: thread safety, atomicity, migration, retry on 529`

---

## Changes Required

### 1. WalletManager thread safety (#506)

**File:** `core/src/main/kotlin/ai/citros/core/WalletManager.kt`

Add `@Synchronized` to ALL public methods. No signature changes needed.

Methods: `loadOrDefault`, `addKey`, `removeKey`, `setActiveKey`, `setChatModel`, `setActionModel`, `activeConfig`, `migrateFromLegacy`

Update class KDoc: "Thread-safe via synchronized methods" (was "NOT thread-safe").

### 2. Atomic key operations (#505)

**File:** `core/src/main/kotlin/ai/citros/core/WalletManager.kt`

**In `addKey()`:** Reorder — save state FIRST, then keyStore.put(). If keyStore fails, loadOrDefault() cleans up. If we stored key first and saveState failed, orphaned key forever.

**In `loadOrDefault()`:** Add orphan cleanup:
```kotlin
val validKeys = state.keys.filter { keyStore.get(it.id) != null }
if (validKeys.size != state.keys.size) {
    val cleaned = state.copy(
        keys = validKeys,
        activeKeyId = if (validKeys.any { it.id == state.activeKeyId }) state.activeKeyId else null
    )
    storage.saveState(cleaned)
    return cleaned
}
```

Remove "Phase 2" notes from KDoc.

### 3. Idempotent migration (#507)

**File:** `core/src/main/kotlin/ai/citros/core/WalletManager.kt`

Add early return in `migrateFromLegacy()` if wallet already has keys:
```kotlin
val existingState = loadOrDefault()
if (existingState.keys.isNotEmpty()) {
    return existingState.keys.first()
}
```

Update KDoc: remove "NOT idempotent", add "Idempotent: returns existing key if wallet is non-empty."

### 4. allowBackup=false (#508)

- `chat/src/main/AndroidManifest.xml` line 18: `allowBackup="true"` -> `allowBackup="false"`
- `app/src/main/AndroidManifest.xml` line 10: `allowBackup="true"` -> `allowBackup="false"`

### 5. Retry on 529 Overloaded (#511)

**File:** `core/src/main/kotlin/ai/citros/core/BaseProviderClient.kt`

Add helper after `shouldRetryRateLimit()`:
```kotlin
private fun isRetryableServerError(code: Int): Boolean =
    code == 529 || code == 503
```

In `executeRequest()`, expand the retry block to also retry on 529/503 with same backoff.
Update class and method KDocs to mention 529/503.

## Tests to Add

**File:** `core/src/test/kotlin/ai/citros/core/WalletManagerTest.kt`

1. `loadOrDefault cleans up orphaned key entries missing from KeyStore` — simulate state entry with no keyStore match
2. `migrateFromLegacy is idempotent - returns existing key if wallet non-empty` — double migration returns same key

## Build & Verify

```bash
cd ~/citros/android
git checkout feat/android-mvp && git pull
git checkout -b fix/wallet-hardening
# Make changes
./gradlew :core:testDebugUnitTest --tests "ai.citros.core.WalletManagerTest"
./gradlew :core:compileDebugKotlin
git add -A && git commit && git push -u origin fix/wallet-hardening
gh pr create --base feat/android-mvp
# Then: @claude review this PR
```
