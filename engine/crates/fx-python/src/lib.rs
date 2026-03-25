mod installer;
mod process;
mod runner;
mod venv;

use async_trait::async_trait;
use fx_kernel::act::ToolCacheability;
use fx_kernel::cancellation::CancellationToken;
use fx_llm::ToolDefinition;
use fx_loadable::skill::{Skill, SkillError};
use installer::{PythonInstallArgs, PythonInstaller};
use runner::{PythonRunArgs, PythonRunner};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::path::Path;
use venv::{PackageInfo, VenvManager};

#[derive(Debug, Deserialize)]
struct PythonVenvsArgs {
    action: String,
    #[serde(default)]
    name: Option<String>,
}

#[derive(Debug, Serialize)]
struct VenvListResponse {
    venvs: Vec<String>,
}

#[derive(Debug, Serialize)]
struct VenvDeleteResponse {
    deleted: String,
}

#[derive(Debug, Serialize)]
struct VenvInfoResponse {
    name: String,
    packages: Vec<PackageInfo>,
}

#[derive(Debug, Clone)]
pub struct PythonSkill {
    venv_manager: VenvManager,
    runner: PythonRunner,
    installer: PythonInstaller,
}

impl PythonSkill {
    #[must_use]
    pub fn new(data_dir: &Path) -> Self {
        let venv_root = data_dir.join("venvs");
        let experiments_root = data_dir.join("experiments");
        let venv_manager = VenvManager::new(&venv_root);
        let runner = PythonRunner::new(venv_manager.clone(), experiments_root.clone());
        let installer = PythonInstaller::new(venv_manager.clone(), experiments_root);

        Self {
            venv_manager,
            runner,
            installer,
        }
    }

    async fn handle_run(
        &self,
        arguments: &str,
        cancel: Option<&CancellationToken>,
    ) -> Result<String, SkillError> {
        let args: PythonRunArgs = parse_arguments(arguments)?;
        let result = self.runner.run(args, cancel).await?;
        serialize_response(&result)
    }

    async fn handle_install(
        &self,
        arguments: &str,
        cancel: Option<&CancellationToken>,
    ) -> Result<String, SkillError> {
        let args: PythonInstallArgs = parse_arguments(arguments)?;
        let result = self.installer.install(args, cancel).await?;
        serialize_response(&result)
    }

    async fn handle_venvs(&self, arguments: &str) -> Result<String, SkillError> {
        let args: PythonVenvsArgs = parse_arguments(arguments)?;
        match args.action.as_str() {
            "list" => self.handle_list_venvs().await,
            "delete" => self.handle_delete_venv(&args).await,
            "info" => self.handle_info_venv(&args).await,
            _ => Err(format!("unknown python_venvs action: {}", args.action)),
        }
    }

    async fn handle_list_venvs(&self) -> Result<String, SkillError> {
        let response = VenvListResponse {
            venvs: self.venv_manager.list_venvs().await?,
        };
        serialize_response(&response)
    }

    async fn handle_delete_venv(&self, args: &PythonVenvsArgs) -> Result<String, SkillError> {
        let name = required_venv_name(args)?;
        self.venv_manager.delete_venv(name).await?;
        serialize_response(&VenvDeleteResponse {
            deleted: name.to_string(),
        })
    }

    async fn handle_info_venv(&self, args: &PythonVenvsArgs) -> Result<String, SkillError> {
        let name = required_venv_name(args)?;
        let packages = self.venv_manager.info(name).await?;
        serialize_response(&VenvInfoResponse {
            name: name.to_string(),
            packages,
        })
    }
}

#[async_trait]
impl Skill for PythonSkill {
    fn name(&self) -> &str {
        "python"
    }

