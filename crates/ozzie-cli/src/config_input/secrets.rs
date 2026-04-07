use std::path::Path;

/// Encrypts and stores a list of `(env_var, plaintext)` secrets to the `.env` file.
///
/// Reusable by the wizard orchestrator and future `config set` command.
pub fn store_secrets(base: &Path, secrets: &[(String, String)]) -> anyhow::Result<()> {
    if secrets.is_empty() {
        return Ok(());
    }
    let enc_svc = crate::crypt::AgeEncryptionService::new(base);
    if !enc_svc.is_available() {
        enc_svc.generate()?;
    }
    let env_path = base.join(".env");
    for (var, plaintext) in secrets {
        if !plaintext.is_empty() {
            let encrypted = enc_svc.encrypt(plaintext)?;
            ozzie_utils::secrets::set_entry(&env_path, var, &encrypted)?;
        }
    }
    Ok(())
}
