use super::{parse_args, to_tool_result, tool_failure_from_io, ToolFailure, ToolRegistry};
use crate::tool_trait::{Tool, ToolContext};
use async_trait::async_trait;
use fx_kernel::act::{ToolCacheability, ToolResult};
use fx_kernel::cancellation::CancellationToken;
use fx_kernel::ToolAuthoritySurface;
use fx_llm::{ToolCall, ToolDefinition};
use reqwest::Url;
use serde::Deserialize;
use std::process::Command;
use std::sync::Arc;

const OPEN_BROWSER_URL_TOOL_NAME: &str = "open_browser_url";
const OPEN_BROWSER_APPLICATION_TOOL_NAME: &str = "open_browser_application";

pub(super) fn register_tools(registry: &mut ToolRegistry, context: &Arc<ToolContext>) {
    registry.register(OpenBrowserUrlTool::new(context));
    registry.register(OpenBrowserApplicationTool::new(context));
}

struct OpenBrowserUrlTool {
    context: Arc<ToolContext>,
}

struct OpenBrowserApplicationTool {
    context: Arc<ToolContext>,
}

impl OpenBrowserUrlTool {
    fn new(context: &Arc<ToolContext>) -> Self {
        Self {
            context: Arc::clone(context),
        }
    }
}

impl OpenBrowserApplicationTool {
    fn new(context: &Arc<ToolContext>) -> Self {
        Self {
            context: Arc::clone(context),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
// Keep this variant list aligned with the parallel kernel-local enum in
// fx-kernel's bounded_local.rs. The duplication is intentional so the kernel
// does not depend on the tool crate, but the supported browser set is one
// cross-crate contract.
enum BrowserApplication {
    Chrome,
    Safari,
    Firefox,
    Brave,
    Edge,
}

impl BrowserApplication {
    const ALL: [Self; 5] = [
        Self::Chrome,
        Self::Safari,
        Self::Firefox,
        Self::Brave,
        Self::Edge,
    ];

    fn schema_values() -> [&'static str; 5] {
        Self::ALL.map(Self::argument_value)
    }

    fn argument_value(self) -> &'static str {
        match self {
            Self::Chrome => "chrome",
            Self::Safari => "safari",
            Self::Firefox => "firefox",
            Self::Brave => "brave",
            Self::Edge => "edge",
        }
    }

    fn display_name(self) -> &'static str {
        match self {
            Self::Chrome => "Google Chrome",
            Self::Safari => "Safari",
            Self::Firefox => "Firefox",
            Self::Brave => "Brave Browser",
            Self::Edge => "Microsoft Edge",
        }
    }

    #[cfg(target_os = "windows")]
    fn windows_launcher(self) -> &'static str {
        match self {
            Self::Chrome => "chrome",
            Self::Safari => "safari",
            Self::Firefox => "firefox",
            Self::Brave => "brave",
            Self::Edge => "msedge",
        }
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    fn linux_launcher(self) -> Result<&'static str, ToolFailure> {
        match self {
            Self::Chrome => Ok("google-chrome"),
            Self::Firefox => Ok("firefox"),
            Self::Brave => Ok("brave-browser"),
            Self::Edge => Ok("microsoft-edge"),
            Self::Safari => Err(ToolFailure::permanent(
                "Safari is not supported on this platform",
            )),
        }
    }
}

#[derive(Deserialize)]
struct OpenBrowserUrlArgs {
    url: String,
}

#[derive(Deserialize)]
struct OpenBrowserApplicationArgs {
    browser: BrowserApplication,
}

#[async_trait]
impl Tool for OpenBrowserUrlTool {
    fn name(&self) -> &'static str {
        OPEN_BROWSER_URL_TOOL_NAME
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "Open an HTTP or HTTPS URL in the default browser.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "url": {
                        "type": "string",
                        "description": "HTTP or HTTPS URL to open in the default browser."
                    }
                },
                "required": ["url"]
            }),
        }
    }

    async fn execute(&self, call: &ToolCall, _cancel: Option<&CancellationToken>) -> ToolResult {
        let context = Arc::clone(&self.context);
        let args = call.arguments.clone();
        let output = run_local_action(move || context.handle_open_browser_url(&args)).await;
        to_tool_result(&call.id, self.name(), output)
    }

    fn cacheability(&self) -> ToolCacheability {
        ToolCacheability::SideEffect
    }

    fn action_category(&self) -> &'static str {
        "app_open"
    }

    fn authority_surface(&self, _call: &ToolCall) -> ToolAuthoritySurface {
        ToolAuthoritySurface::Command
    }
}

