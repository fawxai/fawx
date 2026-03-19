use crate::current_epoch_secs;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use thiserror::Error;

const RECENT_SNAPSHOT_WINDOW_SECS: u64 = 24 * 60 * 60;

pub trait RollbackTrigger: Send + Sync {
    fn trigger_rollback(&self, reason: &RollbackReason) -> Result<(), RollbackError>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RollbackReason {
    pub verdict_message: String,
    pub current_success_rate: f64,
    pub baseline_success_rate: f64,
    pub timestamp_epoch_secs: u64,
}

#[derive(Debug, Error)]
pub enum RollbackError {
    #[error("failed to access rollback state: {0}")]
    Io(#[from] std::io::Error),
    #[error("failed to encode rollback reason: {0}")]
    Serialize(#[from] serde_json::Error),
    #[error("ripcord command failed ({command}, status {status}): {stderr}")]
    CommandFailed {
        command: String,
        status: i32,
        stderr: String,
    },
}

pub struct RipcordTrigger {
    ripcord_path: PathBuf,
    data_dir: PathBuf,
}

impl RipcordTrigger {
    pub fn new(ripcord_path: PathBuf, data_dir: PathBuf) -> Self {
        Self {
            ripcord_path,
            data_dir,
        }
    }

    fn write_reason_file(&self, reason: &RollbackReason) -> Result<(), RollbackError> {
        let path = self.data_dir.join("rollback-reason.json");
        let payload = serde_json::to_string_pretty(reason)?;
        fs::create_dir_all(&self.data_dir)?;
        fs::write(path, payload)?;
        Ok(())
    }

    fn ensure_recent_snapshot(&self) -> Result<(), RollbackError> {
        if self.has_recent_snapshot()? {
            return Ok(());
        }
        self.run_command(&["--create"])
    }

    fn has_recent_snapshot(&self) -> Result<bool, RollbackError> {
        let cutoff = current_epoch_secs().saturating_sub(RECENT_SNAPSHOT_WINDOW_SECS);
        let snapshots_dir = self.data_dir.join("snapshots");
        let entries = match fs::read_dir(&snapshots_dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(error.into()),
        };

        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    tracing::debug!(
                        error = %error,
                        dir = %snapshots_dir.display(),
                        "skipping unreadable snapshot entry"
                    );
                    continue;
                }
            };
            if snapshot_is_recent(&entry.path(), cutoff)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn run_command(&self, args: &[&str]) -> Result<(), RollbackError> {
        let output = Command::new(&self.ripcord_path)
            .args(args)
            .env("FAWX_DATA_DIR", &self.data_dir)
            .output()?;
        if output.status.success() {
            return Ok(());
        }

        Err(RollbackError::CommandFailed {
            command: format_command(&self.ripcord_path, args),
            status: output.status.code().unwrap_or(-1),
            stderr: command_stderr(&output),
        })
    }
}

impl RollbackTrigger for RipcordTrigger {
    fn trigger_rollback(&self, reason: &RollbackReason) -> Result<(), RollbackError> {
        self.write_reason_file(reason)?;
        self.ensure_recent_snapshot()?;
        // On a real restore, ripcord terminates the current fawx process before
        // control can return here. The restore continues in the child process,
        // so callers should treat this as a terminal handoff.
        self.run_command(&["restore", "--yes"])
    }
}

#[derive(Deserialize)]
struct SnapshotManifest {
    created_at: String,
}

fn snapshot_is_recent(path: &Path, cutoff: u64) -> Result<bool, RollbackError> {
    let manifest_path = path.join("manifest.json");
    let data = match fs::read_to_string(&manifest_path) {
        Ok(data) => data,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error.into()),
    };
    let manifest: SnapshotManifest = match serde_json::from_str(&data) {
        Ok(manifest) => manifest,
        Err(error) => {
            tracing::warn!(
                error = %error,
                path = %manifest_path.display(),
                "failed to parse snapshot manifest"
            );
            return Ok(false);
        }
    };
    Ok(parse_timestamp_epoch_secs(&manifest.created_at).is_some_and(|ts| ts >= cutoff))
}

fn command_stderr(output: &std::process::Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if stderr.is_empty() {
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    } else {
        stderr
    }
}

fn format_command(path: &Path, args: &[&str]) -> String {
    std::iter::once(path.display().to_string())
        .chain(args.iter().map(|arg| (*arg).to_string()))
        .collect::<Vec<_>>()
        .join(" ")
}

fn parse_timestamp_epoch_secs(input: &str) -> Option<u64> {
    let (date, time) = input.split_once('T')?;
    let (year, month, day) = parse_date(date)?;
    let (hour, minute, second) = parse_time(time.trim_end_matches('Z'))?;
    let days = days_since_epoch(year, month, day)?;
    Some(days * 86_400 + hour as u64 * 3_600 + minute as u64 * 60 + second as u64)
}

fn parse_date(date: &str) -> Option<(i32, u32, u32)> {
    let mut parts = date.split('-');
    let year = parts.next()?.parse().ok()?;
    let month = parts.next()?.parse().ok()?;
    let day = parts.next()?.parse().ok()?;
    Some((year, month, day))
}

fn parse_time(time: &str) -> Option<(u32, u32, u32)> {
    let separator = if time.contains(':') { ':' } else { '-' };
    let mut parts = time.split(separator);
    let hour = parts.next()?.parse().ok()?;
    let minute = parts.next()?.parse().ok()?;
    let second = parts.next()?.parse().ok()?;
    Some((hour, minute, second))
}

fn days_since_epoch(year: i32, month: u32, day: u32) -> Option<u64> {
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }

