//! TransactionSkill — exposes `fx-transactions` as agent tools.
//!
//! Provides four tools for multi-file atomic edits:
//! `begin_transaction`, `stage_file`, `commit_transaction`, `rollback_transaction`.

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use fx_core::self_modify::SelfModifyConfig;
use fx_kernel::act::ToolCacheability;
use fx_kernel::cancellation::CancellationToken;
use fx_llm::ToolDefinition;
use serde::Deserialize;
use tokio::sync::Mutex;

use fx_transactions::executor;
use fx_transactions::store::TransactionStore;

use crate::skill::{Skill, SkillError};

/// Agent skill that wraps transaction store operations as tool calls.
#[derive(Debug)]
pub struct TransactionSkill {
    store: Arc<Mutex<TransactionStore>>,
    config: SelfModifyConfig,
    work_dir: PathBuf,
}

#[derive(Deserialize)]
struct BeginArgs {
    label: String,
    validation_command: Option<String>,
}

#[derive(Deserialize)]
struct StageArgs {
    tx_id: u32,
    path: String,
    content: String,
}

#[derive(Deserialize)]
struct CommitArgs {
    tx_id: u32,
}

#[derive(Deserialize)]
struct RollbackArgs {
    tx_id: u32,
}

impl TransactionSkill {
    /// Create a new `TransactionSkill`.
    ///
    /// # Arguments
    ///
    /// * `work_dir` — The project/repo root directory. Used as both the working
    ///   directory for file writes (staged paths are joined to this) and the
    ///   base directory for self-modify policy path matching.
    /// * `self_modify` — Self-modification policy config. When `Some`, path-tier
    ///   enforcement is applied at commit time. When `None`, all paths are
    ///   allowed (policy disabled). Callers should pass the real config from
    ///   the application configuration rather than using `SelfModifyConfig::default()`.
    pub fn new(work_dir: PathBuf, self_modify: Option<SelfModifyConfig>) -> Self {
        Self {
            store: Arc::new(Mutex::new(TransactionStore::new())),
            config: self_modify.unwrap_or_default(),
            work_dir,
        }
    }

    async fn handle_begin(&self, arguments: &str) -> Result<String, SkillError> {
        let args: BeginArgs =
            serde_json::from_str(arguments).map_err(|e| format!("Invalid arguments: {e}"))?;
        let mut store = self.store.lock().await;
        // Iteration is hardcoded to 0 because TransactionSkill doesn't have
        // access to the current loop iteration counter. Acceptable for Phase 1;
        // the iteration value is only used for metadata/diagnostics in the store.
        let tx_id = store
            .begin(args.label, args.validation_command, 0)
            .map_err(|e| e.to_string())?;
        Ok(serde_json::json!({"tx_id": tx_id}).to_string())
    }

    async fn handle_stage(&self, arguments: &str) -> Result<String, SkillError> {
        let args: StageArgs =
            serde_json::from_str(arguments).map_err(|e| format!("Invalid arguments: {e}"))?;
        let path = self.work_dir.join(&args.path);
        let mut store = self.store.lock().await;
        // Iteration hardcoded to 0 — see comment in handle_begin.
        store
            .stage(args.tx_id, path, args.content, 0)
            .map_err(|e| e.to_string())?;
        Ok(format!(
            "Staged file '{}' in transaction #{}",
            args.path, args.tx_id
        ))
    }

    async fn handle_commit(&self, arguments: &str) -> Result<String, SkillError> {
        let args: CommitArgs =
            serde_json::from_str(arguments).map_err(|e| format!("Invalid arguments: {e}"))?;
        let mut store = self.store.lock().await;
        let count = executor::commit(
            &mut store,
            args.tx_id,
            Some(&self.work_dir),
            &self.config,
            &self.work_dir,
        )
        .await
        .map_err(|e| e.to_string())?;
        Ok(format!(
            "Transaction #{} committed: {count} file(s) written",
            args.tx_id
        ))
    }

    async fn handle_rollback(&self, arguments: &str) -> Result<String, SkillError> {
        let args: RollbackArgs =
            serde_json::from_str(arguments).map_err(|e| format!("Invalid arguments: {e}"))?;
        let mut store = self.store.lock().await;
        executor::rollback(&mut store, args.tx_id)
            .await
            .map_err(|e| e.to_string())?;
        Ok(format!("Transaction #{} rolled back", args.tx_id))
    }
}

fn begin_transaction_definition() -> ToolDefinition {
    ToolDefinition {
        name: "begin_transaction".to_string(),
        description: concat!(
            "Start a new multi-file transaction. Transactions let you group ",
            "multiple file writes into an atomic unit — either all files are ",
            "written on commit, or none are (on rollback). Use this when you ",
            "need to modify several files that must be consistent with each other.",
        )
        .to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "label": {
                    "type": "string",
                    "description": "Human-readable label describing the purpose of this transaction"
                },
                "validation_command": {
                    "type": "string",
                    "description": "Optional shell command to run before commit completes. If the command fails, the commit is aborted and all staged changes are discarded."
                }
            },
            "required": ["label"]
        }),
    }
}

