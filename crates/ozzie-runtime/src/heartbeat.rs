use std::path::{Path, PathBuf};
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

const DEFAULT_INTERVAL: Duration = Duration::from_secs(30);

/// Heartbeat status levels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Alive,
    Stale,
    Dead,
}

/// Persisted heartbeat data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Heartbeat {
    pub pid: u32,
    pub started_at: DateTime<Utc>,
    pub timestamp: DateTime<Utc>,
    pub uptime_secs: u64,
}

/// Writes periodic heartbeat files for liveness detection.
pub struct Writer {
    path: PathBuf,
    interval: Duration,
    started_at: DateTime<Utc>,
    cancel: Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
}

impl Writer {
    /// Creates a new heartbeat writer at the given path.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            interval: DEFAULT_INTERVAL,
            started_at: Utc::now(),
            cancel: Mutex::new(None),
        }
    }

    /// Sets the heartbeat interval.
    pub fn with_interval(mut self, interval: Duration) -> Self {
        self.interval = interval;
        self
    }

    /// Starts the background heartbeat writer.
    pub async fn start(&self) {
        // Write initial heartbeat
        self.write();

        let (tx, mut rx) = tokio::sync::oneshot::channel();
        *self.cancel.lock().await = Some(tx);

        let path = self.path.clone();
        let interval = self.interval;
        let started_at = self.started_at;

        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);
            ticker.tick().await; // skip first immediate tick

            loop {
                tokio::select! {
                    _ = ticker.tick() => {
                        write_heartbeat(&path, started_at);
                    }
                    _ = &mut rx => {
                        // Cleanup on stop
                        let _ = std::fs::remove_file(&path);
                        break;
                    }
                }
            }
        });
    }

    /// Stops the heartbeat writer and removes the file.
    pub async fn stop(&self) {
        if let Some(tx) = self.cancel.lock().await.take() {
            let _ = tx.send(());
        }
    }

    fn write(&self) {
        write_heartbeat(&self.path, self.started_at);
    }
}

fn write_heartbeat(path: &Path, started_at: DateTime<Utc>) {
    let now = Utc::now();
    let uptime = (now - started_at).num_seconds().max(0) as u64;

    let hb = Heartbeat {
        pid: std::process::id(),
        started_at,
        timestamp: now,
        uptime_secs: uptime,
    };

    let Ok(json) = serde_json::to_string_pretty(&hb) else {
        return;
    };

    // Atomic write via temp + rename
    let tmp = path.with_extension("tmp");
    if std::fs::write(&tmp, &json).is_ok() {
        let _ = std::fs::rename(&tmp, path);
    }
}

/// Checks the liveness status of a heartbeat file.
pub fn check(path: &Path, max_age: Duration) -> (Status, Option<Heartbeat>) {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return (Status::Dead, None),
    };

    let hb: Heartbeat = match serde_json::from_str(&content) {
        Ok(h) => h,
        Err(_) => return (Status::Dead, None),
    };

    let age = Utc::now() - hb.timestamp;
    let status = if age.num_milliseconds() < 0 || age.to_std().unwrap_or(Duration::MAX) <= max_age {
        Status::Alive
    } else {
        Status::Stale
    };

    (status, Some(hb))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_and_check_alive() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("heartbeat.json");

        write_heartbeat(&path, Utc::now());

        let (status, hb) = check(&path, Duration::from_secs(60));
        assert_eq!(status, Status::Alive);
        assert!(hb.is_some());
        assert_eq!(hb.unwrap().pid, std::process::id());
    }

    #[test]
    fn check_stale() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("heartbeat.json");

        // Write a heartbeat with an old timestamp
        let old = Utc::now() - chrono::Duration::minutes(5);
        let hb = Heartbeat {
            pid: 1234,
            started_at: old,
            timestamp: old,
            uptime_secs: 0,
        };
        std::fs::write(&path, serde_json::to_string(&hb).unwrap()).unwrap();

        let (status, _) = check(&path, Duration::from_secs(60));
        assert_eq!(status, Status::Stale);
    }

    #[test]
    fn check_dead_missing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nope.json");

        let (status, hb) = check(&path, Duration::from_secs(60));
        assert_eq!(status, Status::Dead);
        assert!(hb.is_none());
    }

    #[test]
    fn check_dead_corrupt() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("heartbeat.json");
        std::fs::write(&path, "not json").unwrap();

        let (status, hb) = check(&path, Duration::from_secs(60));
        assert_eq!(status, Status::Dead);
        assert!(hb.is_none());
    }

    #[tokio::test]
    async fn writer_start_stop() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("heartbeat.json");

        let writer = Writer::new(&path).with_interval(Duration::from_millis(50));
        writer.start().await;

        // File should exist
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert!(path.exists());

        writer.stop().await;
        // Give the spawned task time to clean up
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(!path.exists());
    }
}
