use crate::startup::fawx_data_dir;
use anyhow::anyhow;
use chrono::{TimeZone, Utc};
use clap::Args;
use fx_session::{
    render_content_blocks_with_options, ContentRenderOptions, SessionInfo, SessionKey, SessionKind,
    SessionMessage, SessionRegistry,
};
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Args)]
pub struct ListArgs {
    /// Output as JSON
    #[arg(long)]
    pub json: bool,
    /// Filter by session kind
    #[arg(long)]
    pub kind: Option<String>,
}

#[derive(Debug, Clone, Args)]
pub struct ExportArgs {
    /// Session ID
    pub id: String,
    /// Output as JSON
    #[arg(long)]
    pub json: bool,
    /// Limit to last N messages
    #[arg(long)]
    pub limit: Option<usize>,
}

#[derive(Debug)]
struct SessionExport {
    info: SessionInfo,
    messages: Vec<SessionMessage>,
}

pub fn run_list(args: &ListArgs) -> anyhow::Result<i32> {
    let filter = parse_kind_filter(args.kind.as_deref())?;
    let sessions = load_session_infos_from(&session_db_path(), filter)?;
    print_output(args.json, &sessions, || render_list_table(&sessions))?;
    Ok(0)
}

pub fn run_export(args: &ExportArgs) -> anyhow::Result<i32> {
    let export = load_session_export_from(&session_db_path(), &args.id, args.limit)?;
    print_output(args.json, &export.messages, || render_export_text(&export))?;
    Ok(0)
}

fn session_db_path() -> PathBuf {
    fawx_data_dir().join("sessions.redb")
}

fn load_session_infos_from(
    db_path: &Path,
    filter: Option<SessionKind>,
) -> anyhow::Result<Vec<SessionInfo>> {
    let Some(registry) = open_registry_from(db_path)? else {
        return Ok(Vec::new());
    };
    let mut sessions = registry.list(filter)?;
    sort_sessions(&mut sessions);
    Ok(sessions)
}

fn load_session_export_from(
    db_path: &Path,
    id: &str,
    limit: Option<usize>,
) -> anyhow::Result<SessionExport> {
    let key = SessionKey::new(id.to_string())?;
    let Some(registry) = open_registry_from(db_path)? else {
        return Err(anyhow!("session not found: {}", key));
    };
    let info = registry.get_info(&key)?;
    let messages = registry.history(&key, limit.unwrap_or(info.message_count))?;
    Ok(SessionExport { info, messages })
}

fn open_registry_from(db_path: &Path) -> anyhow::Result<Option<SessionRegistry>> {
    if !db_path.exists() {
        return Ok(None);
    }
    SessionRegistry::open(db_path)
        .map(Some)
        .ok_or_else(|| anyhow!("failed to open session registry at {}", db_path.display()))
}

fn parse_kind_filter(raw: Option<&str>) -> anyhow::Result<Option<SessionKind>> {
    raw.map(parse_kind).transpose()
}

fn parse_kind(raw: &str) -> anyhow::Result<SessionKind> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "main" => Ok(SessionKind::Main),
        "subagent" => Ok(SessionKind::Subagent),
        "channel" => Ok(SessionKind::Channel),
        "cron" => Ok(SessionKind::Cron),
        _ => Err(anyhow!(
            "invalid session kind '{}'; expected one of: main, subagent, channel, cron",
            raw
        )),
    }
}

fn sort_sessions(sessions: &mut [SessionInfo]) {
    sessions.sort_by(|left, right| {
        right
            .updated_at
            .cmp(&left.updated_at)
            .then_with(|| left.key.as_str().cmp(right.key.as_str()))
    });
}

fn render_list_table(sessions: &[SessionInfo]) -> String {
    if sessions.is_empty() {
        return "No sessions found.\n".to_string();
    }
    let id_width = list_id_width(sessions);
    let header = format_list_header(id_width);
    let rows = sessions
        .iter()
        .map(|info| format_list_row(info, id_width))
        .collect::<Vec<_>>()
        .join("\n");
    format!("{header}\n{rows}\n")
}

fn list_id_width(sessions: &[SessionInfo]) -> usize {
    sessions
        .iter()
        .map(|info| info.key.as_str().len())
        .max()
        .unwrap_or(2)
        .max(36)
}

fn format_list_header(id_width: usize) -> String {
    format!(
        "{:<id_width$}  {:<9}  {:<10}  {:<14}  {:>8}  {:<16}  {}",
        "ID", "KIND", "STATUS", "MODEL", "MESSAGES", "UPDATED", "LABEL",
    )
}

