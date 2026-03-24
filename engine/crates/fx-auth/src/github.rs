//! GitHub PAT validation.
//!
//! Validates a GitHub Personal Access Token by calling the GitHub API
//! and checking scopes/permissions.
//!
//! Uses `reqwest` for consistency with the rest of the codebase (the TUI
//! already depends on `reqwest` for OAuth token exchange). The previous
//! `ureq` sync client was replaced to avoid an extra dependency.

use serde::Deserialize;
use zeroize::Zeroizing;

/// Required scopes for Fawx GitHub operations.
pub const REQUIRED_SCOPES: &[&str] = &["repo", "workflow"];

/// Information about a validated GitHub token.
#[derive(Debug, Clone)]
pub struct GitHubTokenInfo {
    /// GitHub username associated with the token.
    pub login: String,
    /// Scopes granted by the token.
    pub scopes: Vec<String>,
    /// Scopes required but not present.
    pub missing_scopes: Vec<String>,
}

impl GitHubTokenInfo {
    /// Whether the token has all required scopes.
    pub fn has_sufficient_scopes(&self) -> bool {
        self.missing_scopes.is_empty()
    }
}

/// Errors from GitHub token validation.
#[derive(Debug)]
pub enum GitHubValidationError {
    /// Token is invalid (401 Unauthorized).
    InvalidToken,
    /// HTTP request failed.
    RequestFailed(String),
    /// Response was unparseable.
    ParseError(String),
}

impl std::fmt::Display for GitHubValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidToken => write!(f, "token is invalid or expired"),
            Self::RequestFailed(msg) => write!(f, "request failed: {msg}"),
            Self::ParseError(msg) => write!(f, "failed to parse response: {msg}"),
        }
    }
}

impl std::error::Error for GitHubValidationError {}

/// Minimal GitHub user API response.
#[derive(Deserialize)]
struct GitHubUserResponse {
    login: String,
}

/// Validate a GitHub PAT by calling the GitHub API.
///
/// Makes a GET request to `https://api.github.com/user` with the token,
/// checks the response status and `X-OAuth-Scopes` header.
///
/// # Fine-grained PATs
///
/// GitHub fine-grained personal access tokens do **not** return the
/// `X-OAuth-Scopes` response header. As a result, all required scopes
/// will appear in [`GitHubTokenInfo::missing_scopes`] even when the
/// token actually has sufficient permissions. Callers should treat
/// "token valid but scopes missing" differently when the token prefix
/// (e.g. `github_pat_`) suggests a fine-grained PAT — the absence of
/// scope headers does not mean the token lacks permissions.
pub async fn validate_github_pat(
    token: &Zeroizing<String>,
) -> Result<GitHubTokenInfo, GitHubValidationError> {
    validate_github_pat_with_url(token, "https://api.github.com/user").await
}

/// Validate a GitHub PAT against a specific URL (for testing).
pub(crate) async fn validate_github_pat_with_url(
    token: &Zeroizing<String>,
    url: &str,
) -> Result<GitHubTokenInfo, GitHubValidationError> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| GitHubValidationError::RequestFailed(e.to_string()))?;

    let response = client
        .get(url)
        .header("Authorization", format!("Bearer {}", token.as_str()))
        .header("User-Agent", "fawx-cli")
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| GitHubValidationError::RequestFailed(e.to_string()))?;

    let status = response.status();
    if status == reqwest::StatusCode::UNAUTHORIZED {
        return Err(GitHubValidationError::InvalidToken);
    }
    if status == reqwest::StatusCode::FORBIDDEN {
        let is_rate_limited = response
            .headers()
            .get("X-RateLimit-Remaining")
            .and_then(|v| v.to_str().ok())
            .map(|v| v == "0")
            .unwrap_or(false);
        if is_rate_limited {
            return Err(GitHubValidationError::RequestFailed(
                "GitHub API rate limit exceeded, try again later".to_string(),
            ));
        }
    }
    if !status.is_success() {
        return Err(GitHubValidationError::RequestFailed(format!(
            "GitHub API returned HTTP {}",
            status.as_u16()
        )));
    }

    parse_github_response(response).await
}

/// Parse the GitHub API response and extract token info.
async fn parse_github_response(
    resp: reqwest::Response,
) -> Result<GitHubTokenInfo, GitHubValidationError> {
    let scopes = extract_scopes_from_header(&resp);

    let body: GitHubUserResponse = resp
        .json()
        .await
        .map_err(|e| GitHubValidationError::ParseError(e.to_string()))?;

    let missing_scopes = compute_missing_scopes(&scopes);

    Ok(GitHubTokenInfo {
        login: body.login,
        scopes,
        missing_scopes,
    })
}

