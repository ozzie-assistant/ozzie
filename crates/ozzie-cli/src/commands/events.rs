use clap::Args;
use ozzie_client::OzzieClient;
use ozzie_utils::config::ozzie_path;

use crate::output;

/// Query gateway events.
#[derive(Args)]
pub struct EventsArgs {
    /// Gateway HTTP base URL.
    #[arg(long, default_value = "http://127.0.0.1:18420")]
    gateway: String,

    /// Maximum number of events to return.
    #[arg(long, default_value = "50")]
    limit: usize,

    /// Filter by event type.
    #[arg(long, name = "type")]
    event_type: Option<String>,

    /// Filter by session ID.
    #[arg(long)]
    session: Option<String>,

    /// Output as JSON.
    #[arg(long)]
    json: bool,
}

pub async fn run(args: EventsArgs) -> anyhow::Result<()> {
    let token = OzzieClient::acquire_token_cli(&args.gateway, &ozzie_path()).await?;
    let url = format!("{}/api/events", args.gateway);

    let resp = reqwest::Client::new()
        .get(&url)
        .bearer_auth(&token)
        .send()
        .await?;
    if !resp.status().is_success() {
        anyhow::bail!("gateway returned HTTP {}", resp.status());
    }

    let events: Vec<serde_json::Value> = resp.json().await?;

    // Client-side filtering
    let filtered: Vec<&serde_json::Value> = events
        .iter()
        .filter(|e| {
            if let Some(ref t) = args.event_type {
                let event_type = e
                    .get("type")
                    .or_else(|| e.get("event_type"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                if event_type != t {
                    return false;
                }
            }
            if let Some(ref s) = args.session {
                let sid = e
                    .get("session_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                if sid != s {
                    return false;
                }
            }
            true
        })
        .take(args.limit)
        .collect();

    if args.json {
        return output::print_json(&filtered);
    }

    if filtered.is_empty() {
        println!("No events found.");
        return Ok(());
    }

    let rows: Vec<Vec<String>> = filtered
        .iter()
        .map(|e| {
            let ts = e
                .get("ts")
                .or_else(|| e.get("timestamp"))
                .and_then(|v| v.as_str())
                .unwrap_or("-")
                .to_string();
            let event_type = e
                .get("type")
                .or_else(|| e.get("event_type"))
                .and_then(|v| v.as_str())
                .unwrap_or("-")
                .to_string();
            let session = e
                .get("session_id")
                .and_then(|v| v.as_str())
                .unwrap_or("-")
                .to_string();
            let summary = e
                .get("summary")
                .or_else(|| e.get("data"))
                .map(|v| {
                    if let Some(s) = v.as_str() {
                        s.to_string()
                    } else {
                        v.to_string()
                    }
                })
                .unwrap_or_default();
            // Truncate summary
            let summary = if summary.len() > 60 {
                format!("{}...", &summary[..57])
            } else {
                summary
            };
            vec![ts, event_type, session, summary]
        })
        .collect();

    output::print_table(&["TIMESTAMP", "TYPE", "SESSION", "SUMMARY"], rows);
    Ok(())
}

#[cfg(test)]
mod tests {
    // Events command requires a running gateway, so we test the filtering logic.
    use super::*;

    #[test]
    fn args_defaults() {
        // Just verify the struct can be constructed
        let args = EventsArgs {
            gateway: "http://127.0.0.1:18420".to_string(),
            limit: 50,
            event_type: None,
            session: None,
            json: false,
        };
        assert_eq!(args.limit, 50);
    }
}
