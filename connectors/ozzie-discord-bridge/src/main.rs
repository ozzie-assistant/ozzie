/// Standalone Discord connector bridge binary.
///
/// Reads configuration from environment variables:
/// - `OZZIE_CONNECTOR_CONFIG` — JSON with `token` and optional `db_path`
/// - `OZZIE_GATEWAY_URL` — gateway WebSocket URL (default: ws://127.0.0.1:18420/ws)
/// - `OZZIE_GATEWAY_TOKEN` — gateway auth token (optional)
/// - `DISCORD_BOT_TOKEN` — Discord bot token (fallback if not in OZZIE_CONNECTOR_CONFIG)
///
/// Typically launched by the ProcessSupervisor, but can also be run manually.
#[tokio::main]
async fn main() {
    let bridge = match ozzie_discord_bridge::DiscordBridge::from_env() {
        Ok(b) => b,
        Err(e) => {
            eprintln!("ozzie-discord-bridge: {e}");
            std::process::exit(1);
        }
    };

    // Empty string triggers env var fallback in run()
    if let Err(e) = bridge.run("", None).await {
        eprintln!("ozzie-discord-bridge: {e}");
        std::process::exit(1);
    }
}
