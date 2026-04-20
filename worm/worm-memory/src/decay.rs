use chrono::{DateTime, Utc};
use std::time::Duration;

use crate::entry::ImportanceLevel;

struct DecayConfig {
    grace_period: Duration,
    rate: f64, // per week after grace
    floor: f64,
}

fn decay_config(level: ImportanceLevel) -> DecayConfig {
    match level {
        ImportanceLevel::Core => DecayConfig {
            grace_period: Duration::ZERO,
            rate: 0.0,
            floor: 1.0,
        },
        ImportanceLevel::Important => DecayConfig {
            grace_period: Duration::from_secs(30 * 24 * 3600),
            rate: 0.005,
            floor: 0.3,
        },
        ImportanceLevel::Normal => DecayConfig {
            grace_period: Duration::from_secs(7 * 24 * 3600),
            rate: 0.01,
            floor: 0.1,
        },
        ImportanceLevel::Ephemeral => DecayConfig {
            grace_period: Duration::from_secs(24 * 3600),
            rate: 0.05,
            floor: 0.1,
        },
    }
}

/// Reduces confidence based on time since last use and importance level.
/// Core memories never decay.
pub fn apply_decay(
    confidence: f64,
    last_used_at: DateTime<Utc>,
    now: DateTime<Utc>,
    importance: ImportanceLevel,
) -> f64 {
    if importance == ImportanceLevel::Core {
        return confidence;
    }

    let cfg = decay_config(importance);
    let idle = now.signed_duration_since(last_used_at);
    let idle_secs = idle.num_seconds().max(0) as u64;
    let grace_secs = cfg.grace_period.as_secs();

    if idle_secs <= grace_secs {
        return confidence;
    }

    let weeks_idle = (idle_secs - grace_secs) as f64 / (7.0 * 24.0 * 3600.0);
    let decayed = confidence - cfg.rate * weeks_idle;
    decayed.max(cfg.floor)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeDelta;

    #[test]
    fn core_never_decays() {
        let now = Utc::now();
        let old = now - TimeDelta::days(365);
        assert_eq!(apply_decay(0.9, old, now, ImportanceLevel::Core), 0.9);
    }

    #[test]
    fn normal_within_grace_no_decay() {
        let now = Utc::now();
        let recent = now - TimeDelta::days(3);
        assert_eq!(apply_decay(0.8, recent, now, ImportanceLevel::Normal), 0.8);
    }

    #[test]
    fn normal_after_grace_decays() {
        let now = Utc::now();
        let old = now - TimeDelta::days(14);
        let result = apply_decay(0.8, old, now, ImportanceLevel::Normal);
        assert!(result < 0.8);
        assert!(result > 0.1);
    }

    #[test]
    fn ephemeral_decays_fast() {
        let now = Utc::now();
        let old = now - TimeDelta::days(8);
        let result = apply_decay(0.8, old, now, ImportanceLevel::Ephemeral);
        assert!(result < 0.8);
    }

    #[test]
    fn decay_floors() {
        let now = Utc::now();
        let very_old = now - TimeDelta::days(365);
        let result = apply_decay(0.8, very_old, now, ImportanceLevel::Ephemeral);
        assert!((result - 0.1).abs() < 0.01);
    }
}
