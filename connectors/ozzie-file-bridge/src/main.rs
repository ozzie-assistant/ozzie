/// Standalone file connector bridge binary.
///
/// Reads configuration from environment variables:
/// - `OZZIE_CONNECTOR_CONFIG` — JSON with `input` and `output` paths
/// - `OZZIE_GATEWAY_URL` — gateway WebSocket URL (default: ws://127.0.0.1:18420/ws)
/// - `OZZIE_GATEWAY_TOKEN` — gateway auth token (optional)
///
/// Typically launched by the ProcessSupervisor, but can also be run manually.
#[tokio::main]
async fn main() {
    let bridge = match ozzie_file_bridge::FileBridge::from_env() {
        Ok(b) => b,
        Err(e) => {
            eprintln!("ozzie-file-bridge: {e}");
            std::process::exit(1);
        }
    };

    // Empty string triggers env var fallback in run()
    if let Err(e) = bridge.run("", None).await {
        eprintln!("ozzie-file-bridge: {e}");
        std::process::exit(1);
    }
}
