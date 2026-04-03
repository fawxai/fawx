use super::{parse_args, to_tool_result, ToolRegistry};
use crate::tool_trait::{Tool, ToolContext};
use async_trait::async_trait;
use fx_core::runtime_info::RuntimeInfo;
use fx_kernel::act::ToolResult;
use fx_kernel::cancellation::CancellationToken;
use fx_llm::{ToolCall, ToolDefinition};
use serde::Deserialize;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

pub(super) fn register_tools(registry: &mut ToolRegistry, context: &Arc<ToolContext>) {
    registry.register(SelfInfoTool::new(context));
    registry.register(CurrentTimeTool::new(context));
}

struct SelfInfoTool {
    context: Arc<ToolContext>,
}

struct CurrentTimeTool {
    context: Arc<ToolContext>,
}

impl SelfInfoTool {
    fn new(context: &Arc<ToolContext>) -> Self {
        Self {
            context: Arc::clone(context),
        }
    }
}

impl CurrentTimeTool {
    fn new(context: &Arc<ToolContext>) -> Self {
        Self {
            context: Arc::clone(context),
        }
    }
}

#[async_trait]
impl Tool for SelfInfoTool {
    fn name(&self) -> &'static str {
        "self_info"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description:
                "Inspect runtime state: active model, loaded skills, configuration, and version"
                    .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "section": {
                        "type": "string",
                        "enum": ["model", "skills", "config", "all"],
                        "description": "Filter to a specific section. Defaults to 'all'."
                    }
                },
                "required": []
            }),
        }
    }

    async fn execute(&self, call: &ToolCall, _cancel: Option<&CancellationToken>) -> ToolResult {
        to_tool_result(
            &call.id,
            self.name(),
            self.context.handle_self_info(&call.arguments),
        )
    }
}

#[async_trait]
impl Tool for CurrentTimeTool {
    fn name(&self) -> &'static str {
        "current_time"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "Get the current date, time, timezone, and Unix epoch timestamp"
                .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {},
                "required": [],
                "x-fawx-direct-utility": {
                    "enabled": true,
                    "profile": "current_time",
                    "trigger_patterns": [
                        "current time",
                        "what time",
                        "what's the time",
                        "whats the time",
                        "time is it"
                    ]
                }
            }),
        }
    }

    async fn execute(&self, call: &ToolCall, _cancel: Option<&CancellationToken>) -> ToolResult {
        to_tool_result(&call.id, self.name(), self.context.handle_current_time())
    }
}

#[derive(Deserialize)]
struct SelfInfoArgs {
    section: Option<String>,
}

impl ToolContext {
    pub(crate) fn handle_current_time(&self) -> Result<String, String> {
        let now = SystemTime::now();
        let duration = now
            .duration_since(UNIX_EPOCH)
            .map_err(|error| format!("system time before Unix epoch: {error}"))?;
        let epoch = duration.as_secs();
        let iso = iso8601_utc_from_epoch(epoch);
        let day_of_week = day_of_week_from_epoch(epoch);
        Ok(format!(
            "iso8601_utc: {iso}\nepoch: {epoch}\nday_of_week: {day_of_week}"
        ))
    }

    pub(crate) fn handle_self_info(&self, args: &serde_json::Value) -> Result<String, String> {
        let parsed: SelfInfoArgs = parse_args(args)?;
        let info_lock = self
            .runtime_info
            .as_ref()
            .ok_or_else(|| "runtime info not configured".to_string())?;
        let info = info_lock
            .read()
            .map_err(|error| format!("failed to read runtime info: {error}"))?;
        let section = parsed.section.as_deref().unwrap_or("all");
        serialize_section(&info, section)
    }
}

fn serialize_section(info: &RuntimeInfo, section: &str) -> Result<String, String> {
    let value = match section {
        "model" => serde_json::json!({
            "model": {
                "active": &info.active_model,
                "provider": &info.provider,
            }
        }),
        "skills" => serde_json::json!({"skills": &info.skills}),
        "config" => serde_json::json!({"config": &info.config_summary}),
        "all" => serde_json::json!({
            "model": {
                "active": &info.active_model,
                "provider": &info.provider,
            },
            "skills": &info.skills,
            "config": &info.config_summary,
            "version": &info.version,
        }),
        other => {
            return Err(format!(
                "unknown section '{other}', valid sections: model, skills, config, all"
            ));
        }
    };
    serde_json::to_string_pretty(&value).map_err(|error| error.to_string())
}

pub(super) fn day_of_week_from_epoch(epoch: u64) -> &'static str {
    let days_since_epoch = (epoch / 86_400) as i64;
    let weekday_index = (days_since_epoch + 4).rem_euclid(7);
    match weekday_index {
        0 => "Sunday",
        1 => "Monday",
        2 => "Tuesday",
        3 => "Wednesday",
        4 => "Thursday",
        5 => "Friday",
        _ => "Saturday",
    }
}

pub(super) fn iso8601_utc_from_epoch(epoch: u64) -> String {
    let days_since_epoch = (epoch / 86_400) as i64;
    let seconds_of_day = epoch % 86_400;
    let (year, month, day) = civil_from_days(days_since_epoch);
    let hour = seconds_of_day / 3_600;
    let minute = (seconds_of_day % 3_600) / 60;
    let second = seconds_of_day % 60;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

fn civil_from_days(days_since_epoch: i64) -> (i64, u32, u32) {
    let z = days_since_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    if month <= 2 {
        year += 1;
    }
    (year, month as u32, day as u32)
}
