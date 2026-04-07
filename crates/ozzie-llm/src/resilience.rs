use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Circuit breaker states.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CircuitState {
    Closed,
    Open,
    HalfOpen,
}

/// Configuration for the circuit breaker.
pub struct CircuitBreakerConfig {
    /// Consecutive failures before opening (default: 5).
    pub threshold: u32,
    /// Time in open state before half-open (default: 30s).
    pub cooldown: Duration,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            threshold: 5,
            cooldown: Duration::from_secs(30),
        }
    }
}

/// Prevents thundering herd against downed providers.
pub struct CircuitBreaker {
    threshold: u32,
    cooldown: Duration,
    failures: AtomicU32,
    state: Mutex<CircuitState>,
    opened_at: Mutex<Option<Instant>>,
}

impl CircuitBreaker {
    pub fn new(cfg: CircuitBreakerConfig) -> Self {
        Self {
            threshold: cfg.threshold,
            cooldown: cfg.cooldown,
            failures: AtomicU32::new(0),
            state: Mutex::new(CircuitState::Closed),
            opened_at: Mutex::new(None),
        }
    }

    /// Returns true if a request is allowed.
    pub fn allow(&self) -> bool {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        match *state {
            CircuitState::Closed => true,
            CircuitState::Open => {
                let opened = self.opened_at.lock().unwrap_or_else(|e| e.into_inner());
                if let Some(opened_at) = *opened
                    && opened_at.elapsed() >= self.cooldown
                {
                    *state = CircuitState::HalfOpen;
                    return true; // allow one probe
                }
                false
            }
            CircuitState::HalfOpen => false, // only one probe at a time
        }
    }

    /// Records a successful request.
    pub fn record_success(&self) {
        self.failures.store(0, Ordering::Relaxed);
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        *state = CircuitState::Closed;
    }

    /// Records a failed request.
    pub fn record_failure(&self) {
        let count = self.failures.fetch_add(1, Ordering::Relaxed) + 1;
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());

        if *state == CircuitState::HalfOpen || count >= self.threshold {
            *state = CircuitState::Open;
            *self.opened_at.lock().unwrap_or_else(|e| e.into_inner()) = Some(Instant::now());
        }
    }

    /// Returns the current state.
    pub fn state(&self) -> CircuitState {
        *self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// Retry configuration with exponential backoff.
#[derive(Debug, Clone)]
pub struct RetryConfig {
    /// Total attempts including first (default: 3).
    pub max_attempts: u32,
    /// Base delay before first retry (default: 1s).
    pub initial_delay: Duration,
    /// Delay cap (default: 30s).
    pub max_delay: Duration,
    /// Backoff multiplier (default: 2.0).
    pub multiplier: f64,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            initial_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(30),
            multiplier: 2.0,
        }
    }
}

impl RetryConfig {
    /// Computes the delay for a given attempt (0-indexed).
    pub fn delay_for_attempt(&self, attempt: u32) -> Duration {
        let base = self.initial_delay.as_secs_f64() * self.multiplier.powi(attempt as i32);
        let capped = base.min(self.max_delay.as_secs_f64());
        // Add 25% jitter
        let jitter = capped * 0.25 * (rand::random::<f64>() - 0.5) * 2.0;
        Duration::from_secs_f64((capped + jitter).max(0.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn circuit_breaker_starts_closed() {
        let cb = CircuitBreaker::new(CircuitBreakerConfig::default());
        assert_eq!(cb.state(), CircuitState::Closed);
        assert!(cb.allow());
    }

    #[test]
    fn circuit_breaker_opens_after_threshold() {
        let cb = CircuitBreaker::new(CircuitBreakerConfig {
            threshold: 3,
            cooldown: Duration::from_secs(30),
        });

        cb.record_failure();
        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Closed);

        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Open);
        assert!(!cb.allow());
    }

    #[test]
    fn circuit_breaker_resets_on_success() {
        let cb = CircuitBreaker::new(CircuitBreakerConfig {
            threshold: 2,
            cooldown: Duration::from_secs(30),
        });

        cb.record_failure();
        cb.record_success();
        assert_eq!(cb.state(), CircuitState::Closed);
        assert!(cb.allow());
    }

    #[test]
    fn circuit_breaker_half_open_after_cooldown() {
        let cb = CircuitBreaker::new(CircuitBreakerConfig {
            threshold: 1,
            cooldown: Duration::from_millis(10),
        });

        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Open);

        std::thread::sleep(Duration::from_millis(20));
        assert!(cb.allow()); // should transition to half-open
        assert_eq!(cb.state(), CircuitState::HalfOpen);
    }

    #[test]
    fn retry_config_delay_increases() {
        let cfg = RetryConfig {
            max_attempts: 5,
            initial_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(30),
            multiplier: 2.0,
        };

        let d0 = cfg.delay_for_attempt(0);
        let d1 = cfg.delay_for_attempt(1);
        let d2 = cfg.delay_for_attempt(2);

        // With jitter, just check reasonable bounds
        assert!(d0.as_secs_f64() > 0.5);
        assert!(d1.as_secs_f64() > 1.0);
        assert!(d2.as_secs_f64() > 2.0);
    }

    #[test]
    fn retry_config_caps_at_max() {
        let cfg = RetryConfig {
            max_attempts: 10,
            initial_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(5),
            multiplier: 10.0,
        };

        let d = cfg.delay_for_attempt(5);
        assert!(d.as_secs_f64() <= 7.0); // 5s + 25% jitter max
    }
}
