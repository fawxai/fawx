use std::path::{Path, PathBuf};

use crate::skill_manifests::{update_skill_capabilities, SkillManifestError};
use crate::state::HttpState;
use crate::types::ErrorBody;
use axum::extract::{Json, Path as AxumPath, Query, State};
use axum::http::StatusCode;
use fx_marketplace::{InstallResult, MarketplaceError, SkillEntry};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

const MARKETPLACE_NOT_CONNECTED_MESSAGE: &str = "Marketplace not yet connected";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MarketplaceSkillSummary {
    pub name: String,
    pub title: String,
    pub description: String,
    pub publisher: String,
    pub signed: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct SkillSearchResponse {
    pub query: String,
    pub skills: Vec<MarketplaceSkillSummary>,
    pub total: usize,
    pub marketplace_available: bool,
    pub message: String,
}

#[derive(Debug, Deserialize)]
pub struct SearchQuery {
    #[serde(default)]
    pub q: String,
}

#[derive(Debug, Deserialize)]
pub struct InstallSkillRequest {
    /// Skill name to install.
    pub name: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct InstallSkillResponse {
    pub name: String,
    pub version: String,
    pub size_bytes: u64,
    pub installed: bool,
}

#[derive(Debug, Deserialize)]
pub struct UpdateSkillPermissionsRequest {
    #[serde(default)]
    pub capabilities: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UpdateSkillPermissionsResponse {
    pub updated: bool,
    pub name: String,
    pub capabilities: Vec<String>,
    pub restart_required: bool,
}

pub async fn handle_search_skills(
    State(state): State<HttpState>,
    Query(params): Query<SearchQuery>,
) -> Json<SkillSearchResponse> {
    Json(search_skills_response(state.data_dir.clone(), params.q, search_marketplace).await)
}

pub async fn handle_install_skill(
    State(state): State<HttpState>,
    Json(request): Json<InstallSkillRequest>,
) -> Result<Json<InstallSkillResponse>, (StatusCode, Json<ErrorBody>)> {
    install_skill_response(
        state.data_dir.clone(),
        request.name,
        install_marketplace_skill,
    )
    .await
    .map(Json)
}

pub async fn handle_remove_skill(
    State(state): State<HttpState>,
    AxumPath(name): AxumPath<String>,
) -> Result<Json<Value>, (StatusCode, Json<ErrorBody>)> {
    remove_skill_response(state.data_dir.clone(), name)
        .await
        .map(Json)
}

pub async fn handle_update_skill_permissions(
    State(state): State<HttpState>,
    AxumPath(name): AxumPath<String>,
    Json(request): Json<UpdateSkillPermissionsRequest>,
) -> Result<Json<UpdateSkillPermissionsResponse>, (StatusCode, Json<ErrorBody>)> {
    let capabilities =
        update_skill_capabilities(&state.data_dir.join("skills"), &name, &request.capabilities)
            .map_err(skill_manifest_error)?;

    Ok(Json(UpdateSkillPermissionsResponse {
        updated: true,
        name,
        capabilities,
        restart_required: true,
    }))
}

async fn search_skills_response<F>(
    data_dir: PathBuf,
    query: String,
    search_fn: F,
) -> SkillSearchResponse
where
    F: FnOnce(&Path, &str) -> Result<Vec<SkillEntry>, MarketplaceError> + Send + 'static,
{
    let query_for_error = query.clone();
    match tokio::task::spawn_blocking(move || {
        let entries = search_fn(&data_dir, &query)?;
        Ok::<SkillSearchResponse, MarketplaceError>(build_search_response(query, entries))
    })
    .await
    {
        Ok(Ok(response)) => response,
        Ok(Err(error)) => {
            tracing::error!(error = %error, "Marketplace search failed");
            unavailable_search_response(query_for_error, error.to_string())
        }
        Err(error) => {
            tracing::error!(error = %error, "Marketplace search task failed");
            unavailable_search_response(query_for_error, error.to_string())
        }
    }
}

fn search_marketplace(data_dir: &Path, query: &str) -> Result<Vec<SkillEntry>, MarketplaceError> {
    let config = fx_marketplace::default_config(data_dir)?;
    fx_marketplace::search(&config, query)
}

fn build_search_response(query: String, entries: Vec<SkillEntry>) -> SkillSearchResponse {
    let skills: Vec<_> = entries.into_iter().map(map_skill_entry).collect();
    SkillSearchResponse {
        query,
        total: skills.len(),
        skills,
        marketplace_available: true,
        message: String::new(),
    }
}

fn map_skill_entry(entry: SkillEntry) -> MarketplaceSkillSummary {
    MarketplaceSkillSummary {
        title: title_case_skill_name(&entry.name),
        name: entry.name,
        description: entry.description,
        publisher: entry.author,
        signed: true,
    }
}

fn title_case_skill_name(name: &str) -> String {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return String::new();
    };

    let mut title = String::new();
    title.extend(first.to_uppercase());
    title.push_str(&chars.as_str().to_lowercase());
    title
}

fn unavailable_search_response(query: String, message: String) -> SkillSearchResponse {
    let message = if message.is_empty() {
        MARKETPLACE_NOT_CONNECTED_MESSAGE.to_string()
    } else {
        message
    };

    SkillSearchResponse {
        query,
        skills: vec![],
        total: 0,
        marketplace_available: false,
        message,
    }
}

async fn install_skill_response<F>(
    data_dir: PathBuf,
    name: String,
    install_fn: F,
) -> Result<InstallSkillResponse, (StatusCode, Json<ErrorBody>)>
where
    F: FnOnce(&Path, &str) -> Result<InstallResult, MarketplaceError> + Send + 'static,
{
    match tokio::task::spawn_blocking(move || {
        let result = install_fn(&data_dir, &name)?;
        Ok::<InstallSkillResponse, MarketplaceError>(InstallSkillResponse::from(result))
    })
    .await
    {
        Ok(Ok(response)) => Ok(response),
        Ok(Err(error)) => {
            tracing::error!(error = %error, "Marketplace install failed");
            Err(marketplace_error(error))
        }
        Err(error) => {
            tracing::error!(error = %error, "Marketplace install task failed");
            Err(internal_error(error.to_string()))
        }
    }
}

fn install_marketplace_skill(
    data_dir: &Path,
    name: &str,
) -> Result<InstallResult, MarketplaceError> {
    let config = fx_marketplace::default_config(data_dir)?;
    fx_marketplace::install(&config, name)
}

impl From<InstallResult> for InstallSkillResponse {
    fn from(result: InstallResult) -> Self {
        Self {
            name: result.name,
            version: result.version,
            size_bytes: result.size_bytes,
            installed: true,
        }
    }
}

async fn remove_skill_response(
    data_dir: PathBuf,
    name: String,
) -> Result<Value, (StatusCode, Json<ErrorBody>)> {
    match tokio::task::spawn_blocking(move || remove_skill_directory(&data_dir, &name)).await {
        Ok(Ok(response)) => Ok(response),
        Ok(Err(error)) => {
            let Json(body) = &error.1;
            tracing::error!(
                status = %error.0,
                error = %body.error,
                "Marketplace remove failed"
            );
            Err(error)
        }
        Err(error) => {
            tracing::error!(error = %error, "Marketplace remove task failed");
            Err(internal_error(error.to_string()))
        }
    }
}

fn remove_skill_directory(
    data_dir: &Path,
    name: &str,
) -> Result<Value, (StatusCode, Json<ErrorBody>)> {
    fx_marketplace::validate_skill_name(name)
        .map_err(|error| validation_error(error.to_string()))?;

    let skills_dir = data_dir.join("skills");
    let skill_dir = skills_dir.join(name);
    ensure_skill_exists(&skill_dir, name)?;
    ensure_skill_dir_within_skills_dir(&skills_dir, &skill_dir)?;
    std::fs::remove_dir_all(&skill_dir)
        .map_err(|error| internal_error(format!("failed to remove skill '{name}': {error}")))?;

    Ok(json!({ "removed": true, "name": name }))
}

fn ensure_skill_exists(skill_dir: &Path, name: &str) -> Result<(), (StatusCode, Json<ErrorBody>)> {
    if skill_dir
        .try_exists()
        .map_err(|error| internal_error(format!("failed to access skill '{name}': {error}")))?
    {
        return Ok(());
    }
    Err(skill_not_found(name.to_string()))
}

fn ensure_skill_dir_within_skills_dir(
    skills_dir: &Path,
    skill_dir: &Path,
) -> Result<(), (StatusCode, Json<ErrorBody>)> {
    let canonical_skill_dir = std::fs::canonicalize(skill_dir).map_err(|error| {
        tracing::error!(
            error = %error,
            skill_dir = %skill_dir.display(),
            "Failed to resolve skill directory"
        );
        invalid_skill_directory()
    })?;
    let canonical_skills_dir = std::fs::canonicalize(skills_dir).map_err(|error| {
        tracing::error!(
            error = %error,
            skills_dir = %skills_dir.display(),
            "Failed to resolve skills directory"
        );
        invalid_skill_directory()
    })?;
    if canonical_skill_dir.starts_with(&canonical_skills_dir) {
        return Ok(());
    }

    tracing::error!(
        skill_dir = %skill_dir.display(),
        skills_dir = %skills_dir.display(),
        canonical_skill_dir = %canonical_skill_dir.display(),
        canonical_skills_dir = %canonical_skills_dir.display(),
        "Skill directory outside allowed path"
    );
    Err(skill_directory_outside_allowed_path())
}

fn marketplace_error(error: MarketplaceError) -> (StatusCode, Json<ErrorBody>) {
    let status = match &error {
        MarketplaceError::SkillNotFound(_) => StatusCode::NOT_FOUND,
        MarketplaceError::SignatureInvalid(_) | MarketplaceError::ManifestInvalid(_) => {
            StatusCode::UNPROCESSABLE_ENTITY
        }
        MarketplaceError::InvalidIndex(_) | MarketplaceError::NetworkError(_) => {
            StatusCode::BAD_GATEWAY
        }
        MarketplaceError::InstallError(_) | MarketplaceError::InsecureRegistry(_) => {
            StatusCode::INTERNAL_SERVER_ERROR
        }
    };

    (
        status,
        Json(ErrorBody {
            error: error.to_string(),
        }),
    )
}

fn validation_error(error: String) -> (StatusCode, Json<ErrorBody>) {
    (StatusCode::BAD_REQUEST, Json(ErrorBody { error }))
}

fn internal_error(error: String) -> (StatusCode, Json<ErrorBody>) {
    (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorBody { error }))
}

fn invalid_skill_directory() -> (StatusCode, Json<ErrorBody>) {
    internal_error("Invalid skill directory".to_string())
}

fn skill_directory_outside_allowed_path() -> (StatusCode, Json<ErrorBody>) {
    internal_error("Skill directory outside allowed path".to_string())
}

fn skill_not_found(name: String) -> (StatusCode, Json<ErrorBody>) {
    (
        StatusCode::NOT_FOUND,
        Json(ErrorBody {
            error: format!("Skill '{name}' not found"),
        }),
    )
}

fn skill_manifest_error(error: SkillManifestError) -> (StatusCode, Json<ErrorBody>) {
    let status = match &error {
        SkillManifestError::NotFound(_) => StatusCode::NOT_FOUND,
        SkillManifestError::Invalid(_) => StatusCode::BAD_REQUEST,
        SkillManifestError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
    };

    (
        status,
        Json(ErrorBody {
            error: error.message().to_string(),
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use fx_marketplace::MarketplaceError;
    use std::fs;
    use std::path::PathBuf;
    use tempfile::TempDir;

    #[cfg(unix)]
    fn create_dir_symlink(target: &Path, link: &Path) {
        std::os::unix::fs::symlink(target, link).expect("create symlink");
    }

    fn sample_skill_entry(name: &str) -> SkillEntry {
        SkillEntry {
            name: name.to_string(),
            version: "1.0.0".to_string(),
            description: format!("{name} description"),
            author: "Fawx Team".to_string(),
            capabilities: vec!["network".to_string()],
            size_bytes: Some(1024),
        }
    }

    fn sample_install_result() -> InstallResult {
        InstallResult {
            name: "weather".to_string(),
            version: "1.2.3".to_string(),
            size_bytes: 4096,
            install_path: PathBuf::from("/tmp/fawx/skills/weather"),
        }
    }

    #[test]
    fn search_response_serializes() {
        let response = SkillSearchResponse {
            query: "portfolio".into(),
            skills: vec![],
            total: 0,
            marketplace_available: false,
            message: MARKETPLACE_NOT_CONNECTED_MESSAGE.into(),
        };

        let json = serde_json::to_value(response).unwrap();

        assert_eq!(json["query"], "portfolio");
        assert_eq!(json["total"], 0);
        assert_eq!(json["marketplace_available"], false);
    }

    #[test]
    fn skill_summary_serializes() {
        let skill = MarketplaceSkillSummary {
            name: "portfolio-tracker".into(),
            title: "Portfolio Tracker".into(),
            description: "Track holdings and prices.".into(),
            publisher: "fawx-ai".into(),
            signed: true,
        };

        let json = serde_json::to_value(skill).unwrap();

        assert_eq!(json["name"], "portfolio-tracker");
        assert_eq!(json["signed"], true);
    }

    #[test]
    fn install_request_deserializes() {
        let json = r#"{"name":"portfolio-tracker"}"#;
        let request: InstallSkillRequest = serde_json::from_str(json).unwrap();

        assert_eq!(request.name, "portfolio-tracker");
    }

    #[test]
    fn install_response_serializes_without_install_path() {
        let json =
            serde_json::to_value(InstallSkillResponse::from(sample_install_result())).unwrap();

        assert_eq!(json["name"], "weather");
        assert_eq!(json["installed"], true);
        assert_eq!(json.get("install_path"), None);
    }

    #[tokio::test]
    async fn search_response_maps_marketplace_entries() {
        let response = search_skills_response(PathBuf::new(), String::new(), |_, query| {
            assert!(query.is_empty());
            Ok(vec![
                sample_skill_entry("weather"),
                sample_skill_entry("web-fetch"),
            ])
        })
        .await;

        assert!(response.marketplace_available);
        assert_eq!(response.total, 2);
        assert_eq!(response.skills[0].title, "Weather");
        assert_eq!(response.skills[1].title, "Web-fetch");
        assert!(response.skills.iter().all(|skill| skill.signed));
        assert!(response.message.is_empty());
    }

    #[tokio::test]
    async fn search_response_returns_error_message_when_marketplace_fails() {
        let response = search_skills_response(PathBuf::new(), "weather".into(), |_, _| {
            Err(MarketplaceError::NetworkError(
                "registry unavailable".into(),
            ))
        })
        .await;

        assert_eq!(response.query, "weather");
        assert!(response.skills.is_empty());
        assert!(!response.marketplace_available);
        assert_eq!(response.message, "network error: registry unavailable");
    }

    #[tokio::test]
    async fn search_response_handles_blocking_task_panics() {
        let response = search_skills_response(PathBuf::new(), "weather".into(), |_, _| {
            panic!("boom from search")
        })
        .await;

        assert_eq!(response.query, "weather");
        assert!(!response.marketplace_available);
        assert!(response.message.contains("panic") || !response.message.is_empty());
    }

    #[tokio::test]
    async fn install_response_maps_success() {
        let response = install_skill_response(PathBuf::new(), "weather".into(), |_, name| {
            assert_eq!(name, "weather");
            Ok(sample_install_result())
        })
        .await
        .expect("install should succeed");

        assert_eq!(response.name, "weather");
        assert_eq!(response.version, "1.2.3");
        assert_eq!(response.size_bytes, 4096);
        assert!(response.installed);
    }

    #[tokio::test]
    async fn install_response_returns_status_when_marketplace_fails() {
        let error = install_skill_response(PathBuf::new(), "weather".into(), |_, _| {
            Err(MarketplaceError::InstallError("disk full".into()))
        })
        .await
        .expect_err("install should fail");

        let Json(body) = &error.1;

        assert_eq!(error.0, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(body.error, "install error: disk full");
    }

    #[test]
    fn marketplace_error_maps_all_variants() {
        let cases = vec![
            (
                MarketplaceError::SkillNotFound("missing".into()),
                StatusCode::NOT_FOUND,
            ),
            (
                MarketplaceError::SignatureInvalid("bad sig".into()),
                StatusCode::UNPROCESSABLE_ENTITY,
            ),
            (
                MarketplaceError::ManifestInvalid("bad manifest".into()),
                StatusCode::UNPROCESSABLE_ENTITY,
            ),
            (
                MarketplaceError::InvalidIndex("bad index".into()),
                StatusCode::BAD_GATEWAY,
            ),
            (
                MarketplaceError::NetworkError("offline".into()),
                StatusCode::BAD_GATEWAY,
            ),
            (
                MarketplaceError::InstallError("disk full".into()),
                StatusCode::INTERNAL_SERVER_ERROR,
            ),
            (
                MarketplaceError::InsecureRegistry("http://example.com".into()),
                StatusCode::INTERNAL_SERVER_ERROR,
            ),
        ];

        for (error, expected_status) in cases {
            let (status, body) = marketplace_error(error);
            assert_eq!(status, expected_status);
            assert!(!body.0.error.is_empty());
        }
    }

    #[tokio::test]
    async fn remove_response_rejects_invalid_names() {
        let temp = TempDir::new().expect("tempdir");
        let error = remove_skill_response(temp.path().to_path_buf(), "../escape".into())
            .await
            .expect_err("invalid name should fail");

        assert_eq!(error.0, StatusCode::BAD_REQUEST);
        assert!(error.1 .0.error.contains("forbidden characters"));
    }

    #[tokio::test]
    async fn remove_response_returns_not_found_before_canonicalize() {
        let temp = TempDir::new().expect("tempdir");
        let error = remove_skill_response(temp.path().to_path_buf(), "weather".into())
            .await
            .expect_err("missing skill should fail");

        assert_eq!(error.0, StatusCode::NOT_FOUND);
        assert_eq!(error.1 .0.error, "Skill 'weather' not found");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn remove_response_rejects_symlink_escape_without_leaking_paths() {
        let temp = TempDir::new().expect("tempdir");
        let outside_dir = temp.path().join("outside-weather");
        let skills_dir = temp.path().join("skills");
        fs::create_dir_all(&outside_dir).expect("mkdir outside");
        fs::create_dir_all(&skills_dir).expect("mkdir skills");
        create_dir_symlink(&outside_dir, &skills_dir.join("weather"));

        let error = remove_skill_response(temp.path().to_path_buf(), "weather".into())
            .await
            .expect_err("symlink escape should fail");
        let Json(body) = &error.1;
        let client_error = body.error.as_str();

        assert_eq!(error.0, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(client_error, "Skill directory outside allowed path");
        assert!(!client_error.contains(outside_dir.to_string_lossy().as_ref()));
        assert!(!client_error.contains(skills_dir.to_string_lossy().as_ref()));
        assert!(outside_dir.exists());
    }

    #[tokio::test]
    async fn remove_response_deletes_existing_skill_directory() {
        let temp = TempDir::new().expect("tempdir");
        let skill_dir = temp.path().join("skills").join("weather");
        fs::create_dir_all(&skill_dir).expect("mkdir skill");
        fs::write(skill_dir.join("manifest.toml"), "name = \"weather\"").expect("manifest");

        let response = remove_skill_response(temp.path().to_path_buf(), "weather".into())
            .await
            .expect("remove should succeed");

        assert_eq!(response["removed"], true);
        assert_eq!(response["name"], "weather");
        assert!(!skill_dir.exists());
    }

    #[test]
    fn update_skill_permissions_response_serializes() {
        let response = UpdateSkillPermissionsResponse {
            updated: true,
            name: "weather".into(),
            capabilities: vec!["network".into(), "notifications".into()],
            restart_required: true,
        };

        let json = serde_json::to_value(response).unwrap();
        assert_eq!(json["name"], "weather");
        assert_eq!(json["restart_required"], true);
        assert_eq!(json["capabilities"][0], "network");
    }
}
