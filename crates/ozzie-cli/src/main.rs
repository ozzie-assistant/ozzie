mod commands;
pub(crate) mod config_input;
mod crypt;
mod output;
mod provider_factory;

use clap::Parser;

/// Ozzie — Personal AI agent operating system.
#[derive(Parser)]
#[command(name = "ozzie", version = env!("OZZIE_VERSION"), about)]
struct Cli {
    /// Enable debug logging.
    #[arg(long, global = true)]
    debug: bool,

    /// Path to config file.
    #[arg(short, long, global = true)]
    config: Option<String>,

    #[command(subcommand)]
    command: Option<commands::Command>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // Setup tracing
    let filter = if cli.debug {
        "debug"
    } else {
        "info"
    };

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(filter)),
        )
        .init();

    // Ensure all standard directories exist before running any command.
    ozzie_utils::config::ensure_dirs()?;

    match cli.command {
        Some(cmd) => commands::run(cmd, cli.config.as_deref()).await,
        None => commands::chat(cli.config.as_deref()).await,
    }
}