#[async_trait]
impl Tool for OpenBrowserApplicationTool {
    fn name(&self) -> &'static str {
        OPEN_BROWSER_APPLICATION_TOOL_NAME
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "Open a supported browser application locally.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "browser": {
                        "type": "string",
                        "enum": BrowserApplication::schema_values(),
                        "description": "Browser application to open."
                    }
                },
                "required": ["browser"]
            }),
        }
    }

    async fn execute(&self, call: &ToolCall, _cancel: Option<&CancellationToken>) -> ToolResult {
        let context = Arc::clone(&self.context);
        let args = call.arguments.clone();
        let output = run_local_action(move || context.handle_open_browser_application(&args)).await;
        to_tool_result(&call.id, self.name(), output)
    }

    fn cacheability(&self) -> ToolCacheability {
        ToolCacheability::SideEffect
    }

    fn action_category(&self) -> &'static str {
        "app_open"
    }

    fn authority_surface(&self, _call: &ToolCall) -> ToolAuthoritySurface {
        ToolAuthoritySurface::Command
    }
}

impl ToolContext {
    pub(crate) fn handle_open_browser_url(
        &self,
        args: &serde_json::Value,
    ) -> Result<String, ToolFailure> {
        let parsed: OpenBrowserUrlArgs = parse_args(args).map_err(ToolFailure::permanent)?;
        let url = validate_browser_url(&parsed.url)?;
        open_browser_url(&url)?;
        Ok(format!("Opened {url} in the default browser."))
    }

    pub(crate) fn handle_open_browser_application(
        &self,
        args: &serde_json::Value,
    ) -> Result<String, ToolFailure> {
        let parsed: OpenBrowserApplicationArgs =
            parse_args(args).map_err(ToolFailure::permanent)?;
        open_browser_application(parsed.browser)?;
        Ok(format!("Opened {}.", parsed.browser.display_name()))
    }
}

fn validate_browser_url(url: &str) -> Result<String, ToolFailure> {
    let trimmed = url.trim();
    let parsed = Url::parse(trimmed).map_err(|error| {
        ToolFailure::permanent(format!("invalid http:// or https:// url: {error}"))
    })?;
    if matches!(parsed.scheme(), "http" | "https") {
        Ok(trimmed.to_string())
    } else {
        Err(ToolFailure::permanent(
            "url must start with http:// or https://",
        ))
    }
}

fn open_browser_url(url: &str) -> Result<(), ToolFailure> {
    #[cfg(target_os = "windows")]
    {
        return open_browser_url_via_shell_execute(url);
    }

    let (program, args) = browser_url_command(url);
    run_launcher(program, &args)
}

fn open_browser_application(browser: BrowserApplication) -> Result<(), ToolFailure> {
    let (program, args) = browser_application_command(browser)?;
    run_launcher(&program, &args)
}

#[cfg(not(target_os = "windows"))]
fn browser_url_command(url: &str) -> (&'static str, Vec<String>) {
    #[cfg(target_os = "macos")]
    {
        ("open", vec![url.to_string()])
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        ("xdg-open", vec![url.to_string()])
    }
}

#[cfg(target_os = "windows")]
fn open_browser_url_via_shell_execute(url: &str) -> Result<(), ToolFailure> {
    use std::ptr::null;
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let operation = wide_null("open");
    let target = wide_null(url);
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            operation.as_ptr(),
            target.as_ptr(),
            null(),
            null(),
            SW_SHOWNORMAL,
        )
    } as usize;

    if result > 32 {
        Ok(())
    } else {
        Err(ToolFailure::permanent(format!(
            "failed to open URL via ShellExecuteW (code {result})"
        )))
    }
}

