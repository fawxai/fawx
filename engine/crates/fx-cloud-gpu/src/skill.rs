use crate::{CloudGpuProvider, ExecResult, GpuError, Pod, PodConfig};
use async_trait::async_trait;
use fx_kernel::act::ToolCacheability;
use fx_kernel::cancellation::CancellationToken;
use fx_llm::ToolDefinition;
use fx_loadable::{Skill, SkillError};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::{Path, PathBuf};

const GPU_CREATE_TOOL: &str = "gpu_create";
const GPU_LIST_TOOL: &str = "gpu_list";
const GPU_STATUS_TOOL: &str = "gpu_status";
const GPU_STOP_TOOL: &str = "gpu_stop";
const GPU_DESTROY_TOOL: &str = "gpu_destroy";
const GPU_EXEC_TOOL: &str = "gpu_exec";
const GPU_UPLOAD_TOOL: &str = "gpu_upload";
const GPU_DOWNLOAD_TOOL: &str = "gpu_download";

#[derive(Debug)]
pub struct CloudGpuSkill {
    provider: Box<dyn CloudGpuProvider>,
}

impl CloudGpuSkill {
    #[must_use]
    pub fn new(provider: Box<dyn CloudGpuProvider>) -> Self {
        Self { provider }
    }

    fn handles_tool(tool_name: &str) -> bool {
        matches!(
            tool_name,
            GPU_CREATE_TOOL
                | GPU_LIST_TOOL
                | GPU_STATUS_TOOL
                | GPU_STOP_TOOL
                | GPU_DESTROY_TOOL
                | GPU_EXEC_TOOL
                | GPU_UPLOAD_TOOL
                | GPU_DOWNLOAD_TOOL
        )
    }

    async fn execute_tool(
        &self,
        tool_name: &str,
        arguments: &str,
        cancel: Option<&CancellationToken>,
    ) -> Result<String, SkillError> {
        match tool_name {
            GPU_CREATE_TOOL => self.handle_create(arguments).await,
            GPU_LIST_TOOL => self.handle_list(arguments).await,
            GPU_STATUS_TOOL => self.handle_status(arguments).await,
            GPU_STOP_TOOL => self.handle_stop(arguments).await,
            GPU_DESTROY_TOOL => self.handle_destroy(arguments).await,
            GPU_EXEC_TOOL => self.handle_exec(arguments, cancel).await,
            GPU_UPLOAD_TOOL => self.handle_upload(arguments, cancel).await,
            GPU_DOWNLOAD_TOOL => self.handle_download(arguments, cancel).await,
            _ => Err(format!("unknown cloud gpu tool: {tool_name}")),
        }
    }

    async fn handle_create(&self, arguments: &str) -> Result<String, SkillError> {
        let request: GpuCreateRequest = parse_request(arguments)?;
        let pod = self
            .provider
            .create_pod(request.config)
            .await
            .map_err(serialize_gpu_error)?;
        serialize_response(&GpuCreateResponse { pod })
    }

    async fn handle_list(&self, arguments: &str) -> Result<String, SkillError> {
        let _: GpuListRequest = parse_request(arguments)?;
        let pods = self
            .provider
            .list_pods()
            .await
            .map_err(serialize_gpu_error)?;
        serialize_response(&GpuListResponse { pods })
    }

    async fn handle_status(&self, arguments: &str) -> Result<String, SkillError> {
        let request: GpuStatusRequest = parse_request(arguments)?;
        let pod = self
            .provider
            .pod_status(&request.pod_id)
            .await
            .map_err(serialize_gpu_error)?;
        serialize_response(&GpuStatusResponse { pod })
    }

    async fn handle_stop(&self, arguments: &str) -> Result<String, SkillError> {
        let request: GpuStopRequest = parse_request(arguments)?;
        self.provider
            .stop_pod(&request.pod_id)
            .await
            .map_err(serialize_gpu_error)?;
        serialize_response(&GpuStopResponse {
            pod_id: request.pod_id,
            stopped: true,
        })
    }

    async fn handle_destroy(&self, arguments: &str) -> Result<String, SkillError> {
        let request: GpuDestroyRequest = parse_request(arguments)?;
        self.provider
            .destroy_pod(&request.pod_id)
            .await
            .map_err(serialize_gpu_error)?;
        serialize_response(&GpuDestroyResponse {
            pod_id: request.pod_id,
            destroyed: true,
        })
    }