fn stage_file_definition() -> ToolDefinition {
    ToolDefinition {
        name: "stage_file".to_string(),
        description: concat!(
            "Stage a file write inside an open transaction. The file is NOT ",
            "written to disk yet — it is buffered until commit_transaction is ",
            "called. This ensures atomicity: if any file fails validation or ",
            "you decide to rollback, no partial writes exist on disk.",
        )
        .to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "tx_id": {
                    "type": "integer",
                    "description": "Transaction ID returned by begin_transaction"
                },
                "path": {
                    "type": "string",
                    "description": "File path relative to the project root"
                },
                "content": {
                    "type": "string",
                    "description": "Complete file content to write (replaces any existing content)"
                }
            },
            "required": ["tx_id", "path", "content"]
        }),
    }
}

fn commit_transaction_definition() -> ToolDefinition {
    ToolDefinition {
        name: "commit_transaction".to_string(),
        description: concat!(
            "Commit an open transaction, atomically writing all staged files ",
            "to disk. If a validation command was set, it runs first — on ",
            "failure the commit is aborted and no files are written. Committed ",
            "transactions cannot be rolled back.",
        )
        .to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "tx_id": {
                    "type": "integer",
                    "description": "Transaction ID to commit"
                }
            },
            "required": ["tx_id"]
        }),
    }
}

fn rollback_transaction_definition() -> ToolDefinition {
    ToolDefinition {
        name: "rollback_transaction".to_string(),
        description: concat!(
            "Roll back an open transaction, discarding all staged file changes. ",
            "Since staged files are only buffered in memory (not written to disk), ",
            "rollback simply clears the buffer. Use this when you decide the ",
            "planned changes are no longer needed.",
        )
        .to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "tx_id": {
                    "type": "integer",
                    "description": "Transaction ID to roll back"
                }
            },
            "required": ["tx_id"]
        }),
    }
}

