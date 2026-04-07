use clap::{Args, Subcommand};
use ozzie_client::OzzieClient;
use ozzie_utils::config::ozzie_path;
use ozzie_core::domain::{DeviceStorage, PairingStorage};
use ozzie_core::policy::{Pairing, PairingKey};
use ozzie_runtime::{JsonDeviceStore, JsonPairingStore};

/// Arguments for the pairing command.
#[derive(Args)]
pub struct PairingArgs {
    #[command(subcommand)]
    command: PairingCommand,

    /// Gateway HTTP base URL.
    #[arg(long, default_value = "http://127.0.0.1:18420", global = true)]
    gateway: String,

    /// Output as JSON.
    #[arg(long, global = true)]
    json: bool,
}

#[derive(Subcommand)]
enum PairingCommand {
    /// Manage pending pairing requests.
    Requests(RequestsArgs),
    /// Manage chat connector pairings.
    Chats(ChatsArgs),
    /// Manage paired devices.
    Devices(DevicesArgs),
}

// ---- Requests subcommand ----

#[derive(Args)]
struct RequestsArgs {
    #[command(subcommand)]
    command: RequestsCommand,
}

#[derive(Subcommand)]
enum RequestsCommand {
    /// List all pending pairing requests.
    List,
    /// Approve a pending pairing request.
    Approve {
        /// Request ID to approve.
        id: String,
        /// Policy name to assign.
        #[arg(long, default_value = "support")]
        policy: String,
    },
    /// Reject a pending pairing request.
    Reject {
        /// Request ID to reject.
        id: String,
    },
}

// ---- Chats subcommand ----

#[derive(Args)]
struct ChatsArgs {
    #[command(subcommand)]
    command: ChatsCommand,
}

#[derive(Subcommand)]
enum ChatsCommand {
    /// List all approved chat pairings.
    List,
    /// Add a chat pairing directly (bypass pairing flow).
    Add {
        #[arg(long)]
        platform: String,
        #[arg(long, default_value = "")]
        server_id: String,
        #[arg(long)]
        user_id: String,
        #[arg(long, default_value = "support")]
        policy: String,
    },
    /// Remove a specific chat pairing.
    Remove {
        #[arg(long)]
        platform: String,
        #[arg(long, default_value = "")]
        server_id: String,
        #[arg(long)]
        user_id: String,
    },
}

pub async fn run(args: PairingArgs) -> anyhow::Result<()> {
    // Acquire token only for commands that hit protected gateway routes.
    let token_for_gateway = match &args.command {
        PairingCommand::Requests(_) => {
            Some(OzzieClient::acquire_token_cli(&args.gateway, &ozzie_path()).await?)
        }
        _ => None,
    };

    match args.command {
        PairingCommand::Requests(r) => {
            let token = token_for_gateway.as_deref().unwrap_or("");
            match r.command {
                RequestsCommand::List => requests_list(&args.gateway, args.json, token).await,
                RequestsCommand::Approve { id, policy } => {
                    requests_approve(&args.gateway, &id, &policy, token).await
                }
                RequestsCommand::Reject { id } => requests_reject(&args.gateway, &id, token).await,
            }
        }
        PairingCommand::Chats(c) => match c.command {
            ChatsCommand::List => chats_list(args.json),
            ChatsCommand::Add {
                platform,
                server_id,
                user_id,
                policy,
            } => chats_add(&platform, &server_id, &user_id, &policy),
            ChatsCommand::Remove {
                platform,
                server_id,
                user_id,
            } => chats_remove(&platform, &server_id, &user_id),
        },
        PairingCommand::Devices(d) => match d.command {
            DevicesCommand::List => devices_list(args.json),
            DevicesCommand::Revoke { device_id } => devices_revoke(&device_id),
        },
    }
}

// ---- Devices subcommand ----

#[derive(Args)]
struct DevicesArgs {
    #[command(subcommand)]
    command: DevicesCommand,
}

#[derive(Subcommand)]
enum DevicesCommand {
    /// List all paired devices.
    List,
    /// Revoke a paired device by ID.
    Revoke {
        /// Device ID to revoke.
        device_id: String,
    },
}

// ---- HTTP helpers ----

