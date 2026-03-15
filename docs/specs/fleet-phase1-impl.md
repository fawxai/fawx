# Fleet Phase 1 — Implementation Spec

**Subtask 1: Fleet Token + Key** → **Subtask 2: Fleet Manager** → **Subtask 3: CLI Commands**

---

## Subtask 1: Fleet Token + Signing Key

### New file: `engine/crates/fx-fleet/src/token.rs`

#### Types

```rust
/// A fleet bearer token for node authentication.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FleetToken {
    /// Unique token identifier.
    pub token_id: String,
    /// Node this token was issued for.
    pub node_id: String,
    /// When the token was issued (unix ms).
    pub issued_at_ms: u64,
    /// Whether the token has been revoked.
    pub revoked: bool,
    /// The bearer token string (hex-encoded random bytes).
    pub secret: String,
}

/// Fleet signing key for the primary node.
pub struct FleetKey {
    /// Raw key bytes (32 bytes).
    key_bytes: Vec<u8>,
}
```

#### Functions

```rust
impl FleetKey {
    /// Generate a new random fleet key.
    pub fn generate() -> Result<Self, FleetError>;
    
    /// Load from a file path.
    pub fn load(path: &Path) -> Result<Self, FleetError>;
    
    /// Save to a file path (mode 0600).
    pub fn save(&self, path: &Path) -> Result<(), FleetError>;
    
    /// Sign a message (HMAC-SHA256). Used in Phase 2 for task payloads.
    pub fn sign(&self, message: &[u8]) -> Vec<u8>;
    
    /// Verify a signature.
    pub fn verify(&self, message: &[u8], signature: &[u8]) -> bool;
}

impl FleetToken {
    /// Generate a new token for a node.
    pub fn generate(node_id: &str) -> Self;
    
    /// Check if the token matches a presented bearer string.
    pub fn verify_secret(&self, presented: &str) -> bool;
    
    /// Revoke this token.
    pub fn revoke(&mut self);
}
```

#### Dependencies
- `ring` (already in workspace) for random bytes and HMAC
- `hex` for encoding (or use `ring::test::from_hex` / manual encoding)

#### Tests
- `generate_key_produces_32_bytes`
- `save_and_load_key_roundtrip`
- `generate_token_produces_unique_secrets`
- `verify_secret_accepts_correct_token`
- `verify_secret_rejects_wrong_token`
- `revoked_token_still_verifies_secret` (revocation is checked at manager level)
- `sign_and_verify_roundtrip`
- `verify_rejects_tampered_message`

---

## Subtask 2: Fleet Manager + Persistence

### New file: `engine/crates/fx-fleet/src/manager.rs`

#### Types

```rust
/// High-level fleet management.
pub struct FleetManager {
    /// Base directory (~/.fawx/fleet/).
    fleet_dir: PathBuf,
    /// Signing key.
    key: FleetKey,
    /// Node registry.
    registry: NodeRegistry,
    /// Issued tokens.
    tokens: Vec<FleetToken>,
}
```

#### Functions

```rust
impl FleetManager {
    /// Initialize a new fleet (generates key, creates dirs).
    pub fn init(fleet_dir: &Path) -> Result<Self, FleetError>;
    
    /// Load existing fleet state from disk.
    pub fn load(fleet_dir: &Path) -> Result<Self, FleetError>;
    
    /// Add a node: generates token, registers in registry, persists.
    pub fn add_node(&mut self, name: &str, ip: &str, port: u16) -> Result<FleetToken, FleetError>;
    
    /// Remove a node: revokes token, removes from registry, persists.
    pub fn remove_node(&mut self, name: &str) -> Result<(), FleetError>;
    
    /// List all nodes with their status.
    pub fn list_nodes(&self) -> Vec<&NodeInfo>;
    
    /// Verify a presented bearer token. Returns the node_id if valid.
    pub fn verify_bearer(&self, bearer: &str) -> Option<String>;
    
    /// Persist current state (registry + tokens) to disk.
    fn persist(&self) -> Result<(), FleetError>;
}
```

#### File layout
```
~/.fawx/fleet/
├── fleet.key           # 32-byte signing key (mode 0600)
├── nodes.json          # NodeInfo registry
└── tokens.json         # Issued FleetTokens
```

#### Tests
- `init_creates_directory_and_key`
- `add_node_generates_token_and_registers`
- `remove_node_revokes_token`
- `verify_bearer_accepts_valid_token`
- `verify_bearer_rejects_revoked_token`
- `verify_bearer_rejects_unknown_token`
- `persist_and_load_roundtrip`
- `add_duplicate_node_returns_error`

---

## Subtask 3: CLI Commands

### Modified: `engine/crates/fx-cli/src/commands/` (new fleet module)

#### Commands

```
fawx fleet init                     # Initialize fleet on this node (primary)
fawx fleet add <name> --ip <ip>     # Add a worker node, print join command
fawx fleet remove <name>            # Remove a worker node
fawx fleet list                     # List all nodes with status
```

#### Output examples

```
$ fawx fleet init
✓ Fleet initialized at ~/.fawx/fleet/
✓ Signing key generated
✓ Ready to add nodes with: fawx fleet add <name> --ip <ip>

$ fawx fleet add macmini --ip 100.75.191.19
✓ Node "macmini" registered
✓ Token generated: tok_a1b2c3...

  Join command (run on the worker):
  fawx fleet join 100.93.251.101:8400 --token tok_a1b2c3d4e5f6...

$ fawx fleet list
┌──────────┬─────────────────┬────────┬─────────────┐
│ Name     │ IP              │ Status │ Last Seen   │
├──────────┼─────────────────┼────────┼─────────────┤
│ macmini  │ 100.75.191.19   │ online │ 2s ago      │
│ macbook  │ 100.75.191.20   │ stale  │ 5m ago      │
└──────────┴─────────────────┴────────┴─────────────┘

$ fawx fleet remove macbook
✓ Node "macbook" removed
✓ Token revoked
```