    fn description(&self) -> &str {
        "Execute Python code and manage Python virtual environments."
    }

    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        vec![
            python_run_definition(),
            python_install_definition(),
            python_venvs_definition(),
        ]
    }

    fn cacheability(&self, _tool_name: &str) -> ToolCacheability {
        ToolCacheability::SideEffect
    }

    async fn execute(
        &self,
        tool_name: &str,
        arguments: &str,
        cancel: Option<&CancellationToken>,
    ) -> Option<Result<String, SkillError>> {
        match tool_name {
            "python_run" => Some(self.handle_run(arguments, cancel).await),
            "python_install" => Some(self.handle_install(arguments, cancel).await),
            "python_venvs" => Some(self.handle_venvs(arguments).await),
            _ => None,
        }
    }
}

fn python_run_definition() -> ToolDefinition {
    ToolDefinition {
        name: "python_run".to_string(),
        description: "Run Python code inside a named virtual environment and report output, exit code, and generated artifacts.".to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "code": {
                    "type": "string",
                    "description": "Python source code to execute"
                },
                "venv": {
                    "type": "string",
                    "description": "Virtual environment name"
                },
                "timeout_seconds": {
                    "type": "integer",
                    "description": "Execution timeout in seconds",
                    "default": 300
                }
            },
            "required": ["code", "venv"]
        }),
    }
}

fn python_install_definition() -> ToolDefinition {
    ToolDefinition {
        name: "python_install".to_string(),
        description: "Install Python packages into a named virtual environment using pip."
            .to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "packages": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Packages to install"
                },
                "venv": {
                    "type": "string",
                    "description": "Virtual environment name"
                },
                "requirements_file": {
                    "type": ["string", "null"],
                    "description": "Optional requirements file path inside the experiment directory"
                },
                "timeout_seconds": {
                    "type": "integer",
                    "description": "Pip install timeout in seconds",
                    "default": 600
                }
            },
            "required": ["venv"]
        }),
    }
}

fn python_venvs_definition() -> ToolDefinition {
    ToolDefinition {
        name: "python_venvs".to_string(),
        description: "List, inspect, or delete managed Python virtual environments.".to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["list", "delete", "info"],
                    "description": "Virtual environment action to perform"
                },
                "name": {
                    "type": "string",
                    "description": "Virtual environment name for delete or info"
                }
            },
            "required": ["action"]
        }),
    }
}

fn required_venv_name(args: &PythonVenvsArgs) -> Result<&str, SkillError> {
    args.name
        .as_deref()
        .ok_or_else(|| "python_venvs action requires 'name'".to_string())
}

fn parse_arguments<T>(arguments: &str) -> Result<T, SkillError>
where
    T: DeserializeOwned,
{
    serde_json::from_str(arguments).map_err(|error| format!("invalid arguments: {error}"))
}

fn serialize_response<T>(value: &T) -> Result<String, SkillError>
where
    T: Serialize,
{
    serde_json::to_string(value).map_err(|error| format!("serialize response: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn skill_in_tempdir(temp_dir: &TempDir) -> PythonSkill {
        PythonSkill::new(temp_dir.path())
    }

    #[test]
    fn tool_definitions_count() {
        let temp_dir = TempDir::new().expect("tempdir");
        let skill = skill_in_tempdir(&temp_dir);

        assert_eq!(skill.tool_definitions().len(), 3);
    }

    #[tokio::test]
    async fn unknown_tool_returns_none() {
        let temp_dir = TempDir::new().expect("tempdir");
        let skill = skill_in_tempdir(&temp_dir);

        let result = skill.execute("unknown_tool", "{}", None).await;

        assert!(result.is_none());
    }

    #[tokio::test]
    async fn python_run_uses_cancellation_token() {
        let temp_dir = TempDir::new().expect("tempdir");
        let skill = skill_in_tempdir(&temp_dir);
        let token = CancellationToken::new();
        let cancel = token.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            cancel.cancel();
        });

        let result = skill
            .execute(
                "python_run",
                r#"{"code":"import time\ntime.sleep(5)\n","venv":"test"}"#,
                Some(&token),
            )
            .await
            .expect("known tool")
            .expect_err("run should be cancelled");

        assert!(result.contains("cancelled"));
    }
}
