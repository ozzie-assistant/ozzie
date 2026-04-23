//! Persists bus events to JSONL files organized by session.
//!
//! Writes to `{dir}/{conversation_id}.jsonl`, one line per event.
//! Events without a session ID go to `_global.jsonl`.
//! Streaming deltas (AssistantStream) are filtered out to avoid noise.

use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;

use ozzie_core::events::{Event, EventBus, EventPayload};
use tracing::warn;

/// Subscribes to all bus events and persists them as JSONL.
pub struct EventLogger {
    _handle: tokio::task::JoinHandle<()>,
}

impl EventLogger {
    /// Creates and starts an EventLogger that writes events to `dir`.
    pub fn start(dir: PathBuf, bus: Arc<dyn EventBus>) -> Self {
        let mut rx = bus.subscribe(&[]); // all events

        let handle = tokio::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(event) => {
                        if matches!(event.payload, EventPayload::AssistantStream { .. }) {
                            continue;
                        }
                        if let Err(e) = write_event(&dir, &event) {
                            warn!(error = %e, "event logger write failed");
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        warn!(skipped = n, "event logger lagged, some events lost");
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        break;
                    }
                }
            }
        });

        Self { _handle: handle }
    }
}

fn write_event(dir: &std::path::Path, event: &Event) -> std::io::Result<()> {
    let data = serde_json::to_string(event)?;

    let filename = match &event.conversation_id {
        Some(sid) if !sid.is_empty() => format!("{sid}.jsonl"),
        _ => "_global.jsonl".to_string(),
    };

    let path = dir.join(filename);

    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    writeln!(file, "{data}")?;
    Ok(())
}