fn format_list_row(info: &SessionInfo, id_width: usize) -> String {
    format!(
        "{:<id_width$}  {:<9}  {:<10}  {:<14}  {:>8}  {:<16}  {}",
        info.key.as_str(),
        info.kind,
        info.status,
        truncate_cell(&info.model, 14),
        info.message_count,
        format_minute_timestamp(info.updated_at),
        display_label(info.label.as_deref()),
    )
}

fn render_export_text(export: &SessionExport) -> String {
    let mut output = export_header(export);
    if export.messages.is_empty() {
        return output;
    }
    let blocks = export
        .messages
        .iter()
        .map(format_message_block)
        .collect::<Vec<_>>()
        .join("\n\n");
    output.push('\n');
    output.push_str(&blocks);
    output.push('\n');
    output
}

fn export_header(export: &SessionExport) -> String {
    format!(
        "Session: {}\nKind: {} | Status: {} | Model: {}\nCreated: {} | Updated: {}\n{}\n---\n",
        export.info.key,
        export.info.kind,
        export.info.status,
        export.info.model,
        format_minute_timestamp(export.info.created_at),
        format_minute_timestamp(export.info.updated_at),
        format_message_count(export),
    )
}

fn format_message_count(export: &SessionExport) -> String {
    if export.messages.len() == export.info.message_count {
        return format!("Messages: {}", export.info.message_count);
    }
    format!(
        "Messages: {} (showing {})",
        export.info.message_count,
        export.messages.len()
    )
}

fn format_message_block(message: &SessionMessage) -> String {
    let token_suffix = format_token_suffix(message);
    format!(
        "[{}] {}{}\n{}",
        message.role,
        format_second_timestamp(message.timestamp),
        token_suffix,
        render_message_content(message)
    )
}

fn render_message_content(message: &SessionMessage) -> String {
    render_content_blocks_with_options(
        &message.content,
        ContentRenderOptions {
            include_tool_use_id: true,
        },
    )
}

fn format_token_suffix(message: &SessionMessage) -> String {
    match (
        message.total_token_count(),
        message.input_token_count,
        message.output_token_count,
    ) {
        (Some(total), Some(input), Some(output)) => {
            format!(" | {total} tokens ({input} in / {output} out)")
        }
        (Some(total), _, _) => format!(" | {total} tokens"),
        (None, _, _) => String::new(),
    }
}

fn truncate_cell(text: &str, max_chars: usize) -> String {
    let mut chars = text.chars();
    let prefix: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{prefix}…")
    } else {
        text.to_string()
    }
}

fn display_label(label: Option<&str>) -> &str {
    label.filter(|value| !value.is_empty()).unwrap_or("-")
}

fn format_minute_timestamp(timestamp: u64) -> String {
    format_timestamp(timestamp, "%Y-%m-%d %H:%M", "1970-01-01 00:00")
}

fn format_second_timestamp(timestamp: u64) -> String {
    format_timestamp(timestamp, "%Y-%m-%d %H:%M:%S", "1970-01-01 00:00:00")
}

fn format_timestamp(timestamp: u64, pattern: &str, fallback: &str) -> String {
    match Utc.timestamp_opt(timestamp as i64, 0).single() {
        Some(value) => value.format(pattern).to_string(),
        None => fallback.to_string(),
    }
}

