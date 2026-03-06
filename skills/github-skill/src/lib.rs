//! GitHub skill — Manage pull requests and issues via the GitHub API.
//!
//! Actions: `create_pr`, `comment_pr`, `list_prs`, `view_pr`, `list_issues`, `create_issue`.

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
    #[serde(rename = "create_pr", alias = "create_pull_request")]
    CreatePr(CreatePrInput),
    #[serde(rename = "comment_pr", alias = "comment_pull_request")]
    CommentPr(CommentPrInput),
    #[serde(rename = "list_prs", alias = "list_pull_requests", alias = "list_pr")]
    ListPrs(ListPrsInput),
    #[serde(rename = "view_pr", alias = "view_pull_request", alias = "get_pr")]
    ViewPr(ViewPrInput),
    #[serde(rename = "list_issues", alias = "list_issue")]
    ListIssues(ListIssuesInput),
    #[serde(rename = "create_issue", alias = "file_issue", alias = "open_issue")]
    CreateIssue(CreateIssueInput),
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

#[derive(Deserialize)]
struct ListPrsInput {
    owner: String,
    repo: String,
    state: Option<String>,
    per_page: Option<u32>,
}

#[derive(Deserialize)]
struct ViewPrInput {
    owner: String,
    repo: String,
    pr_number: u64,
}

#[derive(Deserialize)]
struct ListIssuesInput {
    owner: String,
    repo: String,
    state: Option<String>,
    labels: Option<String>,
    per_page: Option<u32>,
}

#[derive(Deserialize)]
struct CreateIssueInput {
    owner: String,
    repo: String,
    title: String,
    body: Option<String>,
    labels: Option<Vec<String>>,
    assignees: Option<Vec<String>>,
}

#[derive(Serialize)]
struct ListPrsOutput {
    success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    prs: Option<Vec<PrSummary>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Serialize)]
struct PrSummary {
    number: u64,
    title: String,
    state: String,
    html_url: String,
    head_ref: String,
    base_ref: String,
    user_login: String,
}

#[derive(Serialize)]
struct ViewPrOutput {
    success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    number: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    body: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    state: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    html_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    head_ref: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    base_ref: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    diff: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    comments_count: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Serialize)]
struct ListIssuesOutput {
    success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    issues: Option<Vec<IssueSummary>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Serialize)]
struct IssueSummary {
    number: u64,
    title: String,
    state: String,
    html_url: String,
    user_login: String,
    labels: Vec<String>,
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

#[derive(Serialize)]
struct CreateIssueOutput {
    success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    issue_number: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    html_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Deserialize)]
struct GitHubIssueResponse {
    number: u64,
    html_url: String,
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
    errors: Option<Vec<GitHubErrorDetail>>,
}

#[derive(Deserialize)]
struct GitHubErrorDetail {
    message: Option<String>,
    /// Deserialized from the GitHub API response for completeness;
    /// not currently rendered in error messages.
    #[allow(dead_code)]
    resource: Option<String>,
    field: Option<String>,
    code: Option<String>,
}

#[derive(Deserialize)]
struct GitHubPrListItem {
    number: u64,
    title: String,
    state: String,
    html_url: String,
    head: GitHubRef,
    base: GitHubRef,
    user: GitHubUser,
}

#[derive(Deserialize)]
struct GitHubRef {
    #[serde(rename = "ref")]
    ref_name: String,
}

#[derive(Deserialize)]
struct GitHubUser {
    login: String,
}

#[derive(Deserialize)]
struct GitHubPrDetail {
    number: u64,
    title: String,
    body: Option<String>,
    state: String,
    html_url: String,
    head: GitHubRef,
    base: GitHubRef,
    comments: u64,
}

#[derive(Deserialize)]
struct GitHubIssueLabel {
    name: String,
}

#[derive(Deserialize)]
struct GitHubIssueItem {
    number: u64,
    title: String,
    state: String,
    html_url: String,
    user: GitHubUser,
    labels: Vec<GitHubIssueLabel>,
    pull_request: Option<serde_json::Value>,
}

// ── Serialization Helpers ───────────────────────────────────────────────────

fn serialize_output<T: Serialize>(output: &T) -> String {
    serde_json::to_string(output)
        .unwrap_or_else(|e| format!(r#"{{"error":"serialization failed: {e}"}}"#))
}

/// Percent-encode a query-parameter value.
///
/// Preserves unreserved characters (RFC 3986) and commas (`,`) since
/// GitHub expects comma-separated label lists. Everything else is
/// percent-encoded.
fn simple_url_encode(input: &str) -> String {
    let mut encoded = String::with_capacity(input.len());
    for byte in input.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b',' => {
                encoded.push(byte as char)
            }
            _ => {
                encoded.push('%');
                encoded.push_str(&format!("{byte:02X}"));
            }
        }
    }
    encoded
}