async fn requests_list(gateway: &str, json: bool, token: &str) -> anyhow::Result<()> {
    let url = format!("{gateway}/api/pairings/requests");
    let resp = reqwest::Client::new()
        .get(&url)
        .bearer_auth(token)
        .send()
        .await?;
    let body: serde_json::Value = resp.json().await?;

    if json {
        println!("{}", serde_json::to_string_pretty(&body)?);
        return Ok(());
    }

    let requests = body
        .get("requests")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    if requests.is_empty() {
        println!("No pending pairing requests.");
        return Ok(());
    }

    for req in &requests {
        let id = req.get("request_id").and_then(|v| v.as_str()).unwrap_or("-");
        let kind = req.get("kind").and_then(|v| v.as_str()).unwrap_or("-");
        let platform = req
            .get("platform")
            .and_then(|v| v.as_str())
            .unwrap_or("-");
        let display = req
            .get("display_name")
            .and_then(|v| v.as_str())
            .unwrap_or("-");
        let expires = req
            .get("expires_at")
            .and_then(|v| v.as_str())
            .unwrap_or("-");
        println!("{id}  [{kind}]  {platform}  {display}  expires: {expires}");
    }

    Ok(())
}

async fn requests_approve(gateway: &str, id: &str, policy: &str, token: &str) -> anyhow::Result<()> {
    let url = format!("{gateway}/api/pairings/requests/{id}/approve");
    let resp = reqwest::Client::new()
        .post(&url)
        .bearer_auth(token)
        .json(&serde_json::json!({"policy": policy}))
        .send()
        .await?;

    if resp.status().is_success() {
        println!("Request {id} approved with policy '{policy}'.");
    } else {
        let body: serde_json::Value = resp.json().await.unwrap_or_default();
        let err = body
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown error");
        anyhow::bail!("approve failed: {err}");
    }

    Ok(())
}

async fn requests_reject(gateway: &str, id: &str, token: &str) -> anyhow::Result<()> {
    let url = format!("{gateway}/api/pairings/requests/{id}/reject");
    let resp = reqwest::Client::new()
        .post(&url)
        .bearer_auth(token)
        .send()
        .await?;

    if resp.status().is_success() {
        println!("Request {id} rejected.");
    } else {
        let body: serde_json::Value = resp.json().await.unwrap_or_default();
        let err = body
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown error");
        anyhow::bail!("reject failed: {err}");
    }

    Ok(())
}

// ---- Disk helpers (chats) ----

fn open_store() -> JsonPairingStore {
    JsonPairingStore::new(&ozzie_path())
}

fn chats_list(json: bool) -> anyhow::Result<()> {
    let store = open_store();
    let pairings = store.list();

    if json {
        println!("{}", serde_json::to_string_pretty(&pairings)?);
        return Ok(());
    }

    if pairings.is_empty() {
        println!("No chat pairings configured.");
        return Ok(());
    }

    for p in &pairings {
        println!(
            "{}  server={} user={}  policy={}",
            p.key.platform, p.key.server_id, p.key.user_id, p.policy_name
        );
    }

    Ok(())
}

fn chats_add(
    platform: &str,
    server_id: &str,
    user_id: &str,
    policy: &str,
) -> anyhow::Result<()> {
    let store = open_store();
    store.add(&Pairing {
        key: PairingKey {
            platform: platform.to_string(),
            server_id: server_id.to_string(),
            user_id: user_id.to_string(),
        },
        policy_name: policy.to_string(),
    })?;
    println!("Pairing added: {platform}/{server_id}/{user_id} → {policy}");
    Ok(())
}

fn chats_remove(
    platform: &str,
    server_id: &str,
    user_id: &str,
) -> anyhow::Result<()> {
    let store = open_store();
    let key = PairingKey {
        platform: platform.to_string(),
        server_id: server_id.to_string(),
        user_id: user_id.to_string(),
    };
    match store.remove(&key)? {
        true => println!("Pairing removed."),
        false => println!("Pairing not found."),
    }
    Ok(())
}

// ---- Disk helpers (devices) ----

fn open_device_store() -> JsonDeviceStore {
    JsonDeviceStore::new(&ozzie_path())
}

fn devices_list(json: bool) -> anyhow::Result<()> {
    let store = open_device_store();
    let devices = store.list();

    if json {
        println!("{}", serde_json::to_string_pretty(&devices)?);
        return Ok(());
    }

    if devices.is_empty() {
        println!("No paired devices.");
        return Ok(());
    }

    for d in &devices {
        let label = d.label.as_deref().unwrap_or("-");
        let last_seen = d
            .last_seen
            .map(|t| t.to_rfc3339())
            .unwrap_or_else(|| "-".to_string());
        println!(
            "{}  [{}]  label={label}  paired={}  last_seen={last_seen}",
            d.device_id,
            d.client_type,
            d.paired_at.to_rfc3339(),
        );
    }

    Ok(())
}

fn devices_revoke(device_id: &str) -> anyhow::Result<()> {
    let store = open_device_store();
    match store.revoke(device_id)? {
        true => println!("Device {device_id} revoked."),
        false => println!("Device {device_id} not found."),
    }
    Ok(())
}
