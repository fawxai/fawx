# PAT Borrowing — Spec

Date: 2026-03-17
Status: Implementation-ready

## Goal

Let Fawx subagents/workers safely use the owner's GitHub PAT for repository
operations (push branches, create PRs, comment) without granting merge
authority or leaking credentials beyond their intended scope.

## Current State

- `fx-auth/src/github.rs` — validates PATs (scope checks, fine-grained PAT handling). No brokering.
- `fx-auth/src/credential_store.rs` — `CredentialStore` trait + `EncryptedFileCredentialStore`. Stores GitHub PAT under `AuthProvider::GitHub / CredentialMethod::Pat`.
- `fx-cli/src/startup.rs` — `CredentialStoreBridge` implements `CredentialProvider` (from `fx-skills`). Maps `"github_token"` → owner's PAT. Only available to WASM skills.
- `fx-subagent` — `SpawnConfig` has no credential fields. `HeadlessSubagentFactory::build_app()` creates a `HeadlessApp` per subagent but **does not pass credentials**. Subagents cannot access the credential store.
- `GitSkill` — shells out to `git` CLI. Uses OS-level git credential helpers. No in-engine PAT injection.

**Result:** Subagents have zero GitHub access. They cannot push, create PRs, or even read private repo metadata through the engine.

## Design

### Two borrow scopes

```rust
// In fx-auth/src/token_broker.rs

/// Scope of a borrowed GitHub credential.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BorrowScope {
    /// Read-only: inspect repo state, read PRs/issues/checks.
    /// No pushes, no PR creation, no state-mutating comments.
    ReadOnly,
    /// Contribution: push branches, create PRs, comment on PRs.
    /// Cannot merge protected branches (enforced by GitHub branch protection).
    Contribution,
}
```

### TokenBorrow — scoped credential wrapper

```rust
/// A borrowed GitHub credential with an explicit scope.
///
/// The scope restricts which operations the borrower should perform.
/// ReadOnly borrowers MUST NOT use the token for write operations.
/// Contribution borrowers may push/create PRs but cannot merge
/// protected branches (GitHub branch protection is the hard gate).
///
/// Important: scope enforcement is cooperative at the application level.
/// The token itself has whatever permissions the owner's PAT grants.
/// The scope field documents intent and enables audit logging.
/// Hard enforcement comes from GitHub branch protection rules.
pub struct TokenBorrow {
    token: Zeroizing<String>,
    scope: BorrowScope,
}

impl TokenBorrow {
    pub fn new(token: Zeroizing<String>, scope: BorrowScope) -> Self { ... }
    pub fn token(&self) -> &str { ... }
    pub fn scope(&self) -> BorrowScope { ... }
}
```

No `Debug` impl (would leak token). No `Clone` (forces explicit re-borrow).

### TokenBroker — credential lending trait

```rust
/// Errors from token borrowing.
#[derive(Debug)]
pub enum BorrowError {
    /// No credential is configured for the requested provider.
    NotConfigured,
    /// The credential store could not be read.
    StoreError(String),
    /// The requested scope exceeds what is configured.
    ScopeExceeded { requested: BorrowScope, max: BorrowScope },
}

/// Trait for lending scoped credentials to subagents/workers.
///
/// Implementations read the owner's stored credential and wrap it
/// in a scope-limited `TokenBorrow`. The broker does NOT create
/// new tokens — it lends the existing one with documented constraints.
pub trait TokenBroker: Send + Sync {
    fn borrow_github(&self, scope: BorrowScope) -> Result<TokenBorrow, BorrowError>;
}
```

### CredentialStoreBroker — concrete implementation

```rust
/// Brokers GitHub credentials from the encrypted credential store.
///
/// Reads the owner's stored PAT and wraps it in a `TokenBorrow`
/// with the requested scope, subject to the configured max scope.
pub struct CredentialStoreBroker {
    store: Arc<EncryptedFileCredentialStore>,
    max_scope: BorrowScope,
}
```

- `borrow_github(ReadOnly)` — always succeeds if token exists
- `borrow_github(Contribution)` — succeeds only if `max_scope == Contribution`

### Config surface

```toml
# In config.toml under [security]
# Maximum scope that subagents/workers can borrow.
# "read_only" or "contribution". Default: "read_only".
github_borrow_scope = "contribution"
```

Add to `SecurityConfig` in `fx-config`:

```rust
/// Maximum GitHub PAT borrow scope for subagents/workers.
/// Default: ReadOnly (safest).
#[serde(default)]
pub github_borrow_scope: BorrowScope,
```

`BorrowScope` default impl returns `ReadOnly`.

### Wiring

#### 1. HeadlessSubagentFactoryDeps gets a broker

