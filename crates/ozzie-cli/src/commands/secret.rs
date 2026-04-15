use std::io::{self, BufRead, Write as IoWrite};

use clap::{Args, Subcommand};
use ozzie_utils::config::{dotenv_path, ozzie_path};
use ozzie_utils::secrets;

use crate::crypt::AgeEncryptionService;
use crate::output;

/// Secret management commands.
#[derive(Args)]
pub struct SecretArgs {
    #[command(subcommand)]
    command: SecretCommand,
}

#[derive(Subcommand)]
enum SecretCommand {
    /// Set a secret value.
    Set {
        /// Secret key name.
        key: String,
    },
    /// List all secret keys.
    List {
        /// Output as JSON.
        #[arg(long)]
        json: bool,
    },
    /// Delete a secret.
    Delete {
        /// Secret key name.
        key: String,
    },
    /// Rotate encryption key and re-encrypt all secrets.
    Rotate,
}

pub async fn run(args: SecretArgs) -> anyhow::Result<()> {
    match args.command {
        SecretCommand::Set { key } => set_secret(&key).await,
        SecretCommand::List { json } => list_secrets(json).await,
        SecretCommand::Delete { key } => delete_secret(&key).await,
        SecretCommand::Rotate => rotate_secrets().await,
    }
}

async fn set_secret(key: &str) -> anyhow::Result<()> {
    let env_path = dotenv_path();

    eprint!("Enter value for {key}: ");
    io::stderr().flush()?;

    let value = read_secret_line()?;
    if value.is_empty() {
        anyhow::bail!("empty value");
    }

    let enc_svc = AgeEncryptionService::new(&ozzie_path());
    if !enc_svc.is_available() {
        anyhow::bail!(
            "no encryption key found. Run `ozzie wake` to generate one."
        );
    }
    let final_value = enc_svc.encrypt(&value)?;

    secrets::set_entry(&env_path, key, &final_value)?;
    println!("Secret {key} saved.");
    Ok(())
}

async fn list_secrets(json: bool) -> anyhow::Result<()> {
    let env_path = dotenv_path();
    let entries = secrets::load_dotenv(&env_path)?;

    if json {
        let items: Vec<serde_json::Value> = entries
            .iter()
            .map(|(k, v)| {
                serde_json::json!({
                    "key": k,
                    "encrypted": secrets::is_encrypted(v),
                })
            })
            .collect();
        return output::print_json(&items);
    }

    if entries.is_empty() {
        println!("No secrets found.");
        return Ok(());
    }

    let mut rows: Vec<Vec<String>> = entries
        .iter()
        .map(|(k, v)| {
            let status = if secrets::is_encrypted(v) {
                "encrypted".to_string()
            } else {
                "plaintext".to_string()
            };
            vec![k.clone(), status]
        })
        .collect();

    rows.sort_by(|a, b| a[0].cmp(&b[0]));
    output::print_table(&["KEY", "STATUS"], rows);
    Ok(())
}

async fn delete_secret(key: &str) -> anyhow::Result<()> {
    let env_path = dotenv_path();
    secrets::delete_entry(&env_path, key)?;
    println!("Secret {key} deleted.");
    Ok(())
}

async fn rotate_secrets() -> anyhow::Result<()> {
    let env_path = dotenv_path();
    let entries = secrets::load_dotenv(&env_path)?;

    let enc_svc = AgeEncryptionService::new(&ozzie_path());
    if !enc_svc.is_available() {
        anyhow::bail!("no age key found — cannot rotate");
    }

    // rotate() saves the new key and returns the old one for re-encryption.
    let old_key = enc_svc.rotate()?;

    let mut count = 0;
    for (k, v) in &entries {
        if secrets::is_encrypted(v) {
            let plaintext = AgeEncryptionService::decrypt_with_key_str(v, &old_key)?;
            let new_encrypted = enc_svc.encrypt(&plaintext)?;
            secrets::set_entry(&env_path, k, &new_encrypted)?;
            count += 1;
        }
    }

    println!("Rotated encryption key. Re-encrypted {count} secrets.");
    Ok(())
}

fn read_secret_line() -> anyhow::Result<String> {
    let stdin = io::stdin();
    let mut line = String::new();
    stdin.lock().read_line(&mut line)?;
    Ok(line.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_secrets_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".env");
        let entries = secrets::load_dotenv(&path).unwrap();
        assert!(entries.is_empty());
    }
}
