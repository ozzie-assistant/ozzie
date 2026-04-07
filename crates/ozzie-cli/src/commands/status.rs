use clap::Args;

/// Arguments for the status command.
#[derive(Args)]
pub struct StatusArgs {
    /// Gateway HTTP base URL.
    #[arg(long, default_value = "http://127.0.0.1:18420")]
    gateway: String,
}

/// Checks gateway health.
pub async fn run(args: StatusArgs) -> anyhow::Result<()> {
    let url = format!("{}/api/health", args.gateway);

    match reqwest::get(&url).await {
        Ok(resp) if resp.status().is_success() => {
            let body: serde_json::Value = resp.json().await?;
            let status = body
                .get("status")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            println!("Gateway: ALIVE (status: {status})");
            Ok(())
        }
        Ok(resp) => {
            println!("Gateway: ERROR (HTTP {})", resp.status());
            Ok(())
        }
        Err(_) => {
            println!("Gateway: NOT RUNNING");
            Ok(())
        }
    }
}
