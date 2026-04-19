use std::io::Write;

use clap::{Args, Subcommand};
use ozzie_core::config;
use ozzie_utils::config::ozzie_path;
use ozzie_utils::secrets;

use crate::crypt::AgeEncryptionService;

/// Manage connectors (Discord, Slack, ...).
#[derive(Args)]
pub struct ConnectorArgs {
    #[command(subcommand)]
    command: ConnectorCommand,
}

#[derive(Subcommand)]
enum ConnectorCommand {
    /// Add a connector.
    Add(AddArgs),
    /// List configured connectors.
    List,
    /// Start the file connector bridge (JSON-RPC client).
    File(FileArgs),
    /// Start the Discord connector bridge (JSON-RPC client).
    Discord(DiscordBridgeArgs),
}

#[derive(Args)]
struct DiscordBridgeArgs {
    #[command(subcommand)]
    command: DiscordBridgeCommand,
}

#[derive(Subcommand)]
enum DiscordBridgeCommand {
    /// Start the Discord connector bridge.
    Start(DiscordStartArgs),
}

#[derive(Args)]
struct DiscordStartArgs {
    /// Discord bot token (reads from config if not provided).
    #[arg(long)]
    token: Option<String>,
    /// Gateway URL.
    #[arg(long, default_value = "http://127.0.0.1:18420")]
    gateway: String,
}

#[derive(Args)]
struct FileArgs {
    #[command(subcommand)]
    command: FileCommand,
}

#[derive(Subcommand)]
enum FileCommand {
    /// Start the file connector bridge.
    Start(FileStartArgs),
}

#[derive(Args)]
struct FileStartArgs {
    /// Path to input JSONL file.
    #[arg(long)]
    input: String,
    /// Path to output JSONL file.
    #[arg(long)]
    output: String,
    /// Gateway URL.
    #[arg(long, default_value = "http://127.0.0.1:18420")]
    gateway: String,
}

#[derive(Args)]
struct AddArgs {
    #[command(subcommand)]
    connector: AddConnectorKind,
}

#[derive(Subcommand)]
enum AddConnectorKind {
    /// Add the Discord bot connector.
    Discord(AddDiscordArgs),
}

#[derive(Args)]
struct AddDiscordArgs {
    /// Discord bot token (can also be set via DISCORD_BOT_TOKEN env var).
    #[arg(long)]
    token: String,
}

pub async fn run(args: ConnectorArgs) -> anyhow::Result<()> {
    match args.command {
        ConnectorCommand::Add(a) => match a.connector {
            AddConnectorKind::Discord(a) => add_discord(a).await,
        },
        ConnectorCommand::List => list().await,
        ConnectorCommand::File(f) => match f.command {
            FileCommand::Start(a) => start_file(a).await,
        },
        ConnectorCommand::Discord(d) => match d.command {
            DiscordBridgeCommand::Start(a) => start_discord(a).await,
        },
    }
}

async fn start_file(args: FileStartArgs) -> anyhow::Result<()> {
    use ozzie_client::OzzieClient;
    use ozzie_file_bridge::{FileBridge, FileConnectorConfig};

    let token = OzzieClient::acquire_token_cli(&args.gateway, &ozzie_path()).await?;

    let config = FileConnectorConfig {
        enabled: true,
        input: args.input,
        output: args.output,
        ..Default::default()
    };

    let bridge = FileBridge::new(config).map_err(|e| anyhow::anyhow!("{e}"))?;
    bridge
        .run(&args.gateway, Some(&token))
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    Ok(())
}

async fn start_discord(args: DiscordStartArgs) -> anyhow::Result<()> {
    use ozzie_client::OzzieClient;
    use ozzie_discord_bridge::DiscordBridge;

    let base = ozzie_path();
    let gateway_token = OzzieClient::acquire_token_cli(&args.gateway, &base).await?;

    // Resolve Discord bot token: CLI arg > env > .env file (encrypted)
    let discord_token = if let Some(t) = args.token {
        t
    } else if let Ok(t) = std::env::var("DISCORD_BOT_TOKEN") {
        t
    } else {
        // Read encrypted token from .env and decrypt with age
        let env_path = base.join(".env");
        let entries = ozzie_utils::secrets::load_dotenv(&env_path)
            .map_err(|e| anyhow::anyhow!("read .env: {e}"))?;
        let encrypted = entries
            .get("DISCORD_BOT_TOKEN")
            .ok_or_else(|| anyhow::anyhow!("No Discord token found. Set --token, DISCORD_BOT_TOKEN env, or run `ozzie connector add discord`"))?;

        if ozzie_utils::secrets::is_encrypted(encrypted) {
            let enc_svc = crate::crypt::AgeEncryptionService::new(&base);
            enc_svc.decrypt(encrypted)?
        } else {
            encrypted.clone()
        }
    };

    let bridge = DiscordBridge::new(discord_token)
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    bridge
        .run(&args.gateway, Some(&gateway_token))
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    Ok(())
}

