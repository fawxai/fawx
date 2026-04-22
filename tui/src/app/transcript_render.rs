use super::*;

pub(super) fn phase_separator_line(phase: TuiRenderPhase) -> Line<'static> {
    let (label, style) = match phase {
        TuiRenderPhase::Working => ("· · Working", Style::default().fg(Color::DarkGray)),
        TuiRenderPhase::Activity => ("╌╌ Activity", Style::default().fg(Color::LightBlue)),
        TuiRenderPhase::Summary => (
            "━━ Completed work",
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ),
        TuiRenderPhase::Response => (
            "══ Response",
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ),
    };
    Line::from(vec![Span::styled(label, style)])
}

pub(super) fn render_activity_group_entry(
    group: &TuiActivityGroup,
    width: usize,
    expanded: bool,
    focused: bool,
    active_marker: Option<&str>,
) -> Vec<Line<'static>> {
    let content_width = tool_content_width(width);
    let mut lines = Vec::new();
    lines.push(Line::from(vec![Span::styled(
        activity_group_title(group, expanded, active_marker),
        activity_group_header_style(group, focused),
    )]));

    if let (true, Some(narration)) = (group.is_live || expanded, group.narration.as_ref()) {
        lines.extend(
            wrap_tool_output_text(&sanitize_terminal_text(narration), content_width)
                .into_iter()
                .map(|line| line.patch_style(Style::default().fg(Color::Gray))),
        );
    }

    if group.is_live || expanded {
        for call in &group.tool_calls {
            lines.extend(render_activity_tool_call(call, content_width, true));
        }
    }

    prefix_tool_lines(lines, EntryRole::ActivityGroup)
}

fn activity_group_title(
    group: &TuiActivityGroup,
    expanded: bool,
    active_marker: Option<&str>,
) -> String {
    let label = group
        .title
        .as_deref()
        .filter(|title| !title.trim().is_empty())
        .unwrap_or({
            if group.is_live {
                "Working"
            } else {
                "Completed work"
            }
        });
    let disclosure = if group.is_live {
        active_marker.unwrap_or("◌")
    } else if expanded {
        "▾"
    } else {
        "▸"
    };
    let mut parts = vec![format!("{disclosure} {label}")];
    if let Some(progress) = activity_group_live_progress_summary(group) {
        parts.push(progress);
    }
    if let Some(summary) = activity_group_tool_summary(group) {
        parts.push(summary);
    }
    if !group.is_live && !expanded {
        if let Some(summary) = activity_group_collapsed_tool_summary(group) {
            parts.push(summary);
        }
    }
    parts.join(" · ")
}

fn activity_group_live_progress_summary(group: &TuiActivityGroup) -> Option<String> {
    if !group.is_live {
        return None;
    }

    let progress = group
        .tool_calls
        .iter()
        .rev()
        .find_map(|call| call.progress.as_ref())?;
    let outcome = progress.outcome.replace('_', " ");
    let outcome = outcome.trim();
    let target = progress
        .target
        .as_deref()
        .and_then(non_empty_trimmed_str)
        .map(|target| preview_text(&target, TOOL_VALUE_PREVIEW_CHARS));

    match (outcome.is_empty(), target) {
        (true, None) => None,
        (true, Some(target)) => Some(target),
        (false, None) => Some(outcome.to_string()),
        (false, Some(target)) => Some(format!("{outcome} {target}")),
    }
}

fn activity_group_tool_summary(group: &TuiActivityGroup) -> Option<String> {
    if group.tool_calls.is_empty() {
        return None;
    }

    let mut by_kind = std::collections::BTreeMap::<(&'static str, &'static str), usize>::new();
    for call in &group.tool_calls {
        *by_kind.entry(activity_kind_labels(&call.name)).or_default() += 1;
    }
    let mut pieces = by_kind
        .into_iter()
        .map(|((singular, plural), count)| {
            let label = if count == 1 { singular } else { plural };
            format!("{count} {label}")
        })
        .collect::<Vec<_>>();
    if group.is_live && group.running_count() > 0 {
        pieces.push(format!("{} running", group.running_count()));
    }
    if group.error_count() > 0 {
        pieces.push(format!("{} failed", group.error_count()));
    }
    Some(pieces.join(", "))
}