```rust
pub struct HeadlessSubagentFactoryDeps {
    pub router: Arc<ModelRouter>,
    pub config: FawxConfig,
    pub improvement_provider: Option<Arc<dyn CompletionProvider + Send + Sync>>,
    pub session_bus: Option<SessionBus>,
    pub token_broker: Option<Arc<dyn TokenBroker>>,  // NEW
}
```

#### 2. HeadlessSubagentFactory passes broker to subagent apps

In `build_app()`, after creating the `HeadlessApp`, inject the broker into the subagent's credential provider chain. The subagent's `CredentialStoreBridge` gains a `token_broker: Option<Arc<dyn TokenBroker>>` field. When `"github_token"` is requested:

1. If `token_broker` is Some → call `broker.borrow_github(scope)` → return borrowed token
2. If `token_broker` is None → fall back to direct credential store lookup (owner mode)

#### 3. startup.rs creates the broker

In `build_headless_app()` (the top-level HTTP/headless startup), after opening the credential store:

```rust
let token_broker: Option<Arc<dyn TokenBroker>> = credential_store.as_ref().map(|store| {
    Arc::new(CredentialStoreBroker::new(
        Arc::clone(store),
        config.security.github_borrow_scope,
    )) as Arc<dyn TokenBroker>
});
```

Pass into `HeadlessSubagentFactoryDeps`.

#### 4. SkillRegistryBuildOptions gets a broker

For subagent apps built via `build_headless_loop_engine_bundle()` → `build_skill_registry()`, the broker needs to reach the `CredentialStoreBridge`. Add `token_broker: Option<Arc<dyn TokenBroker>>` to `SkillRegistryBuildOptions` and thread it through.

### What this does NOT do

- Does not create new GitHub tokens or fine-grained PATs (that's a future enhancement)
- Does not enforce scope at the HTTP request level (cooperative enforcement; GitHub branch protection is the hard gate)
- Does not change GitSkill behavior (git CLI still uses OS credential helpers)
- Does not affect WASM skill credential access (they go through the same bridge, unscoped)

### Security invariants (documented, not all code-enforced)

1. Branch protection on `staging` + `main` requires repo-owner review before merge
2. Owner should use a fine-grained PAT scoped to the target repo only
3. No `administration` permission on the PAT
4. Subagents can push branches + create PRs + comment, but cannot merge protected branches
5. `dev` remains open for the automated pipeline (Clawdio merge authority)
6. `max_scope` defaults to `ReadOnly` — must be explicitly configured for contribution

### File changes

| File | Change |
|------|--------|
| `engine/crates/fx-auth/src/token_broker.rs` | **NEW** — `BorrowScope`, `TokenBorrow`, `BorrowError`, `TokenBroker` trait, `CredentialStoreBroker` impl |
| `engine/crates/fx-auth/src/lib.rs` | Add `pub mod token_broker;` |
| `engine/crates/fx-config/src/lib.rs` | Add `github_borrow_scope: BorrowScope` to `SecurityConfig`, add `BorrowScope` type (or re-export from fx-auth) |
| `engine/crates/fx-cli/src/startup.rs` | Add `token_broker` to `CredentialStoreBridge`, thread through `SkillRegistryBuildOptions` and `SkillRegistryBundle`, create broker in top-level startup |
| `engine/crates/fx-cli/src/headless.rs` | Add `token_broker` to `HeadlessSubagentFactoryDeps`, pass to subagent app builds |

### Tests

#### Unit tests in `fx-auth/src/token_broker.rs`:

1. `borrow_readonly_succeeds_when_token_exists` — store has PAT, borrow ReadOnly → Ok
2. `borrow_contribution_succeeds_when_max_is_contribution` — max=Contribution, borrow Contribution → Ok
3. `borrow_contribution_fails_when_max_is_readonly` — max=ReadOnly, borrow Contribution → ScopeExceeded
4. `borrow_fails_when_no_token_stored` — empty store → NotConfigured
5. `token_borrow_exposes_scope` — verify scope() accessor
6. `token_borrow_exposes_token` — verify token() accessor
7. `borrow_scope_default_is_read_only` — Default trait returns ReadOnly
8. `borrow_scope_serde_roundtrip` — serialize/deserialize both variants

#### Unit tests in `fx-config` (if BorrowScope added there):

9. `security_config_default_borrow_scope_is_read_only` — default SecurityConfig
10. `security_config_deserializes_contribution_scope` — TOML with `github_borrow_scope = "contribution"`

#### Integration-worthy (manual, tell Joe):

- Build from branch, configure `github_borrow_scope = "contribution"`, spawn a subagent that runs `git push` to a test branch, verify it works
- Set `github_borrow_scope = "read_only"`, spawn subagent that tries to create a PR, verify it gets ReadOnly scope (cooperative — the token still works, but the scope metadata is correct)

### Estimated scope

~350-450 lines including tests. One PR.
