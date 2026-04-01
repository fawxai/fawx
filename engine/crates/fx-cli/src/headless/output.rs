use super::*;

pub(super) fn json_output_from_cycle(
    result: CycleResult,
    session_messages: &[SessionMessage],
) -> JsonOutput {
    JsonOutput {
        response: result.response,
        model: result.model,
        iterations: result.iterations,
        tool_calls: session_tool_calls(session_messages),
        tool_inputs: session_tool_inputs(session_messages),
        tool_errors: session_tool_errors(session_messages),
    }
}

pub(super) fn write_cycle_output(
    result: CycleResult,
    session_messages: &[SessionMessage],
    json_mode: bool,
) -> Result<(), anyhow::Error> {
    if json_mode {
        return write_json_output(result, session_messages);
    }

    println!("{}", result.response);
    io::stdout().flush()?;
    Ok(())
}

impl HeadlessApp {
    pub(super) fn report_stream_error(event: &StreamEvent) {
        if let StreamEvent::Error {
            category,
            message,
            recoverable,
        } = event
        {
            let level = if *recoverable { "warning" } else { "error" };
            eprintln!("[{level}] [{category}] {message}");
        }
    }

    pub(super) fn print_startup_info(&self) {
        eprintln!("fawx serve — headless mode");
        eprintln!("model: {}", self.active_model);
        if self.custom_system_prompt.is_some() {
            eprintln!("system prompt: custom prompt/context loaded");
        }
        eprintln!("ready (type /quit to exit)");
    }
}

fn write_json_output(
    result: CycleResult,
    session_messages: &[SessionMessage],
) -> Result<(), anyhow::Error> {
    let output = json_output_from_cycle(result, session_messages);
    let json = serde_json::to_string(&output)?;
    println!("{json}");
    io::stdout().flush()?;
    Ok(())
}

fn session_tool_calls(messages: &[SessionMessage]) -> Vec<String> {
    messages
        .iter()
        .flat_map(|message| message.content.iter())
        .filter_map(|block| match block {
            SessionContentBlock::ToolUse { name, .. } => Some(name.clone()),
            _ => None,
        })
        .collect()
}

fn session_tool_inputs(messages: &[SessionMessage]) -> Vec<String> {
    messages
        .iter()
        .flat_map(|message| message.content.iter())
        .filter_map(|block| match block {
            SessionContentBlock::ToolUse { input, .. } => Some(input.to_string()),
            _ => None,
        })
        .collect()
}

fn session_tool_errors(messages: &[SessionMessage]) -> Vec<String> {
    messages
        .iter()
        .flat_map(|message| message.content.iter())
        .filter_map(|block| match block {
            SessionContentBlock::ToolResult {
                content,
                is_error: Some(true),
                ..
            } => Some(session_tool_error_text(content)),
            _ => None,
        })
        .collect()
}

fn session_tool_error_text(content: &serde_json::Value) -> String {
    content
        .as_str()
        .map(ToString::to_string)
        .unwrap_or_else(|| content.to_string())
}
