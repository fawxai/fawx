use std::time::{SystemTime, UNIX_EPOCH};

const LEGACY_MILLISECONDS_THRESHOLD: u64 = 1_000_000_000_000;

pub fn current_time_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or_default()
}

/// Normalize a timestamp that may be in milliseconds to seconds.
pub fn normalize_timestamp(timestamp: u64) -> u64 {
    if timestamp >= LEGACY_MILLISECONDS_THRESHOLD {
        timestamp / 1_000
    } else {
        timestamp
    }
}
