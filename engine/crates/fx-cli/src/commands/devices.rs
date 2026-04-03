use super::api_client::{
    api_error_message, bearer_token, current_unix_seconds, http_client, request_error,
};
use super::runtime_layout::RuntimeLayout;
use anyhow::Context;
use clap::{Args, Subcommand};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Args)]
pub struct DevicesArgs {
    /// Print JSON output for scripting
    #[arg(long, global = true)]
    pub json: bool,

    #[command(subcommand)]
    pub command: Option<DevicesCommand>,
}

#[derive(Debug, Clone, Subcommand)]
pub enum DevicesCommand {
    /// List paired devices
    List,

    /// Revoke a paired device token
    Revoke {
        /// Device identifier to revoke
        device_id: String,
    },
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
struct DeviceInfo {
    id: String,
    device_name: String,
    created_at: u64,
    last_used_at: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
struct DevicesResponse {
    devices: Vec<DeviceInfo>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
struct RevokeDeviceResponse {
    revoked: bool,
    device_id: String,
    #[serde(default)]
    device_name: String,
}

#[derive(Debug, Clone, Copy)]
struct TableWidths {
    id: usize,
    name: usize,
    paired: usize,
    last_used: usize,
}

impl Default for TableWidths {
    fn default() -> Self {
        Self {
            id: 2,
            name: 4,
            paired: 6,
            last_used: 9,
        }
    }
}

pub async fn run(args: &DevicesArgs) -> anyhow::Result<i32> {
    let layout = RuntimeLayout::detect()?;
    let client = http_client()?;
    if let Some(DevicesCommand::Revoke { device_id }) = &args.command {
        return revoke_and_print(&layout, &client, device_id, args.json).await;
    }
    list_and_print(&layout, &client, args.json).await
}

async fn list_and_print(
    layout: &RuntimeLayout,
    client: &reqwest::Client,
    json: bool,
) -> anyhow::Result<i32> {
    let response = fetch_devices(layout, client).await?;
    if json {
        println!("{}", serde_json::to_string_pretty(&response)?);
    } else {
        println!("{}", render_devices_table(&response));
    }
    Ok(0)
}

async fn revoke_and_print(
    layout: &RuntimeLayout,
    client: &reqwest::Client,
    device_id: &str,
    json: bool,
) -> anyhow::Result<i32> {
    let response = revoke_device(layout, client, device_id).await?;
    if json {
        println!("{}", serde_json::to_string_pretty(&response)?);
    } else {
        println!("{}", render_revoke_output(&response));
    }
    Ok(0)
}

async fn fetch_devices(
    layout: &RuntimeLayout,
    client: &reqwest::Client,
) -> anyhow::Result<DevicesResponse> {
    let token = bearer_token(layout)?;
    let response = client
        .get(devices_url(layout.http_port))
        .bearer_auth(token)
        .send()
        .await
        .map_err(request_error)?;
    parse_devices_response(response).await
}

async fn revoke_device(
    layout: &RuntimeLayout,
    client: &reqwest::Client,
    device_id: &str,
) -> anyhow::Result<RevokeDeviceResponse> {
    let token = bearer_token(layout)?;
    let response = client
        .delete(device_url(layout.http_port, device_id))
        .bearer_auth(token)
        .send()
        .await
        .map_err(request_error)?;
    parse_revoke_response(response).await
}

async fn parse_devices_response(response: reqwest::Response) -> anyhow::Result<DevicesResponse> {
    if response.status().is_success() {
        return response
            .json()
            .await
            .context("failed to decode device list response");
    }
    Err(anyhow::anyhow!(api_error_message(response).await))
}

async fn parse_revoke_response(
    response: reqwest::Response,
) -> anyhow::Result<RevokeDeviceResponse> {
    if response.status().is_success() {
        return response
            .json()
            .await
            .context("failed to decode device revoke response");
    }
    Err(anyhow::anyhow!(api_error_message(response).await))
}

fn render_devices_table(response: &DevicesResponse) -> String {
    render_devices_table_at(response, current_unix_seconds())
}

fn render_devices_table_at(response: &DevicesResponse, now: u64) -> String {
    if response.devices.is_empty() {
        return "Paired Devices:\n\n  (no paired devices)\n".to_string();
    }

    let devices = sorted_devices(&response.devices);
    let widths = table_widths(&devices, now);
    let header = render_table_header(&widths);
    let rows = devices
        .iter()
        .map(|device| render_device_row(device, &widths, now))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "Paired Devices:\n\n{header}\n{rows}\n\n{} devices paired.",
        response.devices.len()
    )
}

fn sorted_devices(devices: &[DeviceInfo]) -> Vec<&DeviceInfo> {
    let mut devices = devices.iter().collect::<Vec<_>>();
    devices.sort_by(|left, right| left.device_name.cmp(&right.device_name));
    devices
}

fn table_widths(devices: &[&DeviceInfo], now: u64) -> TableWidths {
    let mut widths = TableWidths::default();
    for device in devices {
        widths.id = widths.id.max(device.id.len());
        widths.name = widths.name.max(device.device_name.len());
        widths.paired = widths
            .paired
            .max(relative_age(now, device.created_at).len());
        widths.last_used = widths
            .last_used
            .max(relative_age(now, device.last_used_at).len());
    }
    widths
}

fn render_table_header(widths: &TableWidths) -> String {
    format!(
        "  {id:<id_width$}  {name:<name_width$}  {paired:<paired_width$}  {last_used}",
        id = "ID",
        name = "Name",
        paired = "Paired",
        last_used = "Last Used",
        id_width = widths.id,
        name_width = widths.name,
        paired_width = widths.paired,
    )
}

fn render_device_row(device: &DeviceInfo, widths: &TableWidths, now: u64) -> String {
    format!(
        "  {id:<id_width$}  {name:<name_width$}  {paired:<paired_width$}  {last_used}",
        id = device.id,
        name = device.device_name,
        paired = relative_age(now, device.created_at),
        last_used = relative_age(now, device.last_used_at),
        id_width = widths.id,
        name_width = widths.name,
        paired_width = widths.paired,
    )
}

fn render_revoke_output(response: &RevokeDeviceResponse) -> String {
    let label = display_device_name(response);
    format!("✓ Device \"{label}\" revoked. Token is no longer valid.")
}

fn display_device_name(response: &RevokeDeviceResponse) -> &str {
    if response.device_name.is_empty() {
        &response.device_id
    } else {
        &response.device_name
    }
}

fn relative_age(now: u64, timestamp: u64) -> String {
    let timestamp = normalize_timestamp(timestamp);
    let age_seconds = now.saturating_sub(timestamp);
    if age_seconds < 60 {
        return format!("{}s ago", age_seconds.max(1));
    }
    if age_seconds < 3_600 {
        return format!("{}m ago", age_seconds / 60);
    }
    if age_seconds < 86_400 {
        return format!("{}h ago", age_seconds / 3_600);
    }
    format!("{}d ago", age_seconds / 86_400)
}

fn normalize_timestamp(timestamp: u64) -> u64 {
    fx_api::time_util::normalize_timestamp(timestamp)
}

fn devices_url(port: u16) -> String {
    format!("http://127.0.0.1:{port}/v1/devices")
}

fn device_url(port: u16, device_id: &str) -> String {
    format!("{}/{}", devices_url(port), device_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[test]
    fn devices_list_json_format() {
        let response = DevicesResponse {
            devices: vec![DeviceInfo {
                id: "dev-a1b2c3".to_string(),
                device_name: "Joe's MacBook".to_string(),
                created_at: 1_773_400_000,
                last_used_at: 1_773_435_000,
            }],
        };

        let json: Value = serde_json::from_str(
            &serde_json::to_string_pretty(&response).expect("serialize device list"),
        )
        .expect("device JSON should parse");

        assert_eq!(json["devices"][0]["id"], "dev-a1b2c3");
        assert_eq!(json["devices"][0]["device_name"], "Joe's MacBook");
        assert_eq!(json["devices"][0]["created_at"], 1_773_400_000);
        assert_eq!(json["devices"][0]["last_used_at"], 1_773_435_000);
    }

    #[test]
    fn render_devices_table_formats_relative_ages() {
        let response = DevicesResponse {
            devices: vec![DeviceInfo {
                id: "dev-a1b2c3".to_string(),
                device_name: "Joe's MacBook".to_string(),
                created_at: 1_700_000_000,
                last_used_at: 1_700_000_300,
            }],
        };

        let rendered = render_devices_table_at(&response, 1_700_000_600);

        assert!(rendered.contains("10m ago"));
        assert!(rendered.contains("5m ago"));
    }
}
