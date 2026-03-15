# Wave 2 PR G — TransactionSkill

## Goal
Wire `fx-transactions` into the agent by implementing the `Skill` trait, exposing 4 tools:
`begin_transaction`, `stage_file`, `commit_transaction`, `rollback_transaction`.

## Target files
- **NEW**: `engine/crates/fx-loadable/src/transaction_skill.rs`
- **EDIT**: `engine/crates/fx-loadable/src/lib.rs` (add `pub mod transaction_skill;`, re-export)
- **EDIT**: `engine/crates/fx-loadable/Cargo.toml` (add `fx-transactions` dependency)
- **EDIT**: `engine/crates/fx-cli/src/tui.rs` — register `TransactionSkill` in SkillRegistry where other skills are registered

## Skill trait contract
```rust
use async_trait::async_trait;
use fx_kernel::act::ToolCacheability;
use fx_kernel::cancellation::CancellationToken;
use fx_llm::ToolDefinition;

pub trait Skill: Send + Sync + std::fmt::Debug {
    fn name(&self) -> &str;
    fn tool_definitions(&self) -> Vec<ToolDefinition>;
    fn cacheability(&self, tool_name: &str) -> ToolCacheability;
    async fn execute(&self, tool_name: &str, arguments: &str, cancel: Option<&CancellationToken>) -> Option<Result<String, SkillError>>;
}
```

## TransactionSkill design

```rust
#[derive(Debug)]
pub struct TransactionSkill {
    store: Arc<Mutex<TransactionStore>>,
    config: SelfModifyConfig, // from fx-transactions, has path tier rules
    work_dir: PathBuf,
}
```

### Tool definitions

1. **begin_transaction** — `{ label: string, validation_command?: string }` → returns `{ tx_id: u32 }`
2. **stage_file** — `{ tx_id: u32, path: string, content: string }` → returns confirmation
3. **commit_transaction** — `{ tx_id: u32 }` → runs validation if set, writes files atomically, returns result
4. **rollback_transaction** — `{ tx_id: u32 }` → restores originals, returns confirmation

### Cacheability
All 4 tools: `ToolCacheability::NeverCache` (stateful, side-effectful).

### Error handling
- Parse errors → `Some(Err("Invalid arguments: ..."))` 
- Transaction errors (from fx-transactions) → `Some(Err(e.to_string()))`
- Unknown tool names → `None`
- Never `.unwrap()` — use `serde_json::from_str` with proper error mapping

### Arguments parsing
Each tool parses its arguments from the `&str` JSON using `serde_json::from_str` into a small args struct:
```rust
#[derive(Deserialize)]
struct BeginArgs { label: String, validation_command: Option<String> }
#[derive(Deserialize)]  
struct StageArgs { tx_id: u32, path: String, content: String }
#[derive(Deserialize)]
struct CommitArgs { tx_id: u32 }
#[derive(Deserialize)]
struct RollbackArgs { tx_id: u32 }
```

### Registration
In `tui.rs`, wherever `SkillRegistry` is constructed and skills are registered:
```rust
let tx_skill = TransactionSkill::new(work_dir.clone());
registry.register(Box::new(tx_skill));
```

`TransactionSkill::new` creates a default `TransactionStore` and `SelfModifyConfig`.

## Tests (in transaction_skill.rs)
1. `name_returns_transaction_skill` — verify `name()` returns `"transaction_skill"`
2. `tool_definitions_returns_four_tools` — verify 4 tools with correct names
3. `begin_and_stage_and_commit_workflow` — full happy path
4. `rollback_restores_original` — begin → stage → rollback → verify original content
5. `unknown_tool_returns_none` — `execute("unknown", ...)` returns `None`
6. `invalid_json_returns_error` — bad args string → `Some(Err(...))`
7. `commit_without_staging_returns_error` — begin → commit with no staged files → error
8. `stage_to_nonexistent_transaction_returns_error` — stage to invalid tx_id

## Constraints
- No functions > 40 lines. Each tool handler is its own method.
- No `.unwrap()` outside tests.
- `cargo fmt --all` before commit.
- Run `cargo test -p fx-loadable` to verify.