fn error_from_response(response: &str) -> String {
    let parsed = match serde_json::from_str::<GitHubErrorResponse>(response) {
        Ok(p) => p,
        Err(_) => return format!("Unknown error: {response}"),
    };

    let base = match parsed.message {
        Some(m) => m,
        None => return format!("Unknown error: {response}"),
    };

    let details = match parsed.errors {
        Some(ref errs) if !errs.is_empty() => format_error_details(errs),
        _ => return base,
    };

    format!("{base}: {details}")
}

fn format_error_details(errors: &[GitHubErrorDetail]) -> String {
    let parts: Vec<String> = errors
        .iter()
        .map(|e| {
            if let Some(msg) = &e.message {
                return msg.clone();
            }
            let field = e.field.as_deref().unwrap_or("unknown");
            let code = e.code.as_deref().unwrap_or("unknown");
            format!("[field: '{field}', code: '{code}']")
        })
        .collect();
    parts.join(", ")
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

// ── Parameter Validation ────────────────────────────────────────────────────

/// Validate that a repository path segment (owner or repo name) contains
/// only characters allowed by GitHub: alphanumerics, hyphens, underscores,
/// and dots.
fn validate_repo_param(s: &str) -> bool {
    !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'_' || b == b'-')
}

/// Validate owner and repo parameters, returning an error string if invalid.
fn check_owner_repo(owner: &str, repo: &str) -> Result<(), String> {
    if !validate_repo_param(owner) {
        return Err(format!("Invalid owner parameter: '{owner}'"));
    }
    if !validate_repo_param(repo) {
        return Err(format!("Invalid repo parameter: '{repo}'"));
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
    if let Err(e) = check_owner_repo(&input.owner, &input.repo) {
        return create_pr_error(e);
    }

    let base = input.base.as_deref().unwrap_or(DEFAULT_BASE);
    if let Err(e) = validate_base(base) {
        return create_pr_error(e);
    }

    let token = match get_token() {
        Ok(t) => t,
        Err(e) => return create_pr_error(e),
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
        return create_pr_error("HTTP request failed".into());
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
        create_pr_error(err_msg)
    }
}

fn handle_comment_pr(input: CommentPrInput) -> String {
    if let Err(e) = check_owner_repo(&input.owner, &input.repo) {
        return comment_pr_error(e);
    }

    let token = match get_token() {
        Ok(t) => t,
        Err(e) => return comment_pr_error(e),
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
        return comment_pr_error("HTTP request failed".into());
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
        comment_pr_error(err_msg)
    }
}

// ── List PRs ────────────────────────────────────────────────────────────────

fn handle_list_prs(input: ListPrsInput) -> String {
    if let Err(e) = check_owner_repo(&input.owner, &input.repo) {
        return serialize_output(&ListPrsOutput {
            success: false,
            prs: None,
            error: Some(e),
        });
    }

    let token = match get_token() {
        Ok(t) => t,
        Err(e) => {
            return serialize_output(&ListPrsOutput {
                success: false,
                prs: None,
                error: Some(e),
            });
        }
    };

    let state = input.state.as_deref().unwrap_or("open");
    let per_page = input.per_page.unwrap_or(10);
    let url = format!(
        "https://api.github.com/repos/{}/{}/pulls?state={}&per_page={}",
        input.owner, input.repo, state, per_page
    );
    let headers = auth_headers(&token);

    log(
        2,
        &format!("Listing PRs for {}/{}", input.owner, input.repo),
    );

    let req = HttpReq {
        method: "GET",
        url: &url,
        headers: &headers,
        body: "",
    };
    parse_list_prs_response(http_request(&req))
}

fn parse_list_prs_response(response: Option<String>) -> String {
    let Some(response) = response else {
        return serialize_output(&ListPrsOutput {
            success: false,
            prs: None,
            error: Some("HTTP request failed".into()),
        });
    };

    match serde_json::from_str::<Vec<GitHubPrListItem>>(&response) {
        Ok(items) => {
            let prs = items.into_iter().map(pr_summary_from_api).collect();
            serialize_output(&ListPrsOutput {
                success: true,
                prs: Some(prs),
                error: None,
            })
        }
        Err(_) => {
            let err_msg = error_from_response(&response);
            log(4, &format!("List PRs failed: {err_msg}"));
            serialize_output(&ListPrsOutput {
                success: false,
                prs: None,
                error: Some(err_msg),
            })
        }
    }
}

fn pr_summary_from_api(item: GitHubPrListItem) -> PrSummary {
    PrSummary {
        number: item.number,
        title: item.title,
        state: item.state,
        html_url: item.html_url,
        head_ref: item.head.ref_name,
        base_ref: item.base.ref_name,
        user_login: item.user.login,
    }
}

// ── View PR ─────────────────────────────────────────────────────────────────

fn handle_view_pr(input: ViewPrInput) -> String {
    if let Err(e) = check_owner_repo(&input.owner, &input.repo) {
        return view_pr_error(e);
    }

    let token = match get_token() {
        Ok(t) => t,
        Err(e) => return view_pr_error(e),
    };

    let url = format!(
        "https://api.github.com/repos/{}/{}/pulls/{}",
        input.owner, input.repo, input.pr_number
    );
    let headers = auth_headers(&token);

    log(
        2,
        &format!(
            "Viewing PR #{} in {}/{}",
            input.pr_number, input.owner, input.repo
        ),
    );

    let req = HttpReq {
        method: "GET",
        url: &url,
        headers: &headers,
        body: "",
    };
    let pr_response = http_request(&req);

    let diff_headers = auth_headers_with_accept(&token, "application/vnd.github.diff");
    let diff_req = HttpReq {
        method: "GET",
        url: &url,
        headers: &diff_headers,
        body: "",
    };
    let diff_response = http_request(&diff_req);

    parse_view_pr_response(pr_response, diff_response)
}

fn auth_headers_with_accept(token: &str, accept: &str) -> String {
    serde_json::json!({
        "Authorization": format!("Bearer {token}"),
        "Accept": accept,
        "User-Agent": "fawx-github-skill/1.0",
        "X-GitHub-Api-Version": "2022-11-28"
    })
    .to_string()
}

fn view_pr_error(err: String) -> String {
    serialize_output(&ViewPrOutput {
        success: false,
        number: None,
        title: None,
        body: None,
        state: None,
        html_url: None,
        head_ref: None,
        base_ref: None,
        diff: None,
        comments_count: None,
        error: Some(err),
    })
}

fn parse_view_pr_response(pr_response: Option<String>, diff_response: Option<String>) -> String {
    let Some(pr_json) = pr_response else {
        return view_pr_error("HTTP request failed".into());
    };

    let pr = match serde_json::from_str::<GitHubPrDetail>(&pr_json) {
        Ok(pr) => pr,
        Err(_) => {
            let err_msg = error_from_response(&pr_json);
            log(4, &format!("View PR failed: {err_msg}"));
            return view_pr_error(err_msg);
        }
    };

    let diff = diff_response.unwrap_or_default();

    serialize_output(&ViewPrOutput {
        success: true,
        number: Some(pr.number),
        title: Some(pr.title),
        body: pr.body,
        state: Some(pr.state),
        html_url: Some(pr.html_url),
        head_ref: Some(pr.head.ref_name),
        base_ref: Some(pr.base.ref_name),
        diff: Some(diff),
        comments_count: Some(pr.comments),
        error: None,
    })
}

// ── List Issues ─────────────────────────────────────────────────────────────

fn handle_list_issues(input: ListIssuesInput) -> String {
    if let Err(e) = check_owner_repo(&input.owner, &input.repo) {
        return serialize_output(&ListIssuesOutput {
            success: false,
            issues: None,
            error: Some(e),
        });
    }

    let token = match get_token() {
        Ok(t) => t,
        Err(e) => {
            return serialize_output(&ListIssuesOutput {
                success: false,
                issues: None,
                error: Some(e),
            });
        }
    };

    let state = input.state.as_deref().unwrap_or("open");
    let per_page = input.per_page.unwrap_or(10);
    let mut url = format!(
        "https://api.github.com/repos/{}/{}/issues?state={}&per_page={}",
        input.owner, input.repo, state, per_page
    );
    if let Some(labels) = &input.labels {
        // URL-encode the labels value to handle special characters.
        // GitHub expects comma-separated label names; commas are preserved
        // but spaces and other characters are percent-encoded.
        let encoded = simple_url_encode(labels);
        url.push_str(&format!("&labels={encoded}"));
    }
    let headers = auth_headers(&token);

    log(
        2,
        &format!("Listing issues for {}/{}", input.owner, input.repo),
    );

    let req = HttpReq {
        method: "GET",
        url: &url,
        headers: &headers,
        body: "",
    };
    parse_list_issues_response(http_request(&req))
}

fn parse_list_issues_response(response: Option<String>) -> String {
    let Some(response) = response else {
        return serialize_output(&ListIssuesOutput {
            success: false,
            issues: None,
            error: Some("HTTP request failed".into()),
        });
    };

    match serde_json::from_str::<Vec<GitHubIssueItem>>(&response) {
        Ok(items) => {
            let issues = items
                .into_iter()
                .filter(|item| item.pull_request.is_none())
                .map(issue_summary_from_api)
                .collect();
            serialize_output(&ListIssuesOutput {
                success: true,
                issues: Some(issues),
                error: None,
            })
        }
        Err(_) => {
            let err_msg = error_from_response(&response);
            log(4, &format!("List issues failed: {err_msg}"));
            serialize_output(&ListIssuesOutput {
                success: false,
                issues: None,
                error: Some(err_msg),
            })
        }
    }
}

fn issue_summary_from_api(item: GitHubIssueItem) -> IssueSummary {
    IssueSummary {
        number: item.number,
        title: item.title,
        state: item.state,
        html_url: item.html_url,
        user_login: item.user.login,
        labels: item.labels.into_iter().map(|l| l.name).collect(),
    }
}

// ── Create Issue ────────────────────────────────────────────────────────────

fn handle_create_issue(input: CreateIssueInput) -> String {
    if let Err(e) = check_owner_repo(&input.owner, &input.repo) {
        return create_issue_error(e);
    }

    let token = match get_token() {
        Ok(t) => t,
        Err(e) => return create_issue_error(e),
    };

    let url = format!(
        "https://api.github.com/repos/{}/{}/issues",
        input.owner, input.repo
    );
    let body = build_create_issue_body(&input);
    let headers = auth_headers(&token);

    log(
        2,
        &format!(
            "Creating issue '{}' in {}/{}",
            input.title, input.owner, input.repo
        ),
    );

    let req = HttpReq {
        method: "POST",
        url: &url,
        headers: &headers,
        body: &body,
    };
    parse_create_issue_response(http_request(&req))
}

fn create_pr_error(msg: String) -> String {
    serialize_output(&CreatePrOutput {
        success: false,
        pr_number: None,
        html_url: None,
        error: Some(msg),
    })
}

fn comment_pr_error(msg: String) -> String {
    serialize_output(&CommentPrOutput {
        success: false,
        comment_id: None,
        error: Some(msg),
    })
}

fn create_issue_error(err: String) -> String {
    serialize_output(&CreateIssueOutput {
        success: false,
        issue_number: None,
        html_url: None,
        error: Some(err),
    })
}

fn build_create_issue_body(input: &CreateIssueInput) -> String {
    let mut body = serde_json::json!({ "title": input.title });
    if let Some(b) = &input.body {
        body["body"] = serde_json::Value::String(b.clone());
    }
    if let Some(labels) = &input.labels {
        body["labels"] = serde_json::json!(labels);
    }
    if let Some(assignees) = &input.assignees {
        body["assignees"] = serde_json::json!(assignees);
    }
    body.to_string()
}

fn parse_create_issue_response(response: Option<String>) -> String {
    let Some(response) = response else {
        return create_issue_error("HTTP request failed".into());
    };

    if let Ok(issue) = serde_json::from_str::<GitHubIssueResponse>(&response) {
        log(
            2,
            &format!("Issue #{} created: {}", issue.number, issue.html_url),
        );
        serialize_output(&CreateIssueOutput {
            success: true,
            issue_number: Some(issue.number),
            html_url: Some(issue.html_url),
            error: None,
        })
    } else {
        let err_msg = error_from_response(&response);
        log(4, &format!("Issue creation failed: {err_msg}"));
        serialize_output(&CreateIssueOutput {
            success: false,
            issue_number: None,
            html_url: None,
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
            r#"{"error":"No input provided. Expected JSON with 'action': 'create_pr', 'comment_pr', 'list_prs', 'view_pr', 'list_issues', or 'create_issue'."}"#,
        );
        return;
    }

    let result = match serde_json::from_str::<Input>(&raw) {
        Ok(Input::CreatePr(input)) => handle_create_pr(input),
        Ok(Input::CommentPr(input)) => handle_comment_pr(input),
        Ok(Input::ListPrs(input)) => handle_list_prs(input),
        Ok(Input::ViewPr(input)) => handle_view_pr(input),
        Ok(Input::ListIssues(input)) => handle_list_issues(input),
        Ok(Input::CreateIssue(input)) => handle_create_issue(input),
        Err(e) => {
            log(4, &format!("Failed to parse input: {e}"));
            serialize_output(&serde_json::json!({
                "error": format!("Invalid input: {e}. Expected 'action': 'create_pr', 'comment_pr', 'list_prs', 'view_pr', 'list_issues', or 'create_issue'.")
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

    // ── List PRs Input Parsing ────────────────────────────────────────

    #[test]
    fn parse_list_prs_input() {
        let json = r#"{
            "action": "list_prs",
            "owner": "acme",
            "repo": "widgets"
        }"#;
        let input: Input = serde_json::from_str(json).unwrap();
        match input {
            Input::ListPrs(lp) => {
                assert_eq!(lp.owner, "acme");
                assert_eq!(lp.repo, "widgets");
                assert!(lp.state.is_none());
                assert!(lp.per_page.is_none());
            }
            _ => panic!("expected ListPrs variant"),
        }
    }

    #[test]
    fn parse_list_prs_input_with_options() {
        let json = r#"{
            "action": "list_prs",
            "owner": "acme",
            "repo": "widgets",
            "state": "closed",
            "per_page": 25
        }"#;
        let input: Input = serde_json::from_str(json).unwrap();
        match input {
            Input::ListPrs(lp) => {
                assert_eq!(lp.state.as_deref(), Some("closed"));
                assert_eq!(lp.per_page, Some(25));
            }
            _ => panic!("expected ListPrs variant"),
        }
    }

    // ── View PR Input Parsing ───────────────────────────────────────────

    #[test]
    fn parse_view_pr_input() {
        let json = r#"{
            "action": "view_pr",
            "owner": "acme",
            "repo": "widgets",
            "pr_number": 55
        }"#;
        let input: Input = serde_json::from_str(json).unwrap();
        match input {
            Input::ViewPr(vp) => {
                assert_eq!(vp.owner, "acme");
                assert_eq!(vp.repo, "widgets");
                assert_eq!(vp.pr_number, 55);
            }
            _ => panic!("expected ViewPr variant"),
        }
    }

    // ── List Issues Input Parsing ───────────────────────────────────────

    #[test]
    fn parse_list_issues_input() {
        let json = r#"{
            "action": "list_issues",
            "owner": "acme",
            "repo": "widgets"
        }"#;
        let input: Input = serde_json::from_str(json).unwrap();
        match input {
            Input::ListIssues(li) => {
                assert_eq!(li.owner, "acme");
                assert_eq!(li.repo, "widgets");
                assert!(li.state.is_none());
                assert!(li.labels.is_none());
                assert!(li.per_page.is_none());
            }
            _ => panic!("expected ListIssues variant"),
        }
    }

    #[test]
    fn parse_list_issues_input_with_options() {
        let json = r#"{
            "action": "list_issues",
            "owner": "acme",
            "repo": "widgets",
            "state": "closed",
            "labels": "bug,urgent",
            "per_page": 5
        }"#;
        let input: Input = serde_json::from_str(json).unwrap();
        match input {
            Input::ListIssues(li) => {
                assert_eq!(li.state.as_deref(), Some("closed"));
                assert_eq!(li.labels.as_deref(), Some("bug,urgent"));
                assert_eq!(li.per_page, Some(5));
            }
            _ => panic!("expected ListIssues variant"),
        }
    }

    // ── List PRs Output Serialization ───────────────────────────────────

    #[test]
    fn serialize_list_prs_success() {
        let output = ListPrsOutput {
            success: true,
            prs: Some(vec![PrSummary {
                number: 10,
                title: "Fix bug".into(),
                state: "open".into(),
                html_url: "https://github.com/acme/widgets/pull/10".into(),
                head_ref: "fix/bug".into(),
                base_ref: "staging".into(),
                user_login: "dev".into(),
            }]),
            error: None,
        };
        let json = serialize_output(&output);
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["success"], true);
        assert_eq!(parsed["prs"][0]["number"], 10);
        assert_eq!(parsed["prs"][0]["head_ref"], "fix/bug");
        assert!(parsed.get("error").is_none());
    }

    #[test]
    fn serialize_list_prs_error() {
        let output = ListPrsOutput {
            success: false,
            prs: None,
            error: Some("Not Found".into()),
        };
        let json = serialize_output(&output);
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["success"], false);
        assert_eq!(parsed["error"], "Not Found");
    }

    // ── View PR Output Serialization ────────────────────────────────────

    #[test]
    fn serialize_view_pr_success() {
        let output = ViewPrOutput {
            success: true,
            number: Some(42),
            title: Some("Big feature".into()),
            body: Some("Description".into()),
            state: Some("open".into()),
            html_url: Some("https://github.com/acme/widgets/pull/42".into()),
            head_ref: Some("feat/big".into()),
            base_ref: Some("staging".into()),
            diff: Some("diff --git a/file".into()),
            comments_count: Some(3),
            error: None,
        };
        let json = serialize_output(&output);
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["success"], true);
        assert_eq!(parsed["number"], 42);
        assert_eq!(parsed["diff"], "diff --git a/file");
        assert_eq!(parsed["comments_count"], 3);
    }

    #[test]
    fn serialize_view_pr_error() {
        let output = ViewPrOutput {
            success: false,
            number: None,
            title: None,
            body: None,
            state: None,
            html_url: None,
            head_ref: None,
            base_ref: None,
            diff: None,
            comments_count: None,
            error: Some("Not Found".into()),
        };
        let json = serialize_output(&output);
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["success"], false);
        assert_eq!(parsed["error"], "Not Found");
    }

    // ── List Issues Output Serialization ────────────────────────────────

    #[test]
    fn serialize_list_issues_success() {
        let output = ListIssuesOutput {
            success: true,
            issues: Some(vec![IssueSummary {
                number: 7,
                title: "Bug report".into(),
                state: "open".into(),
                html_url: "https://github.com/acme/widgets/issues/7".into(),
                user_login: "reporter".into(),
                labels: vec!["bug".into(), "urgent".into()],
            }]),
            error: None,
        };
        let json = serialize_output(&output);
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["success"], true);
        assert_eq!(parsed["issues"][0]["number"], 7);
        assert_eq!(parsed["issues"][0]["labels"][0], "bug");
    }

    // ── List PRs Response Parsing ───────────────────────────────────────

    #[test]
    fn parse_list_prs_response_success() {
        let response = r#"[{
            "number": 1,
            "title": "PR one",
            "state": "open",
            "html_url": "https://github.com/acme/widgets/pull/1",
            "head": {"ref": "feat/one"},
            "base": {"ref": "staging"},
            "user": {"login": "dev1"}
        }]"#;
        let result = parse_list_prs_response(Some(response.into()));
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["success"], true);
        assert_eq!(parsed["prs"][0]["number"], 1);
        assert_eq!(parsed["prs"][0]["user_login"], "dev1");
    }

    #[test]
    fn parse_list_prs_response_api_error() {
        let response = r#"{"message": "Not Found"}"#;
        let result = parse_list_prs_response(Some(response.into()));
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["success"], false);
        assert_eq!(parsed["error"], "Not Found");
    }

    #[test]
    fn parse_list_prs_response_http_failure() {
        let result = parse_list_prs_response(None);
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["success"], false);
        assert_eq!(parsed["error"], "HTTP request failed");
    }

    #[test]
    fn parse_list_prs_response_empty_array() {
        let result = parse_list_prs_response(Some("[]".into()));
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["success"], true);
        assert_eq!(parsed["prs"].as_array().unwrap().len(), 0);
    }

    // ── View PR Response Parsing ────────────────────────────────────────

    #[test]
    fn parse_view_pr_response_success() {
        let pr_json = r#"{
            "number": 42,
            "title": "Feature",
            "body": "Description",
            "state": "open",
            "html_url": "https://github.com/acme/widgets/pull/42",
            "head": {"ref": "feat/x"},
            "base": {"ref": "staging"},
            "comments": 5
        }"#;
        let diff = "diff --git a/file.rs b/file.rs\n+added line";
        let result = parse_view_pr_response(Some(pr_json.into()), Some(diff.into()));
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["success"], true);
        assert_eq!(parsed["number"], 42);
        assert_eq!(parsed["comments_count"], 5);
        assert!(parsed["diff"].as_str().unwrap().contains("+added line"));
    }

    #[test]
    fn parse_view_pr_response_api_error() {
        let response = r#"{"message": "Not Found"}"#;
        let result = parse_view_pr_response(Some(response.into()), None);
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["success"], false);
        assert_eq!(parsed["error"], "Not Found");
    }

    #[test]
    fn parse_view_pr_response_http_failure() {
        let result = parse_view_pr_response(None, None);
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["success"], false);
        assert_eq!(parsed["error"], "HTTP request failed");
    }

    #[test]
    fn parse_view_pr_response_no_diff() {
        let pr_json = r#"{
            "number": 42,
            "title": "Feature",
            "body": null,
            "state": "open",
            "html_url": "https://github.com/acme/widgets/pull/42",
            "head": {"ref": "feat/x"},
            "base": {"ref": "staging"},
            "comments": 0
        }"#;
        let result = parse_view_pr_response(Some(pr_json.into()), None);
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["success"], true);
        assert_eq!(parsed["diff"], "");
    }

    // ── List Issues Response Parsing ────────────────────────────────────

    #[test]
    fn parse_list_issues_response_filters_pull_requests() {
        let response = r#"[
            {
                "number": 1,
                "title": "Bug",
                "state": "open",
                "html_url": "https://github.com/acme/widgets/issues/1",
                "user": {"login": "dev"},
                "labels": [{"name": "bug"}],
                "pull_request": null
            },
            {
                "number": 2,
                "title": "Real issue",
                "state": "open",
                "html_url": "https://github.com/acme/widgets/issues/2",
                "user": {"login": "user1"},
                "labels": []
            },
            {
                "number": 3,
                "title": "PR disguised as issue",
                "state": "open",
                "html_url": "https://github.com/acme/widgets/issues/3",
                "user": {"login": "dev2"},
                "labels": [],
                "pull_request": {"url": "https://api.github.com/repos/acme/widgets/pulls/3"}
            }
        ]"#;
        let result = parse_list_issues_response(Some(response.into()));
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["success"], true);
        let issues = parsed["issues"].as_array().unwrap();
        // Item 1 has pull_request: null (not Some), so it passes
        // Item 2 has no pull_request field at all, so it passes
        // Item 3 has pull_request with a value, so it's filtered out
        assert_eq!(issues.len(), 2);
        assert_eq!(issues[0]["number"], 1);
        assert_eq!(issues[1]["number"], 2);
    }

    #[test]
    fn parse_list_issues_response_api_error() {
        let response = r#"{"message": "Not Found"}"#;
        let result = parse_list_issues_response(Some(response.into()));
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["success"], false);
        assert_eq!(parsed["error"], "Not Found");
    }

    #[test]
    fn parse_list_issues_response_http_failure() {
        let result = parse_list_issues_response(None);
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["success"], false);
        assert_eq!(parsed["error"], "HTTP request failed");
    }

    #[test]
    fn parse_list_issues_response_extracts_labels() {
        let response = r#"[{
            "number": 5,
            "title": "Issue with labels",
            "state": "open",
            "html_url": "https://github.com/acme/widgets/issues/5",
            "user": {"login": "dev"},
            "labels": [{"name": "bug"}, {"name": "p1"}]
        }]"#;
        let result = parse_list_issues_response(Some(response.into()));
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["success"], true);
        let labels = parsed["issues"][0]["labels"].as_array().unwrap();
        assert_eq!(labels.len(), 2);
        assert_eq!(labels[0], "bug");
        assert_eq!(labels[1], "p1");
    }

    // ── Auth Headers With Accept ────────────────────────────────────────

    #[test]
    fn auth_headers_with_accept_uses_custom_accept() {
        let headers = auth_headers_with_accept("ghp_test", "application/vnd.github.diff");
        let parsed: serde_json::Value = serde_json::from_str(&headers).unwrap();
        assert_eq!(parsed["Accept"], "application/vnd.github.diff");
        assert_eq!(parsed["Authorization"], "Bearer ghp_test");
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

    // ── URL Encoding ────────────────────────────────────────────────────

    // ── Repo Parameter Validation ──────────────────────────────────────

    #[test]
    fn validate_repo_param_accepts_normal_names() {
        assert!(validate_repo_param("acme"));
        assert!(validate_repo_param("my-repo"));
        assert!(validate_repo_param("my_repo"));
        assert!(validate_repo_param("my.repo"));
        assert!(validate_repo_param("Repo123"));
    }

    #[test]
    fn validate_repo_param_rejects_invalid_names() {
        assert!(!validate_repo_param(""));
        assert!(!validate_repo_param("acme/repo"));
        assert!(!validate_repo_param("acme repo"));
        assert!(!validate_repo_param("acme;drop"));
        assert!(!validate_repo_param("../etc"));
    }

    #[test]
    fn check_owner_repo_rejects_invalid_owner() {
        let result = check_owner_repo("bad/owner", "repo");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("owner"));
    }

    #[test]
    fn check_owner_repo_rejects_invalid_repo() {
        let result = check_owner_repo("owner", "bad repo");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("repo"));
    }

    #[test]
    fn check_owner_repo_accepts_valid_params() {
        assert!(check_owner_repo("acme", "widgets").is_ok());
    }

    // ── URL Encoding ────────────────────────────────────────────────────

    #[test]
    fn simple_url_encode_preserves_plain_labels() {
        assert_eq!(simple_url_encode("bug,urgent"), "bug,urgent");
    }

    #[test]
    fn simple_url_encode_encodes_spaces() {
        assert_eq!(
            simple_url_encode("good first issue"),
            "good%20first%20issue"
        );
    }

    #[test]
    fn simple_url_encode_encodes_special_chars() {
        assert_eq!(simple_url_encode("bug fix&v2"), "bug%20fix%26v2");
    }

    #[test]
    fn simple_url_encode_preserves_unreserved() {
        assert_eq!(simple_url_encode("a-b_c.d~e"), "a-b_c.d~e");
    }

    // ── Create Issue Input Parsing ──────────────────────────────────────

    #[test]
    fn parse_create_issue_input() {
        let json = r#"{
            "action": "create_issue",
            "owner": "acme",
            "repo": "widgets",
            "title": "Bug: crash on startup"
        }"#;
        let input: Input = serde_json::from_str(json).unwrap();
        match input {
            Input::CreateIssue(ci) => {
                assert_eq!(ci.owner, "acme");
                assert_eq!(ci.repo, "widgets");
                assert_eq!(ci.title, "Bug: crash on startup");
                assert!(ci.body.is_none());
                assert!(ci.labels.is_none());
                assert!(ci.assignees.is_none());
            }
            _ => panic!("expected CreateIssue variant"),
        }
    }

    #[test]
    fn parse_create_issue_with_labels() {
        let json = r#"{
            "action": "create_issue",
            "owner": "acme",
            "repo": "widgets",
            "title": "Add dark mode",
            "body": "Please add dark mode support",
            "labels": ["enhancement", "ui"],
            "assignees": ["dev1", "dev2"]
        }"#;
        let input: Input = serde_json::from_str(json).unwrap();
        match input {
            Input::CreateIssue(ci) => {
                assert_eq!(ci.title, "Add dark mode");
                assert_eq!(ci.body.as_deref(), Some("Please add dark mode support"));
                assert_eq!(
                    ci.labels.as_deref(),
                    Some(vec!["enhancement".to_string(), "ui".to_string()]).as_deref()
                );
                assert_eq!(
                    ci.assignees.as_deref(),
                    Some(vec!["dev1".to_string(), "dev2".to_string()]).as_deref()
                );
            }
            _ => panic!("expected CreateIssue variant"),
        }
    }

    #[test]
    fn parse_create_issue_alias_file_issue() {
        let json = r#"{
            "action": "file_issue",
            "owner": "acme",
            "repo": "widgets",
            "title": "Filed via alias"
        }"#;
        let input: Input = serde_json::from_str(json).unwrap();
        assert!(matches!(input, Input::CreateIssue(_)));
    }

    #[test]
    fn parse_create_issue_alias_open_issue() {
        let json = r#"{
            "action": "open_issue",
            "owner": "acme",
            "repo": "widgets",
            "title": "Opened via alias"
        }"#;
        let input: Input = serde_json::from_str(json).unwrap();
        assert!(matches!(input, Input::CreateIssue(_)));
    }

    // ── Create Issue Response Parsing ───────────────────────────────────

    #[test]
    fn parse_create_issue_response_success() {
        let response =
            r#"{"number": 123, "html_url": "https://github.com/acme/widgets/issues/123"}"#;
        let result = parse_create_issue_response(Some(response.into()));
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["success"], true);
        assert_eq!(parsed["issue_number"], 123);
        assert_eq!(
            parsed["html_url"],
            "https://github.com/acme/widgets/issues/123"
        );
    }

    #[test]
    fn parse_create_issue_response_api_error() {
        let response = r#"{"message": "Validation Failed"}"#;
        let result = parse_create_issue_response(Some(response.into()));
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["success"], false);
        assert_eq!(parsed["error"], "Validation Failed");
    }

    #[test]
    fn parse_create_issue_response_http_failure() {
        let result = parse_create_issue_response(None);
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["success"], false);
        assert_eq!(parsed["error"], "HTTP request failed");
    }

    // ── Create Issue Output Serialization ───────────────────────────────

    #[test]
    fn serialize_create_issue_success() {
        let output = CreateIssueOutput {
            success: true,
            issue_number: Some(42),
            html_url: Some("https://github.com/acme/widgets/issues/42".into()),
            error: None,
        };
        let json = serialize_output(&output);
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["success"], true);
        assert_eq!(parsed["issue_number"], 42);
        assert!(parsed.get("error").is_none());
    }

    // ── Create Issue Body Building ──────────────────────────────────────

    #[test]
    fn build_create_issue_body_minimal() {
        let input = CreateIssueInput {
            owner: "acme".into(),
            repo: "widgets".into(),
            title: "Test".into(),
            body: None,
            labels: None,
            assignees: None,
        };
        let body = build_create_issue_body(&input);
        let parsed: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(parsed["title"], "Test");
        assert!(parsed.get("body").is_none());
        assert!(parsed.get("labels").is_none());
        assert!(parsed.get("assignees").is_none());
    }

    #[test]
    fn build_create_issue_body_full() {
        let input = CreateIssueInput {
            owner: "acme".into(),
            repo: "widgets".into(),
            title: "Full".into(),
            body: Some("Description".into()),
            labels: Some(vec!["bug".into()]),
            assignees: Some(vec!["dev1".into()]),
        };
        let body = build_create_issue_body(&input);
        let parsed: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(parsed["title"], "Full");
        assert_eq!(parsed["body"], "Description");
        assert_eq!(parsed["labels"][0], "bug");
        assert_eq!(parsed["assignees"][0], "dev1");
    }

    // ── Enhanced Error Response ─────────────────────────────────────────

    #[test]
    fn error_from_response_with_errors_array() {
        let response = r#"{
            "message": "Validation Failed",
            "errors": [
                {"field": "head", "code": "invalid"},
                {"field": "base", "code": "missing_field"}
            ]
        }"#;
        let result = error_from_response(response);
        assert!(result.starts_with("Validation Failed: "));
        assert!(result.contains("[field: 'head', code: 'invalid']"));
        assert!(result.contains("[field: 'base', code: 'missing_field']"));
    }

    #[test]
    fn error_from_response_with_error_message_detail() {
        let response = r#"{
            "message": "Validation Failed",
            "errors": [
                {"message": "No commits between main and feat/x"}
            ]
        }"#;
        let result = error_from_response(response);
        assert_eq!(
            result,
            "Validation Failed: No commits between main and feat/x"
        );
    }

    #[test]
    fn error_from_response_with_empty_errors_array() {
        let response = r#"{"message": "Validation Failed", "errors": []}"#;
        let result = error_from_response(response);
        assert_eq!(result, "Validation Failed");
    }

    #[test]
    fn error_from_response_without_errors_field() {
        let response = r#"{"message": "Not Found"}"#;
        let result = error_from_response(response);
        assert_eq!(result, "Not Found");
    }
}