    async fn handle_exec(
        &self,
        arguments: &str,
        cancel: Option<&CancellationToken>,
    ) -> Result<String, SkillError> {
        let request: GpuExecRequest = parse_request(arguments)?;
        let result = self
            .provider
            .exec(
                &request.pod_id,
                &request.command,
                request.timeout_seconds,
                cancel,
            )
            .await
            .map_err(serialize_gpu_error)?;
        serialize_response(&GpuExecResponse { result })
    }

    async fn handle_upload(
        &self,
        arguments: &str,
        cancel: Option<&CancellationToken>,
    ) -> Result<String, SkillError> {
        let request: GpuUploadRequest = parse_request(arguments)?;
        self.provider
            .upload(
                &request.pod_id,
                &request.local_path,
                &request.remote_path,
                cancel,
            )
            .await
            .map_err(serialize_gpu_error)?;
        serialize_response(&GpuUploadResponse {
            pod_id: request.pod_id,
            local_path: path_to_string(&request.local_path),
            remote_path: request.remote_path,
            uploaded: true,
        })
    }

    async fn handle_download(
        &self,
        arguments: &str,
        cancel: Option<&CancellationToken>,
    ) -> Result<String, SkillError> {
        let request: GpuDownloadRequest = parse_request(arguments)?;
        self.provider
            .download(
                &request.pod_id,
                &request.remote_path,
                &request.local_path,
                cancel,
            )
            .await
            .map_err(serialize_gpu_error)?;
        serialize_response(&GpuDownloadResponse {
            pod_id: request.pod_id,
            remote_path: request.remote_path,
            local_path: path_to_string(&request.local_path),
            downloaded: true,
        })
    }
}

#[async_trait]
impl Skill for CloudGpuSkill {
    fn name(&self) -> &str {
        "cloud_gpu"
    }

    fn description(&self) -> &str {
        "Manage cloud GPU pods through a configured provider."
    }

    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        cloud_gpu_tool_definitions()
    }

    fn cacheability(&self, tool_name: &str) -> ToolCacheability {
        match tool_name {
            GPU_CREATE_TOOL | GPU_STOP_TOOL | GPU_DESTROY_TOOL | GPU_EXEC_TOOL
            | GPU_UPLOAD_TOOL | GPU_DOWNLOAD_TOOL => ToolCacheability::SideEffect,
            GPU_LIST_TOOL | GPU_STATUS_TOOL => ToolCacheability::NeverCache,
            _ => ToolCacheability::NeverCache,
        }
    }

    async fn execute(
        &self,
        tool_name: &str,
        arguments: &str,
        cancel: Option<&CancellationToken>,
    ) -> Option<Result<String, SkillError>> {
        if !Self::handles_tool(tool_name) {
            return None;
        }
        Some(self.execute_tool(tool_name, arguments, cancel).await)
    }
}

#[derive(Debug, Deserialize)]
struct GpuCreateRequest {
    config: PodConfig,
}

#[derive(Debug, Serialize, Deserialize)]
struct GpuCreateResponse {
    pod: Pod,
}

#[derive(Debug, Default, Deserialize)]
struct GpuListRequest {}

#[derive(Debug, Serialize, Deserialize)]
struct GpuListResponse {
    pods: Vec<Pod>,
}