    let year = year - i32::from(month <= 2);
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let month_prime = month as i32 + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * month_prime + 2) / 5 + day as i32 - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    let days = era * 146_097 + day_of_era - 719_468;
    u64::try_from(days).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;
    use std::sync::{Arc, Mutex};
    use tempfile::TempDir;
    use tracing::Level;
    use tracing_subscriber::fmt::writer::MakeWriter;

    #[derive(Clone)]
    struct SharedWriter(Arc<Mutex<Vec<u8>>>);

    impl Write for SharedWriter {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0.lock().expect("capture logs").extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[derive(Clone)]
    struct SharedMakeWriter(Arc<Mutex<Vec<u8>>>);

    impl<'a> MakeWriter<'a> for SharedMakeWriter {
        type Writer = SharedWriter;

        fn make_writer(&'a self) -> Self::Writer {
            SharedWriter(Arc::clone(&self.0))
        }
    }

    fn capture_warn_logs<T>(action: impl FnOnce() -> T) -> (T, String) {
        let buffer = Arc::new(Mutex::new(Vec::new()));
        let subscriber = tracing_subscriber::fmt()
            .with_max_level(Level::WARN)
            .with_ansi(false)
            .without_time()
            .with_writer(SharedMakeWriter(Arc::clone(&buffer)))
            .finish();
        let _guard = tracing::subscriber::set_default(subscriber);
        let result = action();
        let logs = String::from_utf8(buffer.lock().expect("capture logs").clone())
            .expect("captured logs should be utf8");
        (result, logs)
    }

    fn reason() -> RollbackReason {
        RollbackReason {
            verdict_message: "degraded".to_string(),
            current_success_rate: 0.25,
            baseline_success_rate: 0.90,
            timestamp_epoch_secs: current_epoch_secs(),
        }
    }

    fn trigger_in(temp_dir: &TempDir, script_body: &str) -> RipcordTrigger {
        let bin_path = temp_dir.path().join("fawx-ripcord");
        fs::write(&bin_path, script_body).expect("write script");
        let mut permissions = fs::metadata(&bin_path).expect("metadata").permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&bin_path, permissions).expect("chmod");
        RipcordTrigger::new(bin_path, temp_dir.path().join("data"))
    }

    fn create_manifest(data_dir: &Path, created_at: &str) {
        let snapshot_dir = data_dir.join("snapshots").join("snapshot-a");
        fs::create_dir_all(&snapshot_dir).expect("create snapshot dir");
        fs::write(
            snapshot_dir.join("manifest.json"),
            format!("{{\"created_at\":\"{created_at}\"}}"),
        )
        .expect("write manifest");
    }

    fn recent_timestamp() -> String {
        "9999-12-31T23-59-59".to_string()
    }

    #[test]
    fn parses_dash_and_colon_timestamps() {
        assert!(parse_timestamp_epoch_secs("2026-03-08T22-00-00").is_some());
        assert!(parse_timestamp_epoch_secs("2026-03-08T22:00:00Z").is_some());
    }

    #[test]
    fn invalid_manifest_logs_warning_and_is_not_recent() {
        let temp_dir = TempDir::new().expect("temp dir");
        let snapshot_dir = temp_dir.path().join("snapshot-a");
        fs::create_dir_all(&snapshot_dir).expect("create snapshot dir");
        fs::write(snapshot_dir.join("manifest.json"), "{").expect("write manifest");

        let (is_recent, logs) =
            capture_warn_logs(|| snapshot_is_recent(&snapshot_dir, 0).expect("check recent"));

        assert!(!is_recent);
        assert!(logs.contains("failed to parse snapshot manifest"));
    }

    #[test]
    fn writes_reason_and_restores_when_snapshot_is_recent() {
        let temp_dir = TempDir::new().expect("temp dir");
        let trigger = trigger_in(
            &temp_dir,
            "#!/bin/sh\necho \"$@\" >> \"$FAWX_DATA_DIR/commands.log\"\nexit 0\n",
        );
        create_manifest(&trigger.data_dir, &recent_timestamp());

        trigger
            .trigger_rollback(&reason())
            .expect("trigger rollback");

        let reason_path = trigger.data_dir.join("rollback-reason.json");
        let commands = fs::read_to_string(trigger.data_dir.join("commands.log")).expect("commands");
        assert!(reason_path.exists());
        assert_eq!(commands.trim(), "restore --yes");
    }

    #[test]
    fn creates_snapshot_before_restore_when_none_are_recent() {
        let temp_dir = TempDir::new().expect("temp dir");
        let trigger = trigger_in(
            &temp_dir,
            "#!/bin/sh\necho \"$@\" >> \"$FAWX_DATA_DIR/commands.log\"\nexit 0\n",
        );

        trigger
            .trigger_rollback(&reason())
            .expect("trigger rollback");

        let commands = fs::read_to_string(trigger.data_dir.join("commands.log")).expect("commands");
        assert_eq!(
            commands.lines().collect::<Vec<_>>(),
            vec!["--create", "restore --yes"]
        );
    }

    #[test]
    fn returns_error_when_ripcord_command_fails() {
        let temp_dir = TempDir::new().expect("temp dir");
        let trigger = trigger_in(&temp_dir, "#!/bin/sh\necho fail >&2\nexit 7\n");

        let error = trigger
            .trigger_rollback(&reason())
            .expect_err("command should fail");
        let message = error.to_string();
        assert!(message.contains("status 7"));
        assert!(message.contains("fail"));
    }
}