fn activity_group_collapsed_tool_summary(group: &TuiActivityGroup) -> Option<String> {
    if group.tool_calls.is_empty() {
        return None;
    }

    let mut details = group
        .tool_calls
        .iter()
        .take(3)
        .map(activity_tool_call_collapsed_label)
        .collect::<Vec<_>>();
    let remaining = group.tool_calls.len().saturating_sub(details.len());
    if remaining > 0 {
        details.push(format!("+{remaining} more"));
    }
    let call_count = group.tool_calls.len();
    let label = if call_count == 1 { "call" } else { "calls" };
    Some(format!("{} ({call_count} {label})", details.join(", ")))
}

fn activity_tool_call_collapsed_label(call: &TuiToolCall) -> String {
    if call.name == "run_command" {
        if let Some(command) = run_command_activity_target(call.arguments.as_ref()) {
            return command;
        }
    }

    let mut label =
        non_empty_trimmed_str(&call.name).unwrap_or_else(|| tool_label(Some(&call.name)));
    if let Some(target) = activity_tool_target(call) {
        label.push_str(" — ");
        label.push_str(&target);
    }
    label
}

fn activity_group_header_style(group: &TuiActivityGroup, focused: bool) -> Style {
    let style = if group.error_count() > 0 {
        Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
    } else if group.is_live {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD)
    };
    if focused {
        style.add_modifier(Modifier::REVERSED)
    } else {
        style
    }
}

fn render_activity_tool_call(
    call: &TuiToolCall,
    width: usize,
    include_preview: bool,
) -> Vec<Line<'static>> {
    let mut lines = vec![activity_tool_call_summary_line(call)];
    if include_preview {
        if let Some(result) = call.result.as_deref().and_then(non_empty_trimmed_str) {
            let preview = preview_tool_result(&result, width);
            if !preview.is_empty() {
                lines.extend(
                    wrap_tool_output_text(&format!("  {}", preview), width)
                        .into_iter()
                        .map(|line| line.patch_style(Style::default().fg(Color::DarkGray))),
                );
            }
        }
    }
    lines
}

fn activity_tool_call_summary_line(call: &TuiToolCall) -> Line<'static> {
    let (icon, style) = match call.success {
        Some(true) => ("✓", Style::default().fg(Color::Green)),
        Some(false) => (
            "✗",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        ),
        None => ("◌", Style::default().fg(Color::Yellow)),
    };
    let mut text = format!("{icon} {}", tool_label(Some(&call.name)));
    if let Some(target) = activity_tool_target(call) {
        text.push_str(" — ");
        text.push_str(&target);
    }
    if let Some(progress) = &call.progress {
        text.push_str(" · ");
        text.push_str(&progress.outcome.replace('_', " "));
    }
    Line::from(vec![Span::styled(text, style)])
}

fn activity_tool_target(call: &TuiToolCall) -> Option<String> {
    if call.name == "run_command" {
        if let Some(command) = run_command_activity_target(call.arguments.as_ref()) {
            return Some(command);
        }
    }

    if let Some(Value::Object(arguments)) = call.arguments.as_ref() {
        for key in ["path", "file", "filename", "url", "query", "pattern", "cmd"] {
            if let Some(value) = arguments.get(key) {
                return Some(summarize_tool_target_value(value, TOOL_VALUE_PREVIEW_CHARS));
            }
        }
    }

    call.progress
        .as_ref()
        .and_then(|progress| progress.target.as_deref())
        .and_then(|target| progress_target_display_text(&call.name, target))
        .map(|target| preview_text(&target, TOOL_VALUE_PREVIEW_CHARS))
}

fn run_command_activity_target(arguments: Option<&Value>) -> Option<String> {
    let Some(Value::Object(arguments)) = arguments else {
        return None;
    };
    let command = arguments
        .get("command")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|command| !command.is_empty())
        .map(str::to_string)
        .or_else(|| {
            arguments
                .get("argv")
                .and_then(Value::as_array)
                .map(|argv| {
                    // Display only: this is intentionally not shell escaping.
                    // The underlying tool already executed the exact argv
                    // vector; the header just needs a compact human label.
                    argv.iter()
                        .filter_map(Value::as_str)
                        .collect::<Vec<_>>()
                        .join(" ")
                })
                .filter(|command| !command.trim().is_empty())
        })?;
    Some(preview_text(&command, TOOL_VALUE_PREVIEW_CHARS))
}

