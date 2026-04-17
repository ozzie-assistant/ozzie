use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let bridge = match ozzie_discord_bridge::DiscordBridge::from_env() {
        Ok(b) => b,
        Err(e) => {
            eprintln!("ozzie-discord-bridge: {e}");
            std::process::exit(1);
        }
    };

    if let Err(e) = bridge.run("", None).await {
        eprintln!("ozzie-discord-bridge: {e}");
        std::process::exit(1);
    }
}
