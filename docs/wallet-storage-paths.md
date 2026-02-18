# Wallet & Storage File Paths

> Issue: [#250](https://github.com/abbudjoe/citros/issues/250)  
> Audience: developers working on credential storage and migration

## Android (Kotlin) — Wallet Storage

Citros Android stores credentials and wallet state in two locations:

### Encrypted Key Store
- **Implementation:** `EncryptedKeyStore` (wraps Android Keystore / EncryptedSharedPreferences)
- **Path:** Managed by Android Keystore — no user-visible file path
- **Contents:** Raw API key values, indexed by wallet key ID

### Wallet Metadata
- **Implementation:** `SharedPreferencesWalletStorage`
- **Path:** `/data/data/ai.citros.app/shared_prefs/citros_wallet.xml`
- **Contents:** `WalletState` JSON — key metadata (provider, label, addedAt), active key ID, model selections
- **Does NOT contain:** Raw API key values (those are in the encrypted key store)
- **Example structure:**
  ```xml
  <?xml version='1.0' encoding='utf-8' standalone='yes' ?>
  <map>
      <string name="wallet_state">{
        "keys": [{
          "id": "abc123",
          "provider": "ANTHROPIC",
          "label": "My Anthropic Key",
          "addedAt": 1707700000000
        }],
        "activeKeyId": "abc123",
        "chatModel": "claude-sonnet-4-20250514",
        "actionModel": "claude-haiku-4-20250514"
      }</string>
  </map>
  ```

### Legacy Storage (pre-wallet)
- **SharedPreferences:** `/data/data/ai.citros.app/shared_prefs/citros.xml`
  - `cloud_token` — plaintext API key (migrated to encrypted store on first launch)
  - `cloud_provider` — provider enum name
  - `cloud_auth_kind` — auth method enum name
- **Secure Store:** `SecureCredentialStore` — EncryptedSharedPreferences wrapper
  - Key: `cloud_token`

### Migration Flow
`WalletManager.migrateFromLegacy()` handles one-time migration from legacy `SharedPreferences` storage to the wallet system:

1. Reads token from legacy prefs or secure store
2. Detects provider from token format or saved preference
3. Creates a `WalletKey` entry with the token stored in `EncryptedKeyStore`
4. Sets the new key as active
5. Configures default chat/action models for the provider

**Note:** Migration does NOT delete legacy prefs — the caller (`ChatScreen`) handles cleanup after successful migration to allow rollback.

---

## Rust Daemon (`ct-cli`) — redb Storage

The Rust daemon (binary name: `ct-cli`) uses redb for persistent encrypted storage:

### Storage Path
- **Config field:** `storage_path` in the daemon config JSON
- **Default location:** Specified at startup — no hardcoded default
- **Example:** `/data/local/tmp/citros-storage.redb`

### Contents
- Conversation history
- Agent state
- Encrypted credentials (via `ring` + `ct-storage`)

### Config Example
```json
{
  "model_path": "/data/local/tmp/model.gguf",
  "api_key_path": "/data/local/tmp/keys.enc",
  "storage_path": "/data/local/tmp/citros-storage.redb",
  "policy_path": "/data/local/tmp/policy.json",
  "log_level": "info"
}
```

---

## Path Discrepancies to Avoid

| ❌ Wrong | ✅ Correct |
|---|---|
| `citros_vault.redb` | `citros-storage.redb` (or whatever `storage_path` is set to) |
| `/sdcard/citros/vault` | `/data/local/tmp/citros-storage.redb` (or app-private storage) |
| `shared_prefs/vault.xml` | `shared_prefs/citros_wallet.xml` (wallet metadata) |

The Rust daemon does not use the term "vault" — it uses `storage_path` pointing to a redb database file. The Android side uses "wallet" for the credential management system.