fn progress_target_display_text(tool_name: &str, target: &str) -> Option<String> {
    let trimmed = non_empty_trimmed_str(target)?;
    if let Some(arguments) = trimmed
        .strip_prefix(tool_name)
        .and_then(|value| value.strip_prefix(':'))
        .and_then(|value| serde_json::from_str::<Value>(value).ok())
    {
        if tool_name == "run_command" {
            return run_command_activity_target(Some(&arguments));
        }
        if let Value::Object(map) = &arguments {
            for key in ["path", "file", "filename", "url", "query", "pattern", "cmd"] {
                if let Some(value) = map.get(key) {
                    return Some(summarize_tool_target_value(value, TOOL_VALUE_PREVIEW_CHARS));
                }
            }
        }
        return None;
    }
    Some(trimmed)
}

fn preview_tool_result(result: &str, width: usize) -> String {
    let collapsed = result
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .take(2)
        .collect::<Vec<_>>()
        .join(" / ");
    preview_text(&collapsed, width.saturating_sub(2).min(140))
}

pub(super) fn render_tool_use_entry(entry: &Entry, width: usize) -> Vec<Line<'static>> {
    let content_width = tool_content_width(width);
    let lines = wrap_tool_text_lines(&tool_use_summary_lines(entry, content_width), content_width);
    prefix_tool_lines(lines, EntryRole::ToolUse)
}

pub(super) fn render_tool_result_entry(
    entry: &Entry,
    width: usize,
    panel_visible: bool,
) -> Vec<Line<'static>> {
    let content_width = tool_content_width(width);
    let text = sanitize_terminal_text(&entry.text);
    let plan = tool_result_render_plan(&text, content_width);
    let mut lines = wrap_tool_text_lines(
        &tool_result_summary_lines(entry, &text, plan.summarize),
        content_width,
    );
    let budget = TOOL_RESULT_MAX_LINES.saturating_sub(lines.len());
    lines.extend(tool_result_preview_lines(
        entry,
        content_width,
        budget,
        panel_visible,
        plan,
    ));
    prefix_tool_lines(lines, entry.role)
}

fn tool_content_width(width: usize) -> usize {
    width.saturating_sub(TOOL_PREFIX_DISPLAY_WIDTH).max(1)
}

fn prefix_tool_lines(lines: Vec<Line<'static>>, role: EntryRole) -> Vec<Line<'static>> {
    let (initial, style) = tool_prefix(role);
    prefix_lines(
        lines,
        Span::styled(initial, style),
        Span::raw(tool_continuation_prefix()),
    )
}

fn tool_continuation_prefix() -> String {
    " ".repeat(TOOL_PREFIX_DISPLAY_WIDTH)
}

fn tool_prefix(role: EntryRole) -> (&'static str, Style) {
    match role {
        EntryRole::ActivityGroup => ("work · ", Style::default().fg(Color::LightBlue)),
        EntryRole::ToolUse => (TOOL_USE_PREFIX, Style::default().fg(Color::Magenta)),
        EntryRole::ToolResult => (TOOL_RESULT_PREFIX, Style::default().fg(Color::Green)),
        EntryRole::ToolError => (
            TOOL_ERROR_PREFIX,
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        ),
        _ => (TOOL_RESULT_PREFIX, Style::default()),
    }
}

fn wrap_tool_text_lines(lines: &[String], width: usize) -> Vec<Line<'static>> {
    let mut rendered = Vec::new();
    for line in lines {
        rendered.extend(wrap_tool_output_text(line, width));
    }
    if rendered.is_empty() {
        rendered.push(Line::default());
    }
    rendered
}

fn tool_use_summary_lines(entry: &Entry, width: usize) -> Vec<String> {
    let mut lines = vec![format!("▶ {}", tool_label(entry.tool_name.as_deref()))];
    if let Some(arguments) = &entry.tool_arguments {
        lines.extend(tool_argument_summary_lines(
            entry.tool_name.as_deref(),
            arguments,
            width,
        ));
    }
    truncate_tool_use_summary(lines)
}

