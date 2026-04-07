use chrono::{DateTime, Datelike, Timelike, Utc};
use serde::{Deserialize, Serialize};

use ozzie_core::domain::SchedulerError;

/// Cron expression (5-field: minute hour dom month dow).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronExpr {
    pub raw: String,
    pub(super) minutes: Vec<u32>,
    pub(super) hours: Vec<u32>,
    pub(super) doms: Vec<u32>,
    pub(super) months: Vec<u32>,
    pub(super) dows: Vec<u32>,
}

impl CronExpr {
    /// Parses a 5-field cron expression.
    ///
    /// Fields: minute(0-59) hour(0-23) dom(1-31) month(1-12) dow(0-6, 0=Sun)
    pub fn parse(raw: &str) -> Result<Self, SchedulerError> {
        let parts: Vec<&str> = raw.split_whitespace().collect();
        if parts.len() != 5 {
            return Err(SchedulerError::InvalidCron(format!(
                "expected 5 fields, got {}",
                parts.len()
            )));
        }

        Ok(Self {
            raw: raw.to_string(),
            minutes: parse_cron_field(parts[0], 0, 59)?,
            hours: parse_cron_field(parts[1], 0, 23)?,
            doms: parse_cron_field(parts[2], 1, 31)?,
            months: parse_cron_field(parts[3], 1, 12)?,
            dows: parse_cron_field(parts[4], 0, 6)?,
        })
    }

    /// Returns true if the given time matches this cron expression.
    pub fn matches(&self, dt: &DateTime<Utc>) -> bool {
        let minute = dt.minute();
        let hour = dt.hour();
        let dom = dt.day();
        let month = dt.month();
        let dow = dt.weekday().num_days_from_sunday();

        self.minutes.contains(&minute)
            && self.hours.contains(&hour)
            && self.doms.contains(&dom)
            && self.months.contains(&month)
            && self.dows.contains(&dow)
    }
}

/// Parses a single cron field into a sorted list of matching values.
pub(super) fn parse_cron_field(field: &str, min: u32, max: u32) -> Result<Vec<u32>, SchedulerError> {
    let mut values = Vec::new();

    for part in field.split(',') {
        if part == "*" {
            return Ok((min..=max).collect());
        }

        // */step
        if let Some(step_str) = part.strip_prefix("*/") {
            let step: u32 = step_str
                .parse()
                .map_err(|_| SchedulerError::InvalidCron(format!("bad step: {part}")))?;
            if step == 0 {
                return Err(SchedulerError::InvalidCron("step cannot be 0".into()));
            }
            let mut v = min;
            while v <= max {
                values.push(v);
                v += step;
            }
            continue;
        }

        // range: a-b
        if part.contains('-') {
            let bounds: Vec<&str> = part.split('-').collect();
            if bounds.len() != 2 {
                return Err(SchedulerError::InvalidCron(format!("bad range: {part}")));
            }
            let lo: u32 = bounds[0]
                .parse()
                .map_err(|_| SchedulerError::InvalidCron(format!("bad range start: {part}")))?;
            let hi: u32 = bounds[1]
                .parse()
                .map_err(|_| SchedulerError::InvalidCron(format!("bad range end: {part}")))?;
            if lo > hi || lo < min || hi > max {
                return Err(SchedulerError::InvalidCron(format!(
                    "range out of bounds: {part}"
                )));
            }
            values.extend(lo..=hi);
            continue;
        }

        // single value
        let v: u32 = part
            .parse()
            .map_err(|_| SchedulerError::InvalidCron(format!("bad value: {part}")))?;
        if v < min || v > max {
            return Err(SchedulerError::InvalidCron(format!(
                "value {v} out of range {min}-{max}"
            )));
        }
        values.push(v);
    }

    values.sort();
    values.dedup();
    Ok(values)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn cron_parse_every_5_min() {
        let expr = CronExpr::parse("*/5 * * * *").unwrap();
        assert!(expr.minutes.contains(&0));
        assert!(expr.minutes.contains(&5));
        assert!(expr.minutes.contains(&55));
        assert!(!expr.minutes.contains(&3));
    }

    #[test]
    fn cron_parse_specific() {
        let expr = CronExpr::parse("30 9 * * 1-5").unwrap();
        assert_eq!(expr.minutes, vec![30]);
        assert_eq!(expr.hours, vec![9]);
        assert_eq!(expr.dows, vec![1, 2, 3, 4, 5]);
    }

    #[test]
    fn cron_matches() {
        let expr = CronExpr::parse("0 12 * * *").unwrap();
        // 2024-01-15 12:00:00 UTC (Monday)
        let dt = Utc.with_ymd_and_hms(2024, 1, 15, 12, 0, 0).unwrap();
        assert!(expr.matches(&dt));

        let dt2 = Utc.with_ymd_and_hms(2024, 1, 15, 13, 0, 0).unwrap();
        assert!(!expr.matches(&dt2));
    }

    #[test]
    fn cron_invalid() {
        assert!(CronExpr::parse("bad").is_err());
        assert!(CronExpr::parse("*/0 * * * *").is_err());
        assert!(CronExpr::parse("60 * * * *").is_err());
    }
}