#[derive(Debug, Deserialize)]
struct GpuStatusRequest {
    pod_id: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct GpuStatusResponse {
    pod: Pod,
}

#[derive(Debug, Deserialize)]
struct GpuStopRequest {
    pod_id: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct GpuStopResponse {
    pod_id: String,
    stopped: bool,
}

#[derive(Debug, Deserialize)]
struct GpuDestroyRequest {
    pod_id: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct GpuDestroyResponse {
    pod_id: String,
    destroyed: bool,
}

#[derive(Debug, Deserialize)]
struct GpuExecRequest {
    pod_id: String,
    command: String,
    timeout_seconds: u32,
}

#[derive(Debug, Serialize, Deserialize)]
struct GpuExecResponse {
    result: ExecResult,
}

#[derive(Debug, Deserialize)]
struct GpuUploadRequest {
    pod_id: String,
    local_path: PathBuf,
    remote_path: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct GpuUploadResponse {
    pod_id: String,
    local_path: String,
    remote_path: String,
    uploaded: bool,
}

#[derive(Debug, Deserialize)]
struct GpuDownloadRequest {
    pod_id: String,
    remote_path: String,
    local_path: PathBuf,
}

#[derive(Debug, Serialize, Deserialize)]
struct GpuDownloadResponse {
    pod_id: String,
    remote_path: String,
    local_path: String,
    downloaded: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct ErrorResponse {
    error: String,
}

fn parse_request<T>(arguments: &str) -> Result<T, SkillError>
where
    T: DeserializeOwned,
{
    serde_json::from_str(arguments).map_err(|error| format!("invalid arguments: {error}"))
}

fn serialize_response<T>(response: &T) -> Result<String, SkillError>
where
    T: Serialize,
{
    serde_json::to_string(response).map_err(|error| format!("serialization failed: {error}"))
}

fn serialize_gpu_error(error: GpuError) -> SkillError {
    serialize_error_message(error.to_string())
}

fn serialize_error_message(message: String) -> SkillError {
    let response = ErrorResponse { error: message };
    match serde_json::to_string(&response) {
        Ok(json) => json,
        Err(error) => format!(
            "serialization failed: {error}; original error: {}",
            response.error
        ),
    }
}

fn path_to_string(path: &Path) -> String {
    path.display().to_string()
}

fn cloud_gpu_tool_definitions() -> Vec<ToolDefinition> {
    vec![
        gpu_create_definition(),
        gpu_list_definition(),
        gpu_status_definition(),
        gpu_stop_definition(),
        gpu_destroy_definition(),
        gpu_exec_definition(),
        gpu_upload_definition(),
        gpu_download_definition(),
    ]
}

fn gpu_create_definition() -> ToolDefinition {
    tool_definition(
        GPU_CREATE_TOOL,
        "Create a new cloud GPU pod from a pod configuration.",
        json!({
            "type": "object",
            "properties": {
                "config": pod_config_schema()
            },
            "required": ["config"]
        }),
    )
}

fn gpu_list_definition() -> ToolDefinition {
    tool_definition(
        GPU_LIST_TOOL,
        "List all cloud GPU pods for the configured provider.",
        empty_object_schema(),
    )
}

fn gpu_status_definition() -> ToolDefinition {
    tool_definition(
        GPU_STATUS_TOOL,
        "Get the current status and connection details for a pod.",
        pod_id_schema(),
    )
}

fn gpu_stop_definition() -> ToolDefinition {
    tool_definition(
        GPU_STOP_TOOL,
        "Stop a running cloud GPU pod.",
        pod_id_schema(),
    )
}

fn gpu_destroy_definition() -> ToolDefinition {
    tool_definition(
        GPU_DESTROY_TOOL,
        "Destroy a cloud GPU pod permanently.",
        pod_id_schema(),
    )
}

fn gpu_exec_definition() -> ToolDefinition {
    tool_definition(
        GPU_EXEC_TOOL,
        "Execute a command inside a cloud GPU pod.",
        json!({
            "type": "object",
            "properties": {
                "pod_id": { "type": "string", "description": "Pod identifier" },
                "command": { "type": "string", "description": "Command to execute" },
                "timeout_seconds": {
                    "type": "integer",
                    "description": "Command timeout in seconds"
                }
            },
            "required": ["pod_id", "command", "timeout_seconds"]
        }),
    )
}

fn gpu_upload_definition() -> ToolDefinition {
    tool_definition(
        GPU_UPLOAD_TOOL,
        "Upload a local file to a cloud GPU pod.",
        json!({
            "type": "object",
            "properties": {
                "pod_id": { "type": "string", "description": "Pod identifier" },
                "local_path": { "type": "string", "description": "Source file path" },
                "remote_path": { "type": "string", "description": "Destination path on the pod" }
            },
            "required": ["pod_id", "local_path", "remote_path"]
        }),
    )
}

fn gpu_download_definition() -> ToolDefinition {
    tool_definition(
        GPU_DOWNLOAD_TOOL,
        "Download a file from a cloud GPU pod to the local machine.",
        json!({
            "type": "object",
            "properties": {
                "pod_id": { "type": "string", "description": "Pod identifier" },
                "remote_path": { "type": "string", "description": "Source path on the pod" },
                "local_path": { "type": "string", "description": "Destination file path" }
            },
            "required": ["pod_id", "remote_path", "local_path"]
        }),
    )
}

fn tool_definition(name: &str, description: &str, parameters: serde_json::Value) -> ToolDefinition {
    ToolDefinition {
        name: name.to_string(),
        description: description.to_string(),
        parameters,
    }
}

fn empty_object_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {},
        "required": []
    })
}

fn pod_id_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "pod_id": { "type": "string", "description": "Pod identifier" }
        },
        "required": ["pod_id"]
    })
}