fn truncate_tool_use_summary(lines: Vec<String>) -> Vec<String> {
    if lines.len() <= TOOL_USE_MAX_LINES {
        return lines;
    }
    let mut limited = lines[..TOOL_USE_MAX_LINES - 1].to_vec();
    limited.push(format!(
        "  … {} more fields",
        lines.len() - TOOL_USE_MAX_LINES + 1
    ));
    limited
}

fn tool_argument_summary_lines(
    tool_name: Option<&str>,
    arguments: &Value,
    width: usize,
) -> Vec<String> {
    match arguments {
        Value::Object(map) => tool_object_summary_lines(tool_name, map, width),
        other => vec![format!(
            "  args: {}",
            summarize_tool_value(other, width.saturating_sub(10))
        )],
    }
}

fn tool_object_summary_lines(
    tool_name: Option<&str>,
    map: &serde_json::Map<String, Value>,
    width: usize,
) -> Vec<String> {
    let fields = prioritized_tool_fields(tool_name, map);
    let mut lines = fields
        .into_iter()
        .take(TOOL_USE_FIELD_LIMIT)
        .map(|(key, value)| {
            let available = width.saturating_sub(key.len() + 7);
            format!("  {key}: {}", summarize_tool_value(value, available))
        })
        .collect::<Vec<_>>();
    let remaining = map.len().saturating_sub(TOOL_USE_FIELD_LIMIT);
    if remaining > 0 {
        lines.push(format!("  … {remaining} more fields"));
    }
    lines
}

fn prioritized_tool_fields<'a>(
    tool_name: Option<&str>,
    map: &'a serde_json::Map<String, Value>,
) -> Vec<(&'a String, &'a Value)> {
    let mut fields = map.iter().collect::<Vec<_>>();
    if tool_name == Some("run_experiment") {
        fields.sort_by_key(|(key, _)| (experiment_tool_argument_priority(key), key.as_str()));
    }
    fields
}

fn experiment_tool_argument_priority(key: &str) -> usize {
    match key {
        "signal" => 0,
        "hypothesis" => 1,
        "scope" => 2,
        "nodes" => 3,
        "mode" => 4,
        "timeout" => 5,
        _ => 6,
    }
}

fn summarize_tool_value(value: &Value, limit: usize) -> String {
    match value {
        Value::String(text) => {
            format!("\"{}\"", preview_text(&sanitize_terminal_text(text), limit))
        }
        Value::Array(items) => format!("[{} items]", items.len()),
        Value::Object(map) => format!("{{{} keys}}", map.len()),
        other => other.to_string(),
    }
}

fn summarize_tool_target_value(value: &Value, limit: usize) -> String {
    match value {
        Value::String(text) => preview_text(&sanitize_terminal_text(text), limit),
        Value::Array(items) => format!("[{} items]", items.len()),
        Value::Object(map) => format!("{{{} keys}}", map.len()),
        other => other.to_string(),
    }
}

fn preview_text(text: &str, limit: usize) -> String {
    let preview_limit = limit.clamp(4, TOOL_VALUE_PREVIEW_CHARS);
    truncate_text(
        &text.split_whitespace().collect::<Vec<_>>().join(" "),
        preview_limit,
    )
}

fn tool_result_summary_lines(entry: &Entry, text: &str, summarize_experiment: bool) -> Vec<String> {
    let status = if matches!(entry.role, EntryRole::ToolError) {
        "failure"
    } else {
        "success"
    };
    let label = tool_label(entry.tool_name.as_deref());
    let mut lines = vec![format!(
        "{} {label} ({status})",
        tool_status_icon(entry.role)
    )];
    if summarize_experiment {
        lines.extend(experiment_result_summary_lines(text));
    }
    lines
}

fn tool_label(name: Option<&str>) -> String {
    let sanitized = sanitize_terminal_text(name.unwrap_or("tool"));
    if sanitized.trim().is_empty() {
        "tool".to_string()
    } else {
        sanitized
    }
}

fn tool_status_icon(role: EntryRole) -> &'static str {
    if matches!(role, EntryRole::ToolError) {
        "✗"
    } else {
        "✓"
    }
}

fn experiment_result_summary_lines(text: &str) -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(decision) = find_prefixed_line(text, "Decision:") {
        lines.push(collapse_whitespace(decision));
    }
    if let Some(score_line) = find_experiment_score_line(text) {
        lines.push(collapse_whitespace(score_line));
    }
    if let Some(chain_entry) = find_line_containing(text, "Chain entry #") {
        lines.push(collapse_whitespace(chain_entry));
    }
    lines
}