/// Extract OAuth scopes from the X-OAuth-Scopes response header.
fn extract_scopes_from_header(resp: &reqwest::Response) -> Vec<String> {
    resp.headers()
        .get("X-OAuth-Scopes")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// Compute which required scopes are missing from the granted scopes.
fn compute_missing_scopes(granted: &[String]) -> Vec<String> {
    REQUIRED_SCOPES
        .iter()
        .filter(|required| !granted.iter().any(|g| g == **required))
        .map(|s| (*s).to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sufficient_scopes_when_all_present() {
        let info = GitHubTokenInfo {
            login: "testuser".to_string(),
            scopes: vec!["repo".to_string(), "workflow".to_string()],
            missing_scopes: vec![],
        };
        assert!(info.has_sufficient_scopes());
    }

    #[test]
    fn insufficient_scopes_when_missing() {
        let info = GitHubTokenInfo {
            login: "testuser".to_string(),
            scopes: vec!["repo".to_string()],
            missing_scopes: vec!["workflow".to_string()],
        };
        assert!(!info.has_sufficient_scopes());
    }

    #[test]
    fn compute_missing_scopes_empty_granted() {
        let missing = compute_missing_scopes(&[]);
        assert_eq!(missing, vec!["repo", "workflow"]);
    }

    #[test]
    fn compute_missing_scopes_all_granted() {
        let granted = vec!["repo".to_string(), "workflow".to_string()];
        let missing = compute_missing_scopes(&granted);
        assert!(missing.is_empty());
    }

    #[test]
    fn compute_missing_scopes_partial() {
        let granted = vec!["repo".to_string(), "read:org".to_string()];
        let missing = compute_missing_scopes(&granted);
        assert_eq!(missing, vec!["workflow"]);
    }

    #[test]
    fn compute_missing_scopes_extra_granted() {
        let granted = vec![
            "repo".to_string(),
            "workflow".to_string(),
            "admin:org".to_string(),
        ];
        let missing = compute_missing_scopes(&granted);
        assert!(missing.is_empty());
    }

    #[test]
    fn extract_scopes_comma_separated() {
        let scopes = "repo, workflow, read:org";
        let parsed: Vec<String> = scopes
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        assert_eq!(parsed, vec!["repo", "workflow", "read:org"]);
    }

    #[test]
    fn extract_scopes_empty_string() {
        let scopes = "";
        let parsed: Vec<String> = scopes
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        assert!(parsed.is_empty());
    }

    #[test]
    fn validation_error_display() {
        assert_eq!(
            GitHubValidationError::InvalidToken.to_string(),
            "token is invalid or expired"
        );
        assert_eq!(
            GitHubValidationError::RequestFailed("timeout".to_string()).to_string(),
            "request failed: timeout"
        );
        assert_eq!(
            GitHubValidationError::ParseError("bad json".to_string()).to_string(),
            "failed to parse response: bad json"
        );
    }

    /// Test response parsing via `validate_github_pat_with_url` against
    /// a local mock server that returns a known JSON body and scopes header.
    #[tokio::test]
    async fn validate_github_pat_with_mock_server() {
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind mock server");
        let addr = listener.local_addr().expect("local addr");
        let url = format!("http://{addr}/user");

        // Spawn a minimal HTTP responder.
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("accept");
            let mut buf = vec![0u8; 4096];
            use tokio::io::AsyncReadExt;
            let _ = stream.read(&mut buf).await;

            let body = r#"{"login":"octocat"}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\n\
                 Content-Type: application/json\r\n\
                 X-OAuth-Scopes: repo, workflow\r\n\
                 Content-Length: {}\r\n\r\n{}",
                body.len(),
                body
            );
            use tokio::io::AsyncWriteExt;
            stream.write_all(response.as_bytes()).await.ok();
        });

        let token = Zeroizing::new("ghp_test_token".to_string());
        let info = validate_github_pat_with_url(&token, &url)
            .await
            .expect("validation should succeed");

        assert_eq!(info.login, "octocat");
        assert_eq!(info.scopes, vec!["repo", "workflow"]);
        assert!(info.missing_scopes.is_empty());
        assert!(info.has_sufficient_scopes());
    }

    /// Test that a 403 with `X-RateLimit-Remaining: 0` returns a rate-limit error.
    #[tokio::test]
    async fn validate_github_pat_rate_limited() {
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind mock server");
        let addr = listener.local_addr().expect("local addr");
        let url = format!("http://{addr}/user");

        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("accept");
            let mut buf = vec![0u8; 4096];
            use tokio::io::AsyncReadExt;
            let _ = stream.read(&mut buf).await;

            let response = "HTTP/1.1 403 Forbidden\r\n\
                            X-RateLimit-Remaining: 0\r\n\
                            Content-Length: 0\r\n\r\n";
            use tokio::io::AsyncWriteExt;
            stream.write_all(response.as_bytes()).await.ok();
        });

        let token = Zeroizing::new("ghp_test_token".to_string());
        let err = validate_github_pat_with_url(&token, &url)
            .await
            .expect_err("should fail with rate limit");

        match err {
            GitHubValidationError::RequestFailed(msg) => {
                assert!(
                    msg.contains("rate limit"),
                    "expected rate limit message, got: {msg}"
                );
            }
            other => panic!("expected RequestFailed, got {other:?}"),
        }
    }

    /// Test that a 401 response maps to `InvalidToken`.
    #[tokio::test]
    async fn validate_github_pat_invalid_token() {
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind mock server");
        let addr = listener.local_addr().expect("local addr");
        let url = format!("http://{addr}/user");

        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("accept");
            let mut buf = vec![0u8; 4096];
            use tokio::io::AsyncReadExt;
            let _ = stream.read(&mut buf).await;

            let response = "HTTP/1.1 401 Unauthorized\r\n\
                            Content-Length: 0\r\n\r\n";
            use tokio::io::AsyncWriteExt;
            stream.write_all(response.as_bytes()).await.ok();
        });

        let token = Zeroizing::new("ghp_bad_token".to_string());
        let err = validate_github_pat_with_url(&token, &url)
            .await
            .expect_err("should fail with InvalidToken");

        assert!(
            matches!(err, GitHubValidationError::InvalidToken),
            "expected InvalidToken, got {err:?}"
        );
    }
}
