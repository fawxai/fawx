use crate::startup::StartupError;
use crossterm::{event, terminal};
use std::{
    io::{self, Write},
    process::Command,
};

const MAX_PROMPT_RETRIES: usize = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PromptSurface {
    PlainTerminal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AuthSelection {
    ClaudeSubscription,
    ChatGptSubscription,
    ApiKey,
    Skip,
}

pub(crate) fn parse_auth_selection(value: &str) -> Option<AuthSelection> {
    match value.trim() {
        "1" => Some(AuthSelection::ClaudeSubscription),
        "2" => Some(AuthSelection::ChatGptSubscription),
        "3" => Some(AuthSelection::ApiKey),
        "" | "4" | "s" | "skip" => Some(AuthSelection::Skip),
        _ => None,
    }
}

pub(crate) fn prompt_choice_with_surface<T, F>(
    surface: PromptSurface,
    prompt: &str,
    invalid_message: &str,
    context: &str,
    parser: F,
) -> Result<T, StartupError>
where
    F: Fn(&str) -> Option<T>,
{
    run_prompt_on_surface(surface, || {
        prompt_choice_inner(prompt, invalid_message, context, parser)
    })
}

pub(crate) fn prompt_line(prompt: &str) -> Result<String, StartupError> {
    ensure_cooked_mode();
    eprint!("{prompt}");
    io::stdout().flush().map_err(StartupError::Io)?;

    let mut input = String::new();
    let bytes = io::stdin()
        .read_line(&mut input)
        .map_err(StartupError::Io)?;
    if bytes == 0 {
        return Err(StartupError::Auth("stdin closed unexpectedly".to_string()));
    }

    Ok(input.trim().to_string())
}

pub(crate) fn prompt_non_empty_line_with_surface(
    surface: PromptSurface,
    prompt: &str,
    empty_message: &str,
    context: &str,
) -> Result<String, StartupError> {
    run_prompt_on_surface(surface, || {
        prompt_non_empty_line_inner(prompt, empty_message, context)
    })
}

pub(crate) fn prompt_api_key_provider_with_surface(
    surface: PromptSurface,
) -> Result<String, StartupError> {
    run_prompt_on_surface(surface, || {
        eprintln!("Which provider?");
        eprintln!("  [1] Anthropic");
        eprintln!("  [2] OpenAI");
        eprintln!("  [3] OpenRouter");
        eprintln!("  [4] Other (OpenAI-compatible)");
        eprintln!();

        let choice = prompt_choice_inner(
            "> ",
            "Please choose 1, 2, 3, or 4.",
            "API key provider selection",
            parse_api_key_provider_selection,
        )?;

        match choice {
            ApiKeyProvider::Anthropic => Ok("anthropic".to_string()),
            ApiKeyProvider::OpenAi => Ok("openai".to_string()),
            ApiKeyProvider::OpenRouter => Ok("openrouter".to_string()),
            ApiKeyProvider::Other => prompt_non_empty_line_inner(
                "Provider name: ",
                "Provider name cannot be empty.",
                "API key provider name",
            ),
        }
    })
}

pub(crate) fn prompt_non_empty_secret_with_surface(
    surface: PromptSurface,
    prompt: &str,
    empty_message: &str,
    context: &str,
) -> Result<String, StartupError> {
    run_prompt_on_surface(surface, || {
        prompt_secret_loop(prompt, empty_message, context)
    })
}

#[allow(clippy::needless_return)]
pub(crate) fn open_browser(url: &str) -> io::Result<()> {
    #[cfg(target_os = "macos")]
    {
        return run_browser_command("open", &[url]);
    }

    #[cfg(target_os = "windows")]
    {
        return run_browser_command("cmd", &["/C", "start", url]);
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        run_browser_command("xdg-open", &[url])
    }
}

fn run_browser_command(program: &str, args: &[&str]) -> io::Result<()> {
    open_browser_with(program, args, |program, args| {
        Command::new(program)
            .args(args)
            .status()
            .map(|status| status.success())
    })
}

fn open_browser_with<Run>(program: &str, args: &[&str], mut run: Run) -> io::Result<()>
where
    Run: FnMut(&str, &[&str]) -> io::Result<bool>,
{
    if run(program, args)? {
        Ok(())
    } else {
        Err(io::Error::other(format!("{program} command failed")))
    }
}

fn run_prompt_on_surface<T>(
    _surface: PromptSurface,
    f: impl FnOnce() -> Result<T, StartupError>,
) -> Result<T, StartupError> {
    f()
}

pub(crate) fn ensure_cooked_mode() {
    ensure_cooked_mode_with(
        || {
            terminal::disable_raw_mode().ok();
        },
        drain_stdin,
    );
}

fn prompt_choice_inner<T, F>(
    prompt: &str,
    invalid_message: &str,
    context: &str,
    parser: F,
) -> Result<T, StartupError>
where
    F: Fn(&str) -> Option<T>,
{
    for _ in 0..MAX_PROMPT_RETRIES {
        let value = prompt_line(prompt)?;
        if let Some(parsed) = parser(&value) {
            return Ok(parsed);
        }
        eprint!("{invalid_message}");
    }

    Err(retry_limit_error(context))
}

fn prompt_non_empty_line_inner(
    prompt: &str,
    empty_message: &str,
    context: &str,
) -> Result<String, StartupError> {
    prompt_non_empty_line_with_reader(prompt, empty_message, context, prompt_line)
}

fn prompt_non_empty_line_with_reader<F>(
    prompt: &str,
    empty_message: &str,
    context: &str,
    mut read: F,
) -> Result<String, StartupError>
where
    F: FnMut(&str) -> Result<String, StartupError>,
{
    for _ in 0..MAX_PROMPT_RETRIES {
        let value = read(prompt)?;
        if !value.is_empty() {
            return Ok(value);
        }
        eprint!("{empty_message}");
    }

    Err(retry_limit_error(context))
}

fn prompt_secret_loop(
    prompt: &str,
    empty_message: &str,
    context: &str,
) -> Result<String, StartupError> {
    for _ in 0..MAX_PROMPT_RETRIES {
        let value = prompt_secret(prompt)?;
        if !value.is_empty() {
            return Ok(value);
        }
        eprint!("{empty_message}");
    }

    Err(retry_limit_error(context))
}

fn prompt_secret(prompt: &str) -> Result<String, StartupError> {
    eprint!("{prompt}");
    io::stdout().flush().map_err(StartupError::Io)?;

    let _guard = RawModeGuard::new()?;
    let mut value = String::new();
    read_secret_input(&mut value)?;

    let trimmed = value.trim().to_string();
    if trimmed.is_empty() {
        eprintln!();
    } else {
        eprint!(" ({} chars)", trimmed.len());
    }

    Ok(trimmed)
}

fn read_secret_input(value: &mut String) -> Result<(), StartupError> {
    let mut display_len: usize = 0;

    loop {
        let event = event::read().map_err(StartupError::Io)?;
        if let event::Event::Key(key_event) = event {
            match classify_secret_input_key(&key_event) {
                SecretInputKeyAction::Submit => return Ok(()),
                SecretInputKeyAction::Cancel => return Err(StartupError::Cancelled),
                SecretInputKeyAction::Ignore => {}
                SecretInputKeyAction::Type(ch) => {
                    value.push(ch);
                    display_len += 1;
                    eprint!("•");
                    io::stdout().flush().map_err(StartupError::Io)?;
                }
                SecretInputKeyAction::Delete => {
                    if value.pop().is_some() && display_len > 0 {
                        display_len -= 1;
                        eprint!("\x08 \x08");
                        io::stdout().flush().map_err(StartupError::Io)?;
                    }
                }
            }
        }
    }
}

fn retry_limit_error(context: &str) -> StartupError {
    StartupError::Auth(format!("maximum input retries exceeded for {context}"))
}

pub(crate) fn ensure_cooked_mode_with<DisableRawMode, DrainStdin>(
    mut disable_raw_mode: DisableRawMode,
    mut drain_stdin: DrainStdin,
) where
    DisableRawMode: FnMut(),
    DrainStdin: FnMut(),
{
    disable_raw_mode();
    drain_stdin();
}

fn drain_stdin() {
    #[cfg(unix)]
    {
        drain_stdin_with(drain_stdin_input_queue, log_drain_stdin_error);
    }
}

#[cfg(unix)]
pub(crate) fn drain_stdin_with<Drain, Log>(mut drain: Drain, mut log_error: Log)
where
    Drain: FnMut() -> io::Result<()>,
    Log: FnMut(&io::Error),
{
    if let Err(error) = drain() {
        log_error(&error);
    }
}

#[cfg(unix)]
fn drain_stdin_input_queue() -> io::Result<()> {
    flush_stdin_input_queue_with(|fd, queue_selector| unsafe { libc::tcflush(fd, queue_selector) })
}

#[cfg(unix)]
fn log_drain_stdin_error(error: &io::Error) {
    if is_benign_stdin_flush_error(error) {
        tracing::debug!(
            errno = ?error.raw_os_error(),
            error = %error,
            "skipping stdin input queue flush because stdin is not a tty"
        );
        return;
    }

    tracing::warn!(
        errno = ?error.raw_os_error(),
        error = %error,
        "failed to flush stdin input queue"
    );
}

#[cfg(unix)]
pub(crate) fn is_benign_stdin_flush_error(error: &io::Error) -> bool {
    error.raw_os_error() == Some(libc::ENOTTY)
}

#[cfg(unix)]
pub(crate) fn flush_stdin_input_queue_with<F>(mut flush: F) -> io::Result<()>
where
    F: FnMut(i32, i32) -> i32,
{
    let status = flush(libc::STDIN_FILENO, libc::TCIFLUSH);
    if status == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ApiKeyProvider {
    Anthropic,
    OpenAi,
    OpenRouter,
    Other,
}

fn parse_api_key_provider_selection(value: &str) -> Option<ApiKeyProvider> {
    match value.trim() {
        "1" => Some(ApiKeyProvider::Anthropic),
        "2" => Some(ApiKeyProvider::OpenAi),
        "3" => Some(ApiKeyProvider::OpenRouter),
        "4" => Some(ApiKeyProvider::Other),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SecretInputKeyAction {
    Submit,
    Cancel,
    Type(char),
    Delete,
    Ignore,
}

pub(crate) fn classify_secret_input_key(key_event: &event::KeyEvent) -> SecretInputKeyAction {
    if is_ctrl_c(key_event) {
        return SecretInputKeyAction::Cancel;
    }

    match key_event.code {
        event::KeyCode::Enter => SecretInputKeyAction::Submit,
        event::KeyCode::Esc => SecretInputKeyAction::Cancel,
        event::KeyCode::Char(ch) => SecretInputKeyAction::Type(ch),
        event::KeyCode::Backspace => SecretInputKeyAction::Delete,
        _ => SecretInputKeyAction::Ignore,
    }
}

fn is_ctrl_c(key_event: &event::KeyEvent) -> bool {
    key_event.code == event::KeyCode::Char('c')
        && key_event.modifiers.contains(event::KeyModifiers::CONTROL)
}

struct RawModeGuard;

impl RawModeGuard {
    fn new() -> io::Result<Self> {
        terminal::enable_raw_mode()?;
        Ok(Self)
    }
}

impl Drop for RawModeGuard {
    fn drop(&mut self) {
        if let Err(error) = terminal::disable_raw_mode() {
            eprintln!("failed to disable raw mode: {error}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auth_wizard_selection_parsing_covers_all_options() {
        assert_eq!(
            parse_auth_selection("1"),
            Some(AuthSelection::ClaudeSubscription)
        );
        assert_eq!(
            parse_auth_selection("2"),
            Some(AuthSelection::ChatGptSubscription)
        );
        assert_eq!(parse_auth_selection("3"), Some(AuthSelection::ApiKey));
        assert_eq!(parse_auth_selection("9"), None);
    }

    #[test]
    fn api_key_provider_selection_parses_all_options() {
        assert_eq!(
            parse_api_key_provider_selection("1"),
            Some(ApiKeyProvider::Anthropic)
        );
        assert_eq!(
            parse_api_key_provider_selection("2"),
            Some(ApiKeyProvider::OpenAi)
        );
        assert_eq!(
            parse_api_key_provider_selection("3"),
            Some(ApiKeyProvider::OpenRouter)
        );
        assert_eq!(
            parse_api_key_provider_selection("4"),
            Some(ApiKeyProvider::Other)
        );
        assert_eq!(parse_api_key_provider_selection("5"), None);
    }

    #[test]
    fn ensure_cooked_mode_disables_raw_mode() {
        if terminal::enable_raw_mode().is_err() {
            return;
        }

        ensure_cooked_mode();
        assert!(!terminal::is_raw_mode_enabled().unwrap_or(false));
    }

    #[test]
    fn prompt_non_empty_line_with_reader_stops_after_retry_limit() {
        let mut calls = 0;
        let result = prompt_non_empty_line_with_reader("> ", "try again\n", "test prompt", |_| {
            calls += 1;
            Ok(String::new())
        });

        assert!(
            matches!(result, Err(StartupError::Auth(message)) if message.contains("maximum input retries exceeded"))
        );
        assert_eq!(calls, MAX_PROMPT_RETRIES);
    }

    #[test]
    fn open_browser_with_runner_returns_error_for_failed_command() {
        let error = open_browser_with("demo", &["https://example.com"], |_program, _args| {
            Ok(false)
        })
        .expect_err("failed launcher should bubble up as an io error");

        assert_eq!(error.kind(), io::ErrorKind::Other);
        assert!(error.to_string().contains("demo command failed"));
    }

    #[test]
    fn open_browser_with_runner_passes_program_and_args() {
        let mut seen = Vec::new();
        open_browser_with("demo", &["alpha", "beta"], |program, args| {
            seen.push((
                program.to_string(),
                args.iter().map(|arg| arg.to_string()).collect::<Vec<_>>(),
            ));
            Ok(true)
        })
        .expect("successful launcher");

        assert_eq!(
            seen,
            vec![(
                "demo".to_string(),
                vec!["alpha".to_string(), "beta".to_string()]
            )]
        );
    }
}
