# Decision: SharedPreferences + Keystore AES for Credential Storage

**Issue:** #249 — Resolve SharedPreferences vs Encrypted DataStore decision

**Date:** 2026-02-12

## Decision

**SharedPreferences + Android Keystore AES is the permanent solution** for credential storage in Fawx. Encrypted DataStore migration is not planned.

## Context

During the Key Wallet implementation ([#233](https://github.com/abbudjoe/fawx/issues/233)), we chose `EncryptedSharedPreferences` (backed by Android Keystore AES-256-GCM) for storing API keys and wallet state. [PR #239](https://github.com/abbudjoe/fawx/pull/239) raised the question of whether this should migrate to Encrypted DataStore.

## Rationale

1. **EncryptedSharedPreferences is production-grade and battle-tested** — used by major apps, backed by AndroidX Security library, AES-256-GCM encryption via hardware-backed Keystore.

2. **DataStore adds complexity without security benefit** — DataStore offers better async/coroutine support but the same encryption layer (both use MasterKey). Our credential access patterns are simple key-value lookups, not streaming.

3. **Small data size** — We store ~5-10 API keys max. SharedPreferences handles this without any performance concerns.

4. **Migration risk** — Migrating encrypted storage requires careful key migration or re-entry. Not worth the risk for no security gain.

## Alternatives Considered

1. **Encrypted DataStore** — Better async/coroutine support but same encryption layer (both use MasterKey). Adds migration complexity for no security gain.
2. **Room with SQLCipher** — Full database encryption. Overkill for ~5-10 key-value pairs; adds significant dependency weight and schema management overhead.
3. **Custom encryption wrapper** — Direct AES/RSA via `javax.crypto`. Fragile, error-prone, and reinvents what AndroidX Security already provides with hardware-backed key management.

## Implications

- `EncryptedKeyStore` in `:chat` module is the canonical credential storage implementation
- `SharedPreferencesWalletStorage` is the canonical wallet state storage
- No DataStore dependency needed in the project
- Future work should use the existing `KeyStore` / `WalletStorage` interfaces in `:core`
