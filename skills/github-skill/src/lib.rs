//! GitHub skill — Create and comment on pull requests via the GitHub API.
//!
//! Actions: `create_pr`, `comment_pr`.

use serde::{Deserialize, Serialize};

// ── Host API FFI ────────────────────────────────────────────────────────────

#[link(wasm_import_module = "host_api_v1")]
extern "C" {
    #[link_name = "log"]
    fn host_log(level: u32, msg_ptr: *const u8, msg_len: u32);
    #[link_name = "get_input"]
    fn host_get_input() -> u32;
    #[link_name = "set_output"]
    fn host_set_output(text_ptr: *const u8, text_len: u32);
    #[link_name = "kv_get"]
    fn host_kv_get(key_ptr: *const u8, key_len: u32) -> u32;

    #[link_name = "http_request"]
    fn host_http_request(
        method_ptr: *const u8,
        method_len: u32,
        url_ptr: *const u8,
        url_len: u32,
        headers_ptr: *const u8,
        headers_len: u32,
        body_ptr: *const u8,
        body_len: u32,
    ) -> u32;
}

// ── FFI Helpers ─────────────────────────────────────────────────────────────

/// Maximum string length to read from host memory.
const MAX_HOST_STRING_LEN: usize = 65536;

/// Read a NUL-terminated string from a pointer in WASM linear memory.
///
/// # Safety
/// The caller must ensure `ptr` was returned by a host API function and
/// points to valid WASM linear memory containing a NUL-terminated UTF-8
/// string. The pointer must remain valid for the duration of this call.
unsafe fn read_host_string(ptr: u32) -> Option<String> {
    if ptr == 0 {
        return None;
    }
    let base = ptr as *const u8;
    let slice = core::slice::from_raw_parts(base, MAX_HOST_STRING_LEN);
    let len = slice
        .iter()
        .position(|&b| b == 0)
        .unwrap_or(MAX_HOST_STRING_LEN);
    Some(String::from_utf8_lossy(&slice[..len]).into_owned())
}

fn log(level: u32, msg: &str) {
    // SAFETY: passing valid pointer and length to host log function.
    unsafe { host_log(level, msg.as_ptr(), msg.len() as u32) }
}

fn get_input() -> String {
    // SAFETY: host_get_input returns a valid NUL-terminated pointer or 0.
    unsafe { read_host_string(host_get_input()).unwrap_or_default() }
}

fn set_output(text: &str) {
    // SAFETY: passing valid pointer and length to host output function.
    unsafe { host_set_output(text.as_ptr(), text.len() as u32) }
}

fn kv_get(key: &str) -> Option<String> {
    // SAFETY: host_kv_get returns a valid NUL-terminated pointer or 0.
    unsafe { read_host_string(host_kv_get(key.as_ptr(), key.len() as u32)) }
}

fn http_request(req: &HttpReq<'_>) -> Option<String> {
    // SAFETY: host_http_request returns a valid NUL-terminated pointer or 0.
    // All string slices passed are valid for the duration of the call.
    unsafe {
        read_host_string(host_http_request(
            req.method.as_ptr(),
            req.method.len() as u32,
            req.url.as_ptr(),
            req.url.len() as u32,
            req.headers.as_ptr(),
            req.headers.len() as u32,
            req.body.as_ptr(),
            req.body.len() as u32,
        ))
    }
}

/// Parameters for an HTTP request (avoids >5 bare params).
struct HttpReq<'a> {
    method: &'a str,
    url: &'a str,
    headers: &'a str,
    body: &'a str,
}

// ── Data Types ──────────────────────────────────────────────────────────────

#[derive(Deserialize)]
#[serde(tag = "action")]
enum Input {
    #[serde(rename = "create_pr")]
    CreatePr(CreatePrInput),
    #[serde(rename = "comment_pr")]
    CommentPr(CommentPrInput),
}