#[cfg(target_os = "windows")]
fn wide_null(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

fn browser_application_command(
    browser: BrowserApplication,
) -> Result<(String, Vec<String>), ToolFailure> {
    #[cfg(target_os = "macos")]
    {
        Ok((
            "open".to_string(),
            vec!["-a".to_string(), browser.display_name().to_string()],
        ))
    }

    #[cfg(target_os = "windows")]
    {
        Ok((
            "cmd".to_string(),
            vec![
                "/C".to_string(),
                "start".to_string(),
                String::new(),
                browser.windows_launcher().to_string(),
            ],
        ))
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        Ok((browser.linux_launcher()?.to_string(), Vec::new()))
    }
}

fn run_launcher(program: &str, args: &[String]) -> Result<(), ToolFailure> {
    run_launcher_with(program, args, |program, args| {
        Command::new(program)
            .args(args)
            .status()
            .map(|status| status.success())
    })
}

fn run_launcher_with<Run>(program: &str, args: &[String], mut run: Run) -> Result<(), ToolFailure>
where
    Run: FnMut(&str, &[String]) -> std::io::Result<bool>,
{
    match run(program, args) {
        Ok(true) => Ok(()),
        Ok(false) => Err(ToolFailure::permanent(format!("{program} command failed"))),
        Err(error) => Err(tool_failure_from_io(error)),
    }
}

async fn run_local_action<Run>(run: Run) -> Result<String, ToolFailure>
where
    Run: FnOnce() -> Result<String, ToolFailure> + Send + 'static,
{
    tokio::task::spawn_blocking(run)
        .await
        .map_err(|error| ToolFailure::unknown(format!("local action task failed: {error}")))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_browser_url_rejects_non_http_scheme() {
        let error = validate_browser_url("ftp://example.com").expect_err("should reject");
        assert_eq!(error.message, "url must start with http:// or https://");
    }

    #[test]
    fn validate_browser_url_accepts_query_parameters() {
        let url = validate_browser_url("https://example.com/path?a=1&b=2")
            .expect("query string should remain valid");
        assert_eq!(url, "https://example.com/path?a=1&b=2");
    }

    #[test]
    fn validate_browser_url_rejects_invalid_http_url() {
        let error = validate_browser_url("https://").expect_err("malformed URL should be rejected");
        assert!(
            error
                .message
                .starts_with("invalid http:// or https:// url:"),
            "unexpected error: {}",
            error.message
        );
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn open_browser_url_uses_platform_launcher_contract() {
        let mut captured: Option<(String, Vec<String>)> = None;
        let url = "https://example.com";
        let (program, args) = browser_url_command(url);

        run_launcher_with(program, &args, |program, args| {
            captured = Some((program.to_string(), args.to_vec()));
            Ok(true)
        })
        .expect("launcher should succeed");

        let (program, args) = captured.expect("captured command");
        assert!(!program.is_empty());
        assert!(args.iter().any(|value| value == url));
    }

    #[test]
    fn open_browser_application_uses_platform_launcher_contract() {
        let mut captured: Option<(String, Vec<String>)> = None;
        let (program, args) =
            browser_application_command(BrowserApplication::Chrome).expect("command");

        run_launcher_with(&program, &args, |program, args| {
            captured = Some((program.to_string(), args.to_vec()));
            Ok(true)
        })
        .expect("launcher should succeed");

        let (program, args) = captured.expect("captured command");
        assert!(!program.is_empty());

        #[cfg(target_os = "macos")]
        assert!(args.iter().any(|value| value == "Google Chrome"));
        #[cfg(target_os = "windows")]
        assert!(args.iter().any(|value| value == "chrome"));
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            assert_eq!(program, "google-chrome");
            assert!(args.is_empty());
        }
    }

    #[test]
    fn run_launcher_with_surfaces_failed_exit() {
        let error = run_launcher_with("demo", &[], |_program, _args| Ok(false))
            .expect_err("non-zero launcher should fail");
        assert_eq!(error.message, "demo command failed");
    }
}