#[async_trait]
impl Skill for TransactionSkill {
    fn name(&self) -> &str {
        "transaction_skill"
    }

    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        vec![
            begin_transaction_definition(),
            stage_file_definition(),
            commit_transaction_definition(),
            rollback_transaction_definition(),
        ]
    }

    fn cacheability(&self, tool_name: &str) -> ToolCacheability {
        // Match explicitly for future-proofing: if new tools are added with
        // different cacheability, the compiler will not warn on a catch-all.
        match tool_name {
            "begin_transaction" | "stage_file" | "commit_transaction" | "rollback_transaction" => {
                ToolCacheability::NeverCache
            }
            _ => ToolCacheability::NeverCache,
        }
    }

    async fn execute(
        &self,
        tool_name: &str,
        arguments: &str,
        _cancel: Option<&CancellationToken>,
    ) -> Option<Result<String, SkillError>> {
        match tool_name {
            "begin_transaction" => Some(self.handle_begin(arguments).await),
            "stage_file" => Some(self.handle_stage(arguments).await),
            "commit_transaction" => Some(self.handle_commit(arguments).await),
            "rollback_transaction" => Some(self.handle_rollback(arguments).await),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn skill_in_tempdir(dir: &TempDir) -> TransactionSkill {
        TransactionSkill::new(dir.path().to_path_buf(), None)
    }

    #[test]
    fn name_returns_transaction_skill() {
        let dir = TempDir::new().unwrap();
        let skill = skill_in_tempdir(&dir);
        assert_eq!(skill.name(), "transaction_skill");
    }

    #[test]
    fn tool_definitions_returns_four_tools() {
        let dir = TempDir::new().unwrap();
        let skill = skill_in_tempdir(&dir);
        let defs = skill.tool_definitions();
        assert_eq!(defs.len(), 4);

        let names: Vec<&str> = defs.iter().map(|d| d.name.as_str()).collect();
        assert!(names.contains(&"begin_transaction"));
        assert!(names.contains(&"stage_file"));
        assert!(names.contains(&"commit_transaction"));
        assert!(names.contains(&"rollback_transaction"));
    }

    #[tokio::test]
    async fn begin_and_stage_and_commit_workflow() {
        let dir = TempDir::new().unwrap();
        let skill = skill_in_tempdir(&dir);

        // Begin
        let result = skill
            .execute("begin_transaction", r#"{"label": "test workflow"}"#, None)
            .await;
        let output = result.unwrap().unwrap();
        assert!(output.contains("\"tx_id\":0"));

        // Stage
        let result = skill
            .execute(
                "stage_file",
                r#"{"tx_id": 0, "path": "hello.txt", "content": "hello world"}"#,
                None,
            )
            .await;
        let output = result.unwrap().unwrap();
        assert!(output.contains("Staged file"));

        // Commit
        let result = skill
            .execute("commit_transaction", r#"{"tx_id": 0}"#, None)
            .await;
        let output = result.unwrap().unwrap();
        assert!(output.contains("committed"));
        assert!(output.contains("1 file(s)"));

        // Verify file on disk
        let content = std::fs::read_to_string(dir.path().join("hello.txt")).unwrap();
        assert_eq!(content, "hello world");
    }

    #[tokio::test]
    async fn rollback_uncommitted_discards_staged_changes() {
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("existing.txt");
        std::fs::write(&file_path, "original content").unwrap();
        let skill = skill_in_tempdir(&dir);

        // Begin and stage (file is only buffered, not written to disk)
        skill
            .execute("begin_transaction", r#"{"label": "rollback test"}"#, None)
            .await
            .unwrap()
            .unwrap();

        skill
            .execute(
                "stage_file",
                r#"{"tx_id": 0, "path": "existing.txt", "content": "modified"}"#,
                None,
            )
            .await
            .unwrap()
            .unwrap();

        // Rollback discards the staged buffer
        let result = skill
            .execute("rollback_transaction", r#"{"tx_id": 0}"#, None)
            .await;
        let output = result.unwrap().unwrap();
        assert!(output.contains("rolled back"));

        // Original file on disk should be unchanged since staged writes never hit disk
        let content = std::fs::read_to_string(&file_path).unwrap();
        assert_eq!(content, "original content");
    }

    #[tokio::test]
    async fn unknown_tool_returns_none() {
        let dir = TempDir::new().unwrap();
        let skill = skill_in_tempdir(&dir);
        let result = skill.execute("unknown_tool", "{}", None).await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn invalid_json_returns_error() {
        let dir = TempDir::new().unwrap();
        let skill = skill_in_tempdir(&dir);
        let result = skill
            .execute("begin_transaction", "not valid json", None)
            .await;
        let err = result.unwrap().unwrap_err();
        assert!(err.contains("Invalid arguments"));
    }

    #[tokio::test]
    async fn commit_without_staging_returns_error() {
        let dir = TempDir::new().unwrap();
        let skill = skill_in_tempdir(&dir);

        // Begin
        skill
            .execute("begin_transaction", r#"{"label": "empty commit"}"#, None)
            .await
            .unwrap()
            .unwrap();

        // Commit with no staged files
        let result = skill
            .execute("commit_transaction", r#"{"tx_id": 0}"#, None)
            .await;
        let err = result.unwrap().unwrap_err();
        assert!(err.contains("no staged writes"));
    }

    #[tokio::test]
    async fn stage_to_nonexistent_transaction_returns_error() {
        let dir = TempDir::new().unwrap();
        let skill = skill_in_tempdir(&dir);

        let result = skill
            .execute(
                "stage_file",
                r#"{"tx_id": 999, "path": "foo.txt", "content": "bar"}"#,
                None,
            )
            .await;
        let err = result.unwrap().unwrap_err();
        assert!(err.contains("not found"));
    }

    #[tokio::test]
    async fn commit_rejects_denied_paths_with_policy() {
        let dir = TempDir::new().unwrap();
        let config = SelfModifyConfig {
            enabled: true,
            allow_paths: vec!["src/**".to_string()],
            deny_paths: vec!["*.key".to_string()],
            ..SelfModifyConfig::default()
        };
        let skill = TransactionSkill::new(dir.path().to_path_buf(), Some(config));

        // Begin and stage a file matching a deny pattern
        skill
            .execute("begin_transaction", r#"{"label": "policy test"}"#, None)
            .await
            .unwrap()
            .unwrap();

        skill
            .execute(
                "stage_file",
                r#"{"tx_id": 0, "path": "secret.key", "content": "private"}"#,
                None,
            )
            .await
            .unwrap()
            .unwrap();

        // Commit should fail because *.key is in deny tier
        let result = skill
            .execute("commit_transaction", r#"{"tx_id": 0}"#, None)
            .await;
        let err = result.unwrap().unwrap_err();
        assert!(
            err.contains("Deny")
                || err.contains("deny")
                || err.contains("blocked")
                || err.contains("policy violation"),
            "unexpected error: {err}"
        );
    }

    #[tokio::test]
    async fn commit_allows_permitted_paths_with_policy() {
        let dir = TempDir::new().unwrap();
        let config = SelfModifyConfig {
            enabled: true,
            allow_paths: vec!["src/**".to_string()],
            deny_paths: vec!["*.key".to_string()],
            ..SelfModifyConfig::default()
        };
        let skill = TransactionSkill::new(dir.path().to_path_buf(), Some(config));

        skill
            .execute(
                "begin_transaction",
                r#"{"label": "allowed path test"}"#,
                None,
            )
            .await
            .unwrap()
            .unwrap();

        skill
            .execute(
                "stage_file",
                r#"{"tx_id": 0, "path": "src/main.rs", "content": "fn main() {}"}"#,
                None,
            )
            .await
            .unwrap()
            .unwrap();

        let result = skill
            .execute("commit_transaction", r#"{"tx_id": 0}"#, None)
            .await;
        let output = result.unwrap().unwrap();
        assert!(output.contains("committed"));
    }
}
