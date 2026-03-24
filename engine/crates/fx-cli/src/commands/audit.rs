//! Audit log management commands

use fx_security::{AuditFilter, AuditLog};

const CHECK_MARK: &str = "\x1b[32m✓\x1b[0m"; // Green checkmark
const CROSS_MARK: &str = "\x1b[31m✗\x1b[0m"; // Red X

/// Show recent audit entries
pub async fn show(limit: Option<usize>) -> anyhow::Result<()> {
    let log_path = get_audit_log_path()?;

    if !log_path.exists() {
        println!("No audit log found. The log will be created when events are recorded.");
        return Ok(());
    }

    let log = AuditLog::open(&log_path).await?;

    let filter = AuditFilter {
        limit,
        ..Default::default()
    };

    let events = log.query(&filter)?;

    if events.is_empty() {
        println!("No audit events recorded yet.");
        return Ok(());
    }

    println!("Recent audit events:\n");

    for event in events {
        println!("{}", format_event(&event));
        println!();
    }

    println!("Total events in log: {}", log.count());

    Ok(())
}

/// Verify audit log integrity
pub async fn verify() -> anyhow::Result<i32> {
    let log_path = get_audit_log_path()?;

    if !log_path.exists() {
        println!("No audit log found.");
        return Ok(0);
    }

    println!("Verifying audit log integrity...\n");

    let log = AuditLog::open(&log_path).await?;

    match log.verify_integrity()? {
        true => {
            println!("{} Audit log integrity verified", CHECK_MARK);
            println!("Total events: {}", log.count());
            Ok(0)
        }
        false => {
            println!("{} Audit log integrity check FAILED", CROSS_MARK);
            println!("The log may have been tampered with!");
            Ok(1)
        }
    }
}

fn format_event(event: &fx_security::AuditEvent) -> String {
    let timestamp = format_timestamp(event.timestamp);
    let event_type = format!("{:?}", event.event_type);

    let mut output = String::new();
    output.push_str(&format!("ID: {}\n", event.id));
    output.push_str(&format!("Time: {}\n", timestamp));
    output.push_str(&format!("Type: {}\n", event_type));
    output.push_str(&format!("Actor: {}\n", event.actor));
    output.push_str(&format!("Description: {}", event.description));

    if !event.metadata.is_empty() {
        output.push_str("\nMetadata:");
        for (key, value) in &event.metadata {
            output.push_str(&format!("\n  {}: {}", key, value));
        }
    }

    output
}

fn format_timestamp(millis: u64) -> String {
    let secs = millis / 1000;

    if let Some(datetime) = chrono::DateTime::from_timestamp(secs as i64, 0) {
        datetime.format("%Y-%m-%d %H:%M:%S UTC").to_string()
    } else {
        format!("{} ms", millis)
    }
}

fn get_audit_log_path() -> anyhow::Result<std::path::PathBuf> {
    let home =
        dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Could not determine home directory"))?;
    Ok(home.join(".fawx").join("audit.log"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use fx_security::{AuditEvent, AuditEventType};
    use std::collections::BTreeMap;

    #[test]
    fn test_format_event_produces_readable_output() {
        let event =
            AuditEvent::new(AuditEventType::ActionExecuted, "agent", "Test action").unwrap();

        let formatted = format_event(&event);

        assert!(formatted.contains("ID:"));
        assert!(formatted.contains("Time:"));
        assert!(formatted.contains("Type:"));
        assert!(formatted.contains("Actor:"));
        assert!(formatted.contains("Description:"));
        assert!(formatted.contains("agent"));
        assert!(formatted.contains("Test action"));
    }

    #[test]
    fn test_format_event_with_metadata() {
        let mut metadata = BTreeMap::new();
        metadata.insert("key1".to_string(), "value1".to_string());
        metadata.insert("key2".to_string(), "value2".to_string());

        let event = AuditEvent::with_metadata(
            AuditEventType::ActionExecuted,
            "agent",
            "Test action",
            metadata,
        )
        .unwrap();

        let formatted = format_event(&event);

        assert!(formatted.contains("Metadata:"));
        assert!(formatted.contains("key1"));
        assert!(formatted.contains("value1"));
    }

    #[test]
    fn test_format_timestamp() {
        let millis = 1704067200000u64; // 2024-01-01 00:00:00 UTC
        let formatted = format_timestamp(millis);

        assert!(formatted.contains("2024"));
        assert!(formatted.contains("UTC") || formatted.contains("ms"));
    }
}
