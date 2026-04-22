mod ask;
mod chat;
mod connector;
pub(crate) mod daemon;
mod events;
mod gateway;
mod mcp_serve;
mod memory;
mod pairing;
mod schedule;
mod secret;
mod sessions;
mod status;
mod tui;
mod wake;

use clap::Subcommand;

/// Top-level CLI commands.
#[derive(Subcommand)]
pub enum Command {
    /// Start the gateway server.
    Gateway(gateway::GatewayArgs),
    /// Check gateway health status.
    Status(status::StatusArgs),
    /// Manage sessions.
    Sessions(sessions::SessionsArgs),
    /// Query gateway events.
    Events(events::EventsArgs),
    /// Manage secrets.
    Secret(secret::SecretArgs),
    /// Manage scheduled triggers.
    Schedule(schedule::ScheduleArgs),
    /// Manage semantic memory.
    Memory(memory::MemoryArgs),
    /// Expose tools via MCP protocol on stdio.
    McpServe(mcp_serve::McpServeArgs),
    /// Send a message and stream the response.
    Ask(ask::AskArgs),
    /// Interactive setup wizard.
    Wake(wake::WakeArgs),
    /// Manage device and chat pairings.
    Pairing(pairing::PairingArgs),
    /// Manage connectors (Discord, ...).
    Connector(connector::ConnectorArgs),
    /// Manage the Ozzie daemon (start, stop, status).
    Daemon(daemon::DaemonArgs),
    /// Launch the interactive TUI.
    Tui(tui::TuiArgs),
}

/// Launches the interactive REPL (default when no subcommand is given).
pub async fn chat(config_path: Option<&str>) -> anyhow::Result<()> {
    let _ = config_path; // reserved for future config loading
    chat::run(chat::ChatArgs::default()).await
}

/// Dispatches the command.
pub async fn run(cmd: Command, config_path: Option<&str>) -> anyhow::Result<()> {
    match cmd {
        Command::Gateway(args) => gateway::run(args, config_path).await,
        Command::Status(args) => status::run(args).await,
        Command::Sessions(args) => sessions::run(args).await,
        Command::Events(args) => events::run(args).await,
        Command::Secret(args) => secret::run(args).await,
        Command::Schedule(args) => schedule::run(args).await,
        Command::Memory(args) => memory::run(args).await,
        Command::McpServe(args) => mcp_serve::run(args).await,
        Command::Ask(args) => ask::run(args).await,
        Command::Wake(args) => wake::run(args).await,

        Command::Pairing(args) => pairing::run(args).await,
        Command::Connector(args) => connector::run(args).await,
        Command::Daemon(args) => daemon::run(args).await,
        Command::Tui(args) => tui::run(args).await,
    }
}
