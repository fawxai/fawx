use crate::{CronError, CronJob, Schedule};
use chrono::{TimeZone, Utc};
use cron::Schedule as CronSchedule;
use std::str::FromStr;

const MIN_EVERY_MS: u64 = 60_000;
const TZ_UNSUPPORTED: &str = "timezone support not yet implemented";

pub fn next_run_time(schedule: &Schedule, now_ms: u64) -> Option<u64> {
    match schedule {
        Schedule::At { at_ms } => Some(*at_ms),
        Schedule::Every {
            every_ms,
            anchor_ms,
        } => next_every_run(*every_ms, *anchor_ms, now_ms),
        Schedule::Cron { expr, tz } => next_cron_run(expr, tz.as_deref(), now_ms),
    }
}

pub fn validate_schedule(schedule: &Schedule) -> Result<(), CronError> {
    match schedule {
        Schedule::At { .. } => Ok(()),
        Schedule::Every { every_ms, .. } => validate_every(*every_ms),
        Schedule::Cron { expr, tz } => {
            validate_timezone(tz.as_deref())?;
            CronSchedule::from_str(expr)
                .map(|_| ())
                .map_err(|error| CronError::InvalidCron(error.to_string()))
        }
    }
}

pub fn is_due(job: &CronJob, now_ms: u64) -> bool {
    job.enabled
        && job
            .next_run_at
            .is_some_and(|next_run_at| next_run_at <= now_ms)
}

fn next_every_run(every_ms: u64, anchor_ms: Option<u64>, now_ms: u64) -> Option<u64> {
    if validate_every(every_ms).is_err() {
        return None;
    }
    let anchor = anchor_ms.unwrap_or(now_ms);
    if now_ms <= anchor {
        return Some(anchor);
    }
    let elapsed = now_ms.saturating_sub(anchor);
    let steps = elapsed / every_ms;
    Some(anchor.saturating_add(steps.saturating_add(1).saturating_mul(every_ms)))
}

fn validate_every(every_ms: u64) -> Result<(), CronError> {
    if every_ms < MIN_EVERY_MS {
        return Err(CronError::InvalidSchedule(
            "every_ms must be at least 60000".to_string(),
        ));
    }
    Ok(())
}

fn validate_timezone(tz: Option<&str>) -> Result<(), CronError> {
    if tz.is_some() {
        return Err(CronError::InvalidSchedule(TZ_UNSUPPORTED.to_string()));
    }
    Ok(())
}

fn next_cron_run(expr: &str, tz: Option<&str>, now_ms: u64) -> Option<u64> {
    if validate_timezone(tz).is_err() {
        return None;
    }
    let schedule = CronSchedule::from_str(expr).ok()?;
    let now = millis_to_datetime(now_ms)?;
    schedule.after(&now).next().and_then(datetime_to_millis)
}

fn millis_to_datetime(ms: u64) -> Option<chrono::DateTime<Utc>> {
    let ms_i64 = i64::try_from(ms).ok()?;
    Utc.timestamp_millis_opt(ms_i64).single()
}

fn datetime_to_millis(value: chrono::DateTime<Utc>) -> Option<u64> {
    u64::try_from(value.timestamp_millis()).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{JobPayload, Schedule};
    use uuid::Uuid;

    fn job(next_run_at: Option<u64>, enabled: bool) -> CronJob {
        CronJob {
            id: Uuid::new_v4(),
            name: None,
            schedule: Schedule::At { at_ms: 1_000 },
            payload: JobPayload::AgentTurn {
                message: "hi".to_string(),
            },
            enabled,
            created_at: 0,
            updated_at: 0,
            last_run_at: None,
            next_run_at,
            run_count: 0,
        }
    }

    #[test]
    fn schedule_every_computes_next_run() {
        let next = next_run_time(
            &Schedule::Every {
                every_ms: 60_000,
                anchor_ms: Some(0),
            },
            61_000,
        );
        assert_eq!(next, Some(120_000));
    }

    #[test]
    fn schedule_cron_parses_expression() {
        let next = next_run_time(
            &Schedule::Cron {
                expr: "0 * * * * * *".to_string(),
                tz: None,
            },
            1_710_000_000_000,
        );
        assert!(next.is_some());
    }

    #[test]
    fn is_due_returns_true_when_past_next_run() {
        assert!(is_due(&job(Some(1_000), true), 1_001));
    }

    #[test]
    fn is_due_returns_false_when_before_next_run() {
        assert!(!is_due(&job(Some(1_000), true), 999));
    }

    #[test]
    fn is_due_returns_false_when_disabled() {
        assert!(!is_due(&job(Some(1_000), false), 5_000));
    }

    #[test]
    fn validate_schedule_rejects_short_every_interval() {
        let err = validate_schedule(&Schedule::Every {
            every_ms: 59_999,
            anchor_ms: None,
        })
        .expect_err("short interval");
        assert!(err.to_string().contains("60000"));
    }

    #[test]
    fn validate_schedule_rejects_timezones() {
        let err = validate_schedule(&Schedule::Cron {
            expr: "0 * * * * * *".to_string(),
            tz: Some("UTC".to_string()),
        })
        .expect_err("timezone unsupported");
        assert_eq!(
            err.to_string(),
            format!("invalid schedule: {TZ_UNSUPPORTED}")
        );
    }
}