fn print_output<T>(
    json: bool,
    value: &T,
    render_text: impl FnOnce() -> String,
) -> anyhow::Result<()>
where
    T: Serialize,
{
    if json {
        println!("{}", serde_json::to_string_pretty(value)?);
    } else {
        print!("{}", render_text());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use fx_session::{MessageRole, SessionConfig, SessionContentBlock};
    use tempfile::TempDir;

    fn db_path(temp_dir: &TempDir) -> PathBuf {
        temp_dir.path().join("sessions.redb")
    }

    fn test_registry(temp_dir: &TempDir) -> SessionRegistry {
        SessionRegistry::open(&db_path(temp_dir)).expect("open session registry")
    }

    fn create_session(
        registry: &SessionRegistry,
        id: &str,
        kind: SessionKind,
        label: Option<&str>,
    ) -> SessionKey {
        let key = SessionKey::new(id).expect("session key");
        registry
            .create(
                key.clone(),
                kind,
                SessionConfig {
                    label: label.map(ToString::to_string),
                    model: "gpt-4o-mini".to_string(),
                },
            )
            .expect("create session");
        key
    }

    #[test]
    fn list_sessions_empty_registry() {
        let temp_dir = TempDir::new().expect("tempdir");

        let sessions = load_session_infos_from(&db_path(&temp_dir), None).expect("list sessions");

        assert!(sessions.is_empty());
    }

    #[test]
    fn list_sessions_with_filter() {
        let temp_dir = TempDir::new().expect("tempdir");
        let registry = test_registry(&temp_dir);
        create_session(&registry, "main-1", SessionKind::Main, Some("primary"));
        create_session(&registry, "sub-1", SessionKind::Subagent, Some("reviewer"));
        drop(registry);

        let sessions = load_session_infos_from(&db_path(&temp_dir), Some(SessionKind::Subagent))
            .expect("list sessions");

        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].key.as_str(), "sub-1");
        assert_eq!(sessions[0].kind, SessionKind::Subagent);
    }

    #[test]
    fn export_session_shows_messages() {
        let temp_dir = TempDir::new().expect("tempdir");
        let registry = test_registry(&temp_dir);
        let key = create_session(&registry, "main-1", SessionKind::Main, Some("primary"));
        registry
            .record_message(&key, MessageRole::User, "What's the weather?")
            .expect("record user message");
        registry
            .record_message(&key, MessageRole::Assistant, "It's 45°F and clear.")
            .expect("record assistant message");
        drop(registry);

        let export = load_session_export_from(&db_path(&temp_dir), "main-1", None).expect("export");
        let rendered = render_export_text(&export);
        let user_index = rendered.find("What's the weather?").expect("user text");
        let assistant_index = rendered
            .find("It's 45°F and clear.")
            .expect("assistant text");

        assert!(rendered.contains("[user]"));
        assert!(rendered.contains("[assistant]"));
        assert!(user_index < assistant_index);
    }

    #[test]
    fn export_nonexistent_session_returns_error() {
        let temp_dir = TempDir::new().expect("tempdir");

        let error = load_session_export_from(&db_path(&temp_dir), "missing", None)
            .expect_err("missing session should fail");

        assert!(error.to_string().contains("session not found: missing"));
    }

    #[test]
    fn export_with_limit() {
        let temp_dir = TempDir::new().expect("tempdir");
        let registry = test_registry(&temp_dir);
        let key = create_session(&registry, "main-1", SessionKind::Main, Some("primary"));
        for content in ["first", "second", "third"] {
            registry
                .record_message(&key, MessageRole::User, content)
                .expect("record message");
        }
        drop(registry);

        let export = load_session_export_from(&db_path(&temp_dir), "main-1", Some(2))
            .expect("export with limit");
        let contents = export
            .messages
            .iter()
            .map(render_message_content)
            .collect::<Vec<_>>();

        assert_eq!(contents, vec!["second", "third"]);
    }

    #[test]
    fn export_renders_structured_tool_messages() {
        let temp_dir = TempDir::new().expect("tempdir");
        let registry = test_registry(&temp_dir);
        let key = create_session(&registry, "tool-1", SessionKind::Main, Some("primary"));
        registry
            .record_message_blocks(
                &key,
                MessageRole::Assistant,
                vec![
                    SessionContentBlock::Text {
                        text: "Let me check.".to_string(),
                    },
                    SessionContentBlock::ToolUse {
                        id: "call_1".to_string(),
                        provider_id: None,
                        name: "read_file".to_string(),
                        input: serde_json::json!({"path": "README.md"}),
                    },
                ],
                Some(17),
            )
            .expect("record tool use");
        registry
            .record_message_blocks(
                &key,
                MessageRole::Tool,
                vec![SessionContentBlock::ToolResult {
                    tool_use_id: "call_1".to_string(),
                    content: serde_json::json!("file contents"),
                    is_error: Some(false),
                }],
                None,
            )
            .expect("record tool result");
        drop(registry);

        let export = load_session_export_from(&db_path(&temp_dir), "tool-1", None).expect("export");
        let rendered = render_export_text(&export);

        assert!(rendered.contains("[assistant]"));
        assert!(rendered.contains("| 17 tokens"));
        assert!(rendered.contains("[tool_use:read_file#call_1]"));
        assert!(rendered.contains("[tool_result:call_1]"));
        assert!(rendered.contains("[tool]"));
    }

    #[test]
    fn parse_kind_filter_rejects_unknown_value() {
        let error = parse_kind_filter(Some("weird")).expect_err("invalid kind should fail");

        assert!(error.to_string().contains("invalid session kind 'weird'"));
    }
}