fn find_prefixed_line<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    text.lines()
        .find(|line| line.trim_start().starts_with(prefix))
}

fn find_experiment_score_line(text: &str) -> Option<&str> {
    text.lines()
        .find(|line| line.contains("score:") && line.contains("WINNER"))
        .or_else(|| text.lines().find(|line| line.contains("score:")))
}

fn find_line_containing<'a>(text: &'a str, needle: &str) -> Option<&'a str> {
    text.lines().find(|line| line.contains(needle))
}

fn collapse_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn activity_kind_labels(name: &str) -> (&'static str, &'static str) {
    let normalized = name.to_ascii_lowercase();
    if normalized.contains("command") || normalized.contains("shell") {
        ("command", "commands")
    } else if normalized.contains("search") || normalized == "rg" || normalized == "grep" {
        ("search", "searches")
    } else if normalized.contains("edit")
        || normalized.contains("write")
        || normalized.contains("patch")
    {
        ("edit", "edits")
    } else if normalized.contains("read") || normalized.contains("file") || normalized == "ls" {
        ("file read", "file reads")
    } else {
        ("tool", "tools")
    }
}

struct ToolResultRenderPlan {
    wrapped_output: Vec<Line<'static>>,
    summarize: bool,
}

fn tool_result_preview_lines(
    entry: &Entry,
    width: usize,
    limit: usize,
    panel_visible: bool,
    plan: ToolResultRenderPlan,
) -> Vec<Line<'static>> {
    if limit == 0 {
        return Vec::new();
    }
    let total_lines = plan.wrapped_output.len();
    if plan.summarize {
        return experiment_notice_lines(entry, width, panel_visible, limit, total_lines);
    }
    truncate_wrapped_lines(
        plan.wrapped_output,
        width,
        limit,
        tool_result_notice(entry, panel_visible, total_lines),
    )
}

fn has_experiment_summary(text: &str) -> bool {
    !experiment_result_summary_lines(text).is_empty()
}

fn tool_result_render_plan(text: &str, width: usize) -> ToolResultRenderPlan {
    let wrapped_output = wrap_tool_output_text(text, width);
    let summarize = has_experiment_summary(text) && wrapped_output.len() > TOOL_RESULT_MAX_LINES;
    ToolResultRenderPlan {
        wrapped_output,
        summarize,
    }
}

fn experiment_notice_lines(
    entry: &Entry,
    width: usize,
    panel_visible: bool,
    limit: usize,
    total_lines: usize,
) -> Vec<Line<'static>> {
    let notice = wrap_plain_text(
        &tool_result_notice(entry, panel_visible, total_lines),
        width,
    );
    notice.into_iter().take(limit).collect()
}

fn truncate_wrapped_lines(
    wrapped: Vec<Line<'static>>,
    width: usize,
    limit: usize,
    notice: String,
) -> Vec<Line<'static>> {
    if wrapped.len() <= limit {
        return wrapped;
    }
    let notice_lines = wrap_tool_output_text(&notice, width);
    let keep = limit.saturating_sub(notice_lines.len());
    let mut out = wrapped.into_iter().take(keep).collect::<Vec<_>>();
    out.extend(
        notice_lines
            .into_iter()
            .take(limit.saturating_sub(out.len())),
    );
    out
}

fn tool_result_notice(entry: &Entry, panel_visible: bool, total_lines: usize) -> String {
    if panel_visible && entry.tool_name.as_deref() == Some("run_experiment") {
        return format!("[full output: {total_lines} lines — see Experiment panel →]");
    }
    format!("[full output: {total_lines} lines — truncated in transcript]")
}

fn wrap_tool_output_text(text: &str, width: usize) -> Vec<Line<'static>> {
    let mut out = Vec::new();
    let options = textwrap::Options::new(width).break_words(true);
    for raw_line in text.lines() {
        out.extend(
            textwrap::wrap(raw_line, &options)
                .into_iter()
                .map(|line| Line::from(line.into_owned())),
        );
    }
    if text.ends_with('\n') {
        out.push(Line::default());
    }
    if out.is_empty() {
        out.push(Line::default());
    }
    out
}