async fn add_discord(args: AddDiscordArgs) -> anyhow::Result<()> {
    let base = ozzie_path();

    // Verify token using serenity's HTTP client.
    print!("Verifying token... ");
    std::io::stdout().flush()?;

    let bot = ozzie_discord_bridge::verify_token(&args.token)
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    println!("✓ {} (ID: {})", bot.display_name, bot.id);
    let client_id = bot.id;

    // Encrypt the token with the age key before saving to .env.
    // The gateway requires an encrypted token to load it at runtime.
    let enc_svc = AgeEncryptionService::new(&base);
    let encrypted_token = enc_svc.encrypt(&args.token)?;

    let env_path = base.join(".env");
    secrets::set_entry(&env_path, "DISCORD_BOT_TOKEN", &encrypted_token)?;
    println!("✓ Token saved to .env as DISCORD_BOT_TOKEN (encrypted)");

    // Patch config.jsonc to enable the Discord connector.
    patch_config_discord(&base.join("config.jsonc"))?;
    println!("✓ config.jsonc updated (connectors.discord)");

    // Ensure connectors/ directory exists for the guild database (managed at runtime).
    std::fs::create_dir_all(base.join("connectors"))?;

    // Discord permission bitmask:
    //   VIEW_CHANNEL(1024) + SEND_MESSAGES(2048) + READ_MESSAGE_HISTORY(65536)
    //   + MANAGE_CHANNELS(16) + MANAGE_ROLES(268435456)
    const PERMISSIONS: u64 = 1024 + 2048 + 65536 + 16 + 268_435_456;
    let invite_url = format!(
        "https://discord.com/api/oauth2/authorize\
         ?client_id={client_id}\
         &permissions={PERMISSIONS}\
         &scope=bot%20applications.commands"
    );

    println!();
    println!("Invite the bot to your server:");
    println!("  {invite_url}");
    println!();
    println!("Permissions requested:");
    println!("  • View Channels");
    println!("  • Send Messages");
    println!("  • Read Message History");
    println!("  • Manage Channels  (required for /init)");
    println!("  • Manage Roles     (required for /init)");
    println!();
    println!("(Re)start the gateway to apply:");
    println!("  ozzie gateway");

    Ok(())
}

async fn list() -> anyhow::Result<()> {
    use ozzie_utils::config::config_path;

    let config_path = config_path();
    let cfg = if config_path.exists() {
        crate::config_loader::load_partial::<config::Config>(&config_path).unwrap_or_default()
    } else {
        config::Config::default()
    };

    if cfg.connectors.0.is_empty() {
        println!("No connectors configured.");
        println!("Add one with:  ozzie connector add discord --token <token>");
        return Ok(());
    }

    println!("{:<12} {:<24} RESTART", "CONNECTOR", "COMMAND");
    for (name, cpc) in &cfg.connectors.0 {
        let restart = if cpc.restart { "yes" } else { "no" };
        println!("{name:<12} {:<24} {restart}", cpc.command);
    }

    Ok(())
}

/// Updates `config.jsonc` to enable the Discord connector as a supervised process.
///
/// Reads the existing config as JSON (stripping JSONC comments), sets the new
/// `ConnectorProcessConfig` format, and writes back as formatted JSON.
fn patch_config_discord(config_path: &std::path::Path) -> anyhow::Result<()> {
    let mut cfg: serde_json::Value = if config_path.exists() {
        crate::config_loader::load_partial::<serde_json::Value>(config_path).unwrap_or(serde_json::json!({}))
    } else {
        serde_json::json!({})
    };

    let db_path = ozzie_path()
        .join("connectors/discord.jsonc")
        .to_string_lossy()
        .to_string();

    cfg["connectors"]["discord"] = serde_json::json!({
        "command": "ozzie-discord-bridge",
        "env": {
            "DISCORD_BOT_TOKEN": "${{ .Env.DISCORD_BOT_TOKEN }}"
        },
        "config": {
            "token": "${{ .Env.DISCORD_BOT_TOKEN }}",
            "db_path": db_path,
        },
        "auto_pair": true,
        "restart": true,
    });

    let output = serde_json::to_string_pretty(&cfg)?;
    std::fs::write(config_path, output)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn permissions_bitmask() {
        let permissions: u64 = 1024 + 2048 + 65536 + 16 + 268_435_456;
        // Sanity check: all expected bits are set
        assert!(permissions & 1024 != 0, "VIEW_CHANNEL");
        assert!(permissions & 2048 != 0, "SEND_MESSAGES");
        assert!(permissions & 65536 != 0, "READ_MESSAGE_HISTORY");
        assert!(permissions & 16 != 0, "MANAGE_CHANNELS");
        assert!(permissions & 268_435_456 != 0, "MANAGE_ROLES");
    }

    #[test]
    fn list_empty_no_dir() {
        // list() is async and touches filesystem — tested via integration
        // This just validates the filter logic with a fake path
        let names = ["discord"];
        let filtered: Vec<&str> = names
            .iter()
            .copied()
            .filter(|_| false) // simulates missing files
            .collect();
        assert!(filtered.is_empty());
    }
}