#[derive(Deserialize)]
struct CreatePrInput {
    owner: String,
    repo: String,
    title: String,
    body: Option<String>,
    head: String,
    base: Option<String>,
    draft: Option<bool>,
}

#[derive(Deserialize)]
struct CommentPrInput {
    owner: String,
    repo: String,
    pr_number: u64,
    body: String,
}

#[derive(Serialize)]
struct CreatePrOutput {
    success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pr_number: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    html_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Serialize)]
struct CommentPrOutput {
    success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    comment_id: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Deserialize)]
struct GitHubPrResponse {
    number: u64,
    html_url: String,
}

#[derive(Deserialize)]
struct GitHubCommentResponse {
    id: u64,
}

#[derive(Deserialize)]
struct GitHubErrorResponse {
    message: Option<String>,
}

// ── Serialization Helpers ───────────────────────────────────────────────────

fn serialize_output<T: Serialize>(output: &T) -> String {
    serde_json::to_string(output)
        .unwrap_or_else(|e| format!(r#"{{"error":"serialization failed: {e}"}}"#))
}

fn error_from_response(response: &str) -> String {
    serde_json::from_str::<GitHubErrorResponse>(response)
        .ok()
        .and_then(|e| e.message)
        .unwrap_or_else(|| format!("Unknown error: {response}"))
}

// ── Blocked Branches ────────────────────────────────────────────────────────

/// Branches that must never be used as a PR base.
const BLOCKED_BASES: &[&str] = &["main", "master"];

/// Default base branch when none is specified.
const DEFAULT_BASE: &str = "staging";

fn validate_base(base: &str) -> Result<(), String> {
    if BLOCKED_BASES.contains(&base) {
        return Err(format!(
            "Base branch '{base}' is blocked. PRs must target 'staging', not '{base}'."
        ));
    }
    Ok(())
}

// ── Core Logic ──────────────────────────────────────────────────────────────

const TOKEN_KEY: &str = "github_token";

fn get_token() -> Result<String, String> {
    kv_get(TOKEN_KEY).ok_or_else(|| {
        "GitHub token not configured. Store a token with key 'github_token' via the host KV API."
            .to_string()
    })
}

fn auth_headers(token: &str) -> String {
    serde_json::json!({
        "Authorization": format!("Bearer {token}"),
        "Accept": "application/vnd.github+json",
        "Content-Type": "application/json",
        "User-Agent": "fawx-github-skill/1.0",
        "X-GitHub-Api-Version": "2022-11-28"
    })
    .to_string()
}

fn build_create_pr_request(input: &CreatePrInput) -> (String, String) {
    let url = format!(
        "https://api.github.com/repos/{}/{}/pulls",
        input.owner, input.repo
    );
    let body = serde_json::json!({
        "title": input.title,
        "body": input.body.as_deref().unwrap_or_default(),
        "head": input.head,
        "base": input.base.as_deref().unwrap_or(DEFAULT_BASE),
        "draft": input.draft.unwrap_or(false),
    })
    .to_string();
    (url, body)
}

fn handle_create_pr(input: CreatePrInput) -> String {
    let base = input.base.as_deref().unwrap_or(DEFAULT_BASE);
    if let Err(e) = validate_base(base) {
        return serialize_output(&CreatePrOutput {
            success: false,
            pr_number: None,
            html_url: None,
            error: Some(e),
        });
    }

    let token = match get_token() {
        Ok(t) => t,
        Err(e) => {
            return serialize_output(&CreatePrOutput {
                success: false,
                pr_number: None,
                html_url: None,
                error: Some(e),
            });
        }
    };

    let (url, body) = build_create_pr_request(&input);
    let headers = auth_headers(&token);
    log(
        2,
        &format!(
            "Creating PR: {} -> {}/{}",
            input.head, input.owner, input.repo
        ),
    );

    let req = HttpReq {
        method: "POST",
        url: &url,
        headers: &headers,
        body: &body,
    };
    parse_create_pr_response(http_request(&req))
}

fn parse_create_pr_response(response: Option<String>) -> String {
    let Some(response) = response else {
        return serialize_output(&CreatePrOutput {
            success: false,
            pr_number: None,
            html_url: None,
            error: Some("HTTP request failed".into()),
        });
    };

    if let Ok(pr) = serde_json::from_str::<GitHubPrResponse>(&response) {
        log(2, &format!("PR #{} created: {}", pr.number, pr.html_url));
        serialize_output(&CreatePrOutput {
            success: true,
            pr_number: Some(pr.number),
            html_url: Some(pr.html_url),
            error: None,
        })
    } else {
        let err_msg = error_from_response(&response);
        log(4, &format!("PR creation failed: {err_msg}"));
        serialize_output(&CreatePrOutput {
            success: false,
            pr_number: None,
            html_url: None,
            error: Some(err_msg),
        })
    }
}

fn handle_comment_pr(input: CommentPrInput) -> String {
    let token = match get_token() {
        Ok(t) => t,
        Err(e) => {
            return serialize_output(&CommentPrOutput {
                success: false,
                comment_id: None,
                error: Some(e),
            });
        }
    };

    let url = format!(
        "https://api.github.com/repos/{}/{}/issues/{}/comments",
        input.owner, input.repo, input.pr_number
    );
    let body = serde_json::json!({ "body": input.body }).to_string();
    let headers = auth_headers(&token);

    log(
        2,
        &format!(
            "Commenting on PR #{} in {}/{}",
            input.pr_number, input.owner, input.repo
        ),
    );

    let req = HttpReq {
        method: "POST",
        url: &url,
        headers: &headers,
        body: &body,
    };
    parse_comment_pr_response(http_request(&req))
}

fn parse_comment_pr_response(response: Option<String>) -> String {
    let Some(response) = response else {
        return serialize_output(&CommentPrOutput {
            success: false,
            comment_id: None,
            error: Some("HTTP request failed".into()),
        });
    };

    if let Ok(comment) = serde_json::from_str::<GitHubCommentResponse>(&response) {
        log(2, &format!("Comment {} posted", comment.id));
        serialize_output(&CommentPrOutput {
            success: true,
            comment_id: Some(comment.id),
            error: None,
        })
    } else {
        let err_msg = error_from_response(&response);
        log(4, &format!("Comment failed: {err_msg}"));
        serialize_output(&CommentPrOutput {
            success: false,
            comment_id: None,
            error: Some(err_msg),
        })
    }
}

// ── Entry Point ─────────────────────────────────────────────────────────────

#[no_mangle]
pub extern "C" fn run() {
    let raw = get_input();
    if raw.is_empty() {
        set_output(
            r#"{"error":"No input provided. Expected JSON with 'action': 'create_pr' or 'comment_pr'."}"#,
        );
        return;
    }

    let result = match serde_json::from_str::<Input>(&raw) {
        Ok(Input::CreatePr(input)) => handle_create_pr(input),
        Ok(Input::CommentPr(input)) => handle_comment_pr(input),
        Err(e) => {
            log(4, &format!("Failed to parse input: {e}"));
            serialize_output(&serde_json::json!({
                "error": format!("Invalid input: {e}. Expected 'action': 'create_pr' or 'comment_pr'.")
            }))
        }
    };

    set_output(&result);
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Input Parsing ───────────────────────────────────────────────────

    #[test]
    fn parse_create_pr_input() {
        let json = r#"{
            "action": "create_pr",
            "owner": "acme",
            "repo": "widgets",
            "title": "Add feature",
            "head": "feat/thing",
            "base": "staging"
        }"#;
        let input: Input = serde_json::from_str(json).unwrap();
        match input {
            Input::CreatePr(pr) => {
                assert_eq!(pr.owner, "acme");
                assert_eq!(pr.repo, "widgets");
                assert_eq!(pr.title, "Add feature");
                assert_eq!(pr.head, "feat/thing");
                assert_eq!(pr.base.as_deref(), Some("staging"));
                assert!(pr.body.is_none());
                assert!(pr.draft.is_none());
            }
            _ => panic!("expected CreatePr variant"),
        }
    }

    #[test]
    fn parse_create_pr_with_optional_fields() {
        let json = r#"{
            "action": "create_pr",
            "owner": "acme",
            "repo": "widgets",
            "title": "Draft PR",
            "head": "feat/draft",
            "body": "Some description",
            "draft": true
        }"#;
        let input: Input = serde_json::from_str(json).unwrap();
        match input {
            Input::CreatePr(pr) => {
                assert_eq!(pr.body.as_deref(), Some("Some description"));
                assert_eq!(pr.draft, Some(true));
                assert!(pr.base.is_none()); // defaults to staging
            }
            _ => panic!("expected CreatePr variant"),
        }
    }

    #[test]
    fn parse_comment_pr_input() {
        let json = r#"{
            "action": "comment_pr",
            "owner": "acme",
            "repo": "widgets",
            "pr_number": 42,
            "body": "LGTM"
        }"#;
        let input: Input = serde_json::from_str(json).unwrap();
        match input {
            Input::CommentPr(c) => {
                assert_eq!(c.owner, "acme");
                assert_eq!(c.repo, "widgets");
                assert_eq!(c.pr_number, 42);
                assert_eq!(c.body, "LGTM");
            }
            _ => panic!("expected CommentPr variant"),
        }
    }

    #[test]
    fn parse_empty_input_fails() {
        let result = serde_json::from_str::<Input>("");
        assert!(result.is_err());
    }

    #[test]
    fn parse_malformed_json_fails() {
        let result = serde_json::from_str::<Input>("{not json}");
        assert!(result.is_err());
    }

    #[test]
    fn parse_missing_action_fails() {
        let json = r#"{"owner": "acme", "repo": "widgets"}"#;
        let result = serde_json::from_str::<Input>(json);
        assert!(result.is_err());
    }

    #[test]
    fn parse_unknown_action_fails() {
        let json = r#"{"action": "delete_repo", "owner": "acme"}"#;
        let result = serde_json::from_str::<Input>(json);
        assert!(result.is_err());
    }

    // ── Base Branch Validation ──────────────────────────────────────────

    #[test]
    fn validate_base_rejects_main() {
        assert!(validate_base("main").is_err());
    }

    #[test]
    fn validate_base_rejects_master() {
        assert!(validate_base("master").is_err());
    }

    #[test]
    fn validate_base_accepts_staging() {
        assert!(validate_base("staging").is_ok());
    }

    #[test]
    fn validate_base_accepts_develop() {
        assert!(validate_base("develop").is_ok());
    }

    // ── Output Serialization ────────────────────────────────────────────

    #[test]
    fn serialize_create_pr_success() {
        let output = CreatePrOutput {
            success: true,
            pr_number: Some(99),
            html_url: Some("https://github.com/acme/widgets/pull/99".into()),
            error: None,
        };
        let json = serialize_output(&output);
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["success"], true);
        assert_eq!(parsed["pr_number"], 99);
        assert!(parsed.get("error").is_none());
    }

    #[test]
    fn serialize_create_pr_error() {
        let output = CreatePrOutput {
            success: false,
            pr_number: None,
            html_url: None,
            error: Some("token missing".into()),
        };
        let json = serialize_output(&output);
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["success"], false);
        assert_eq!(parsed["error"], "token missing");
        assert!(parsed.get("pr_number").is_none());
    }

    #[test]
    fn serialize_comment_pr_success() {
        let output = CommentPrOutput {
            success: true,
            comment_id: Some(12345),
            error: None,
        };
        let json = serialize_output(&output);
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["success"], true);
        assert_eq!(parsed["comment_id"], 12345);
    }

    #[test]
    fn serialize_comment_pr_error() {
        let output = CommentPrOutput {
            success: false,
            comment_id: None,
            error: Some("HTTP request failed".into()),
        };
        let json = serialize_output(&output);
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["success"], false);
        assert_eq!(parsed["error"], "HTTP request failed");
    }

    // ── Response Parsing ────────────────────────────────────────────────

    #[test]
    fn parse_create_pr_response_success() {
        let response = r#"{"number": 42, "html_url": "https://github.com/acme/widgets/pull/42"}"#;
        let result = parse_create_pr_response(Some(response.into()));
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["success"], true);
        assert_eq!(parsed["pr_number"], 42);
    }

    #[test]
    fn parse_create_pr_response_api_error() {
        let response = r#"{"message": "Validation Failed"}"#;
        let result = parse_create_pr_response(Some(response.into()));
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["success"], false);
        assert_eq!(parsed["error"], "Validation Failed");
    }

    #[test]
    fn parse_create_pr_response_http_failure() {
        let result = parse_create_pr_response(None);
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["success"], false);
        assert_eq!(parsed["error"], "HTTP request failed");
    }

    #[test]
    fn parse_comment_pr_response_success() {
        let response = r#"{"id": 999}"#;
        let result = parse_comment_pr_response(Some(response.into()));
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["success"], true);
        assert_eq!(parsed["comment_id"], 999);
    }

    #[test]
    fn parse_comment_pr_response_api_error() {
        let response = r#"{"message": "Not Found"}"#;
        let result = parse_comment_pr_response(Some(response.into()));
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["success"], false);
        assert_eq!(parsed["error"], "Not Found");
    }

    #[test]
    fn parse_comment_pr_response_http_failure() {
        let result = parse_comment_pr_response(None);
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["success"], false);
        assert_eq!(parsed["error"], "HTTP request failed");
    }

    // ── Error Extraction ────────────────────────────────────────────────

    #[test]
    fn error_from_response_extracts_message() {
        let response = r#"{"message": "Bad credentials"}"#;
        assert_eq!(error_from_response(response), "Bad credentials");
    }

    #[test]
    fn error_from_response_unknown_format() {
        let response = "unexpected html";
        assert!(error_from_response(response).contains("Unknown error"));
    }

    // ── Auth Headers ────────────────────────────────────────────────────

    #[test]
    fn auth_headers_contains_bearer_token() {
        let headers = auth_headers("ghp_test123");
        let parsed: serde_json::Value = serde_json::from_str(&headers).unwrap();
        assert_eq!(parsed["Authorization"], "Bearer ghp_test123");
        assert_eq!(parsed["Accept"], "application/vnd.github+json");
    }

    // ── Request Building ────────────────────────────────────────────────

    #[test]
    fn build_create_pr_request_url_and_body() {
        let input = CreatePrInput {
            owner: "acme".into(),
            repo: "widgets".into(),
            title: "My PR".into(),
            body: Some("Description".into()),
            head: "feat/x".into(),
            base: Some("staging".into()),
            draft: Some(true),
        };
        let (url, body) = build_create_pr_request(&input);
        assert_eq!(url, "https://api.github.com/repos/acme/widgets/pulls");
        let parsed: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(parsed["title"], "My PR");
        assert_eq!(parsed["base"], "staging");
        assert_eq!(parsed["draft"], true);
    }

    #[test]
    fn build_create_pr_request_defaults_base_to_staging() {
        let input = CreatePrInput {
            owner: "acme".into(),
            repo: "widgets".into(),
            title: "My PR".into(),
            body: None,
            head: "feat/x".into(),
            base: None,
            draft: None,
        };
        let (_, body) = build_create_pr_request(&input);
        let parsed: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(parsed["base"], "staging");
        assert_eq!(parsed["draft"], false);
        assert_eq!(parsed["body"], "");
    }
}