fn pod_config_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "name": { "type": "string" },
            "gpu": gpu_type_schema(),
            "gpu_count": { "type": "integer" },
            "image": { "type": "string" },
            "disk_gb": { "type": "integer" },
            "env": {
                "type": "object",
                "additionalProperties": { "type": "string" }
            }
        },
        "required": ["name", "gpu", "gpu_count", "image", "disk_gb"]
    })
}

fn gpu_type_schema() -> serde_json::Value {
    json!({
        "oneOf": [
            {
                "type": "string",
                "enum": ["Rtx3090", "Rtx4090", "A100_80gb", "H100_80gb"]
            },
            {
                "type": "object",
                "properties": {
                    "Custom": { "type": "string" }
                },
                "required": ["Custom"]
            }
        ]
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{GpuType, PodStatus};
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    #[derive(Debug)]
    struct MockProvider {
        calls: Arc<Mutex<Vec<String>>>,
        destroy_missing: bool,
    }

    impl MockProvider {
        fn new(destroy_missing: bool) -> (Self, Arc<Mutex<Vec<String>>>) {
            let calls = Arc::new(Mutex::new(Vec::new()));
            (
                Self {
                    calls: Arc::clone(&calls),
                    destroy_missing,
                },
                calls,
            )
        }

        fn record_call(&self, call: impl Into<String>) {
            self.calls.lock().unwrap().push(call.into());
        }

        fn cancel_suffix(cancel: Option<&CancellationToken>) -> String {
            cancel.map_or_else(String::new, |token| {
                format!(":cancelled:{}", token.is_cancelled())
            })
        }
    }

    #[async_trait]
    impl CloudGpuProvider for MockProvider {
        fn provider_name(&self) -> &str {
            "mock"
        }

        async fn create_pod(&self, config: PodConfig) -> Result<Pod, GpuError> {
            self.record_call(format!("create:{}", config.name));
            Ok(sample_pod())
        }

        async fn list_pods(&self) -> Result<Vec<Pod>, GpuError> {
            self.record_call("list");
            Ok(vec![sample_pod()])
        }

        async fn pod_status(&self, pod_id: &str) -> Result<Pod, GpuError> {
            self.record_call(format!("status:{pod_id}"));
            Ok(sample_pod())
        }

        async fn stop_pod(&self, pod_id: &str) -> Result<(), GpuError> {
            self.record_call(format!("stop:{pod_id}"));
            Ok(())
        }

        async fn destroy_pod(&self, pod_id: &str) -> Result<(), GpuError> {
            self.record_call(format!("destroy:{pod_id}"));
            if self.destroy_missing {
                Err(GpuError::PodNotFound(pod_id.to_string()))
            } else {
                Ok(())
            }
        }

        async fn exec(
            &self,
            pod_id: &str,
            command: &str,
            timeout_seconds: u32,
            cancel: Option<&CancellationToken>,
        ) -> Result<ExecResult, GpuError> {
            self.record_call(format!(
                "exec:{pod_id}:{command}:{timeout_seconds}{}",
                Self::cancel_suffix(cancel)
            ));
            Ok(sample_exec_result())
        }

        async fn upload(
            &self,
            pod_id: &str,
            local_path: &std::path::Path,
            remote_path: &str,
            cancel: Option<&CancellationToken>,
        ) -> Result<(), GpuError> {
            self.record_call(format!(
                "upload:{pod_id}:{}:{remote_path}{}",
                local_path.display(),
                Self::cancel_suffix(cancel)
            ));
            Ok(())
        }

        async fn download(
            &self,
            pod_id: &str,
            remote_path: &str,
            local_path: &std::path::Path,
            cancel: Option<&CancellationToken>,
        ) -> Result<(), GpuError> {
            self.record_call(format!(
                "download:{pod_id}:{remote_path}:{}{}",
                local_path.display(),
                Self::cancel_suffix(cancel)
            ));
            Ok(())
        }
    }

    fn sample_config() -> PodConfig {
        let mut env = HashMap::new();
        env.insert("TOKEN".to_string(), "abc123".to_string());
        PodConfig {
            name: "trainer".to_string(),
            gpu: GpuType::Rtx4090,
            gpu_count: 1,
            image: "nvidia/cuda:12.0.0-runtime-ubuntu22.04".to_string(),
            disk_gb: 200,
            env,
        }
    }

    fn sample_pod() -> Pod {
        Pod {
            id: "pod-1".to_string(),
            status: PodStatus::Running,
            ssh_host: "gpu.example.com".to_string(),
            ssh_port: 22,
            gpu: GpuType::Rtx4090,
            cost_per_hour: 1.5,
        }
    }

    fn sample_exec_result() -> ExecResult {
        ExecResult {
            stdout: "GPU ready".to_string(),
            stderr: String::new(),
            exit_code: 0,
            duration_ms: 125,
        }
    }

    fn test_skill(destroy_missing: bool) -> (CloudGpuSkill, Arc<Mutex<Vec<String>>>) {
        let (provider, calls) = MockProvider::new(destroy_missing);
        (CloudGpuSkill::new(Box::new(provider)), calls)
    }

    async fn execute_tool(
        skill: &CloudGpuSkill,
        tool_name: &str,
        arguments: serde_json::Value,
    ) -> Result<String, SkillError> {
        execute_tool_with_cancel(skill, tool_name, arguments, None).await
    }

    async fn execute_tool_with_cancel(
        skill: &CloudGpuSkill,
        tool_name: &str,
        arguments: serde_json::Value,
        cancel: Option<&CancellationToken>,
    ) -> Result<String, SkillError> {
        skill
            .execute(tool_name, &arguments.to_string(), cancel)
            .await
            .expect("tool should be handled")
    }

    #[tokio::test]
    async fn mock_provider_returns_canned_responses() {
        let provider = MockProvider::new(false).0;
        let pod = provider
            .create_pod(sample_config())
            .await
            .expect("create pod");
        let pods = provider.list_pods().await.expect("list pods");
        let exec = provider
            .exec("pod-1", "nvidia-smi", 30, None)
            .await
            .expect("exec command");

        assert_eq!(pod.id, "pod-1");
        assert_eq!(pods.len(), 1);
        assert_eq!(exec.stdout, "GPU ready");
    }

    async fn exercise_lifecycle_tools(skill: &CloudGpuSkill) {
        let _ = execute_tool(skill, GPU_CREATE_TOOL, json!({ "config": sample_config() })).await;
        let _ = execute_tool(skill, GPU_LIST_TOOL, json!({})).await;
        let _ = execute_tool(skill, GPU_STATUS_TOOL, json!({ "pod_id": "pod-1" })).await;
        let _ = execute_tool(skill, GPU_STOP_TOOL, json!({ "pod_id": "pod-1" })).await;
        let _ = execute_tool(skill, GPU_DESTROY_TOOL, json!({ "pod_id": "pod-1" })).await;
        let _ = execute_tool(
            skill,
            GPU_EXEC_TOOL,
            json!({ "pod_id": "pod-1", "command": "nvidia-smi", "timeout_seconds": 30 }),
        )
        .await;
    }

    async fn exercise_transfer_tools(skill: &CloudGpuSkill) {
        let _ = execute_tool(
            skill,
            GPU_UPLOAD_TOOL,
            json!({
                "pod_id": "pod-1",
                "local_path": "/tmp/input.txt",
                "remote_path": "/workspace/input.txt"
            }),
        )
        .await;
        let _ = execute_tool(
            skill,
            GPU_DOWNLOAD_TOOL,
            json!({
                "pod_id": "pod-1",
                "remote_path": "/workspace/output.txt",
                "local_path": "/tmp/output.txt"
            }),
        )
        .await;
    }

    fn expected_calls() -> Vec<&'static str> {
        vec![
            "create:trainer",
            "list",
            "status:pod-1",
            "stop:pod-1",
            "destroy:pod-1",
            "exec:pod-1:nvidia-smi:30",
            "upload:pod-1:/tmp/input.txt:/workspace/input.txt",
            "download:pod-1:/workspace/output.txt:/tmp/output.txt",
        ]
    }

    #[tokio::test]
    async fn cloud_gpu_skill_routes_to_correct_provider_method() {
        let (skill, calls) = test_skill(false);

        exercise_lifecycle_tools(&skill).await;
        exercise_transfer_tools(&skill).await;

        assert_eq!(calls.lock().unwrap().clone(), expected_calls());
    }

    #[tokio::test]
    async fn exec_upload_and_download_forward_cancellation_token() {
        let (skill, calls) = test_skill(false);
        let cancel = CancellationToken::new();
        cancel.cancel();

        let _ = execute_tool_with_cancel(
            &skill,
            GPU_EXEC_TOOL,
            json!({ "pod_id": "pod-1", "command": "nvidia-smi", "timeout_seconds": 30 }),
            Some(&cancel),
        )
        .await;
        let _ = execute_tool_with_cancel(
            &skill,
            GPU_UPLOAD_TOOL,
            json!({
                "pod_id": "pod-1",
                "local_path": "/tmp/input.txt",
                "remote_path": "/workspace/input.txt"
            }),
            Some(&cancel),
        )
        .await;
        let _ = execute_tool_with_cancel(
            &skill,
            GPU_DOWNLOAD_TOOL,
            json!({
                "pod_id": "pod-1",
                "remote_path": "/workspace/output.txt",
                "local_path": "/tmp/output.txt"
            }),
            Some(&cancel),
        )
        .await;

        assert_eq!(
            calls.lock().unwrap().clone(),
            vec![
                "exec:pod-1:nvidia-smi:30:cancelled:true",
                "upload:pod-1:/tmp/input.txt:/workspace/input.txt:cancelled:true",
                "download:pod-1:/workspace/output.txt:/tmp/output.txt:cancelled:true",
            ]
        );
    }

    #[tokio::test]
    async fn unknown_tool_returns_none() {
        let (skill, _) = test_skill(false);
        let result = skill.execute("gpu_unknown", "{}", None).await;

        assert!(result.is_none());
    }

    #[tokio::test]
    async fn error_serialization_returns_json_error_payload() {
        let (skill, _) = test_skill(true);
        let result = execute_tool(&skill, GPU_DESTROY_TOOL, json!({ "pod_id": "pod-404" }))
            .await
            .expect_err("destroy should fail");
        let payload: ErrorResponse = serde_json::from_str(&result).expect("error json");

        assert_eq!(payload.error, "pod not found: pod-404");
    }

    #[test]
    fn gpu_create_schema_allows_omitting_env() {
        let schema = gpu_create_definition().parameters;
        let required = schema["properties"]["config"]["required"]
            .as_array()
            .expect("config schema should list required fields");
        let required_fields: Vec<&str> = required
            .iter()
            .map(|field| field.as_str().expect("required field should be a string"))
            .collect();

        assert!(!required_fields.contains(&"env"));
    }

    #[test]
    fn tool_definitions_match_expected_count_and_names() {
        let (skill, _) = test_skill(false);
        let definitions = skill.tool_definitions();
        let names: Vec<&str> = definitions
            .iter()
            .map(|definition| definition.name.as_str())
            .collect();

        assert_eq!(definitions.len(), 8);
        assert_eq!(skill.name(), "cloud_gpu");
        assert_eq!(
            names,
            vec![
                GPU_CREATE_TOOL,
                GPU_LIST_TOOL,
                GPU_STATUS_TOOL,
                GPU_STOP_TOOL,
                GPU_DESTROY_TOOL,
                GPU_EXEC_TOOL,
                GPU_UPLOAD_TOOL,
                GPU_DOWNLOAD_TOOL,
            ]
        );
    }
}
