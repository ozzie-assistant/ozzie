//! Age-based encryption service for Ozzie secrets.
//!
//! All secrets (API keys, bot tokens, ...) are encrypted at rest using
//! [age](https://age-encryption.org/) x25519. The key lives at
//! `$OZZIE_PATH/.age/current.key` and is generated once by `ozzie wake`.
//!
//! Encrypted values are stored in `.env` as `ENC[age:<base64>]` (binary format).
//! Legacy armored format (`\\n`-escaped ASCII armor) is also supported for decryption.

use std::io;
use std::path::{Path, PathBuf};

use age::secrecy::ExposeSecret;
use base64::Engine;

use ozzie_utils::secrets;

/// Age-based encryption service.
///
/// All operations read the key lazily from disk — no state held in memory.
pub struct AgeEncryptionService {
    key_path: PathBuf,
}

impl AgeEncryptionService {
    /// Creates a service pointing to `ozzie_path/.age/current.key`.
    pub fn new(ozzie_path: &Path) -> Self {
        Self {
            key_path: ozzie_path.join(".age").join("current.key"),
        }
    }

    /// Returns true if the key file exists and contains a valid AGE-SECRET-KEY.
    pub fn is_available(&self) -> bool {
        self.load_key().is_ok()
    }

    /// Generates a new key and writes it to `.age/current.key`.
    ///
    /// No-op if the key already exists. Creates the `.age/` directory if needed.
    pub fn generate(&self) -> anyhow::Result<()> {
        if self.key_path.exists() {
            return Ok(());
        }
        if let Some(parent) = self.key_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let identity = age::x25519::Identity::generate();
        let key_str = identity.to_string().expose_secret().to_string();
        write_key_file(&self.key_path, &key_str)?;
        Ok(())
    }

    /// Encrypts `plaintext` and returns `ENC[age:<base64>]`.
    ///
    /// Uses raw binary age output encoded as standard base64 — compact and
    /// suitable for single-line `.env` storage.
    pub fn encrypt(&self, plaintext: &str) -> anyhow::Result<String> {
        let key_str = self.load_key()?;
        let identity: age::x25519::Identity = key_str
            .trim()
            .parse()
            .map_err(|e| anyhow::anyhow!("invalid age key: {e}"))?;
        let recipient = identity.to_public();

        let recipients: Vec<Box<dyn age::Recipient + Send>> = vec![Box::new(recipient)];
        let encryptor =
            age::Encryptor::with_recipients(recipients.iter().map(|r| r.as_ref() as &dyn age::Recipient))
                .map_err(|_| anyhow::anyhow!("age: no recipients"))?;

        let mut output = Vec::new();
        let mut writer = encryptor
            .wrap_output(&mut output)
            .map_err(|e| anyhow::anyhow!("age encrypt: {e}"))?;

        io::Write::write_all(&mut writer, plaintext.as_bytes())?;
        writer.finish()?;

        let encoded = base64::engine::general_purpose::STANDARD.encode(&output);
        Ok(secrets::wrap_encrypted(&encoded))
    }

    /// Decrypts an `ENC[age:...]` value and returns the plaintext.
    pub fn decrypt(&self, ciphertext: &str) -> anyhow::Result<String> {
        let key_str = self.load_key()?;
        Self::decrypt_with_key_str(ciphertext, &key_str)
    }

    /// Decrypts an `ENC[age:...]` value using the provided key string directly.
    ///
    /// Useful when the key is no longer on disk (e.g. after a rotation).
    ///
    /// Supports two storage formats:
    /// - Binary: raw age binary encoded as base64 (current format)
    /// - Armored: ASCII armor with `\n` replaced by `\\n` (legacy format)
    pub fn decrypt_with_key_str(ciphertext: &str, key_str: &str) -> anyhow::Result<String> {
        let inner = secrets::unwrap_encrypted(ciphertext)
            .ok_or_else(|| anyhow::anyhow!("not an encrypted value: {ciphertext}"))?;

        let identity: age::x25519::Identity = key_str
            .trim()
            .parse()
            .map_err(|e| anyhow::anyhow!("invalid age key: {e}"))?;

        if inner.contains("\\n") {
            // Legacy format: ASCII armored with escaped newlines.
            let armored = inner.replace("\\n", "\n");
            let reader = age::armor::ArmoredReader::new(armored.as_bytes());
            let decryptor =
                age::Decryptor::new(reader).map_err(|e| anyhow::anyhow!("age decryptor: {e}"))?;
            finish_decrypt(decryptor, &identity)
        } else {
            // Current format: raw age binary encoded as base64.
            let raw = base64::engine::general_purpose::STANDARD
                .decode(inner)
                .map_err(|e| anyhow::anyhow!("base64 decode: {e}"))?;
            let decryptor = age::Decryptor::new(raw.as_slice())
                .map_err(|e| anyhow::anyhow!("age decryptor: {e}"))?;
            finish_decrypt(decryptor, &identity)
        }
    }

    /// Generates a brand-new key, saves it, and returns the old key string
    /// so the caller can re-encrypt existing secrets.
    pub fn rotate(&self) -> anyhow::Result<String> {
        let old_key = self.load_key()?;
        let identity = age::x25519::Identity::generate();
        let new_key = identity.to_string().expose_secret().to_string();
        write_key_file(&self.key_path, &new_key)?;
        Ok(old_key)
    }

    /// Reads and validates the key from disk.
    ///
    /// Supports both bare key files and the age-keygen format where comment
    /// lines (starting with `#`) precede the actual key.
    fn load_key(&self) -> anyhow::Result<String> {
        let content = std::fs::read_to_string(&self.key_path).map_err(|_| {
            anyhow::anyhow!(
                "encryption key not found at {}.\nRun `ozzie wake` to generate it.",
                self.key_path.display()
            )
        })?;
        let key = content
            .lines()
            .find(|l| !l.trim_start().starts_with('#') && !l.trim().is_empty())
            .map(|l| l.trim().to_string())
            .ok_or_else(|| anyhow::anyhow!("no key found in {}", self.key_path.display()))?;
        if !key.to_ascii_uppercase().starts_with("AGE-SECRET-KEY-") {
            anyhow::bail!("invalid key format in {}", self.key_path.display());
        }
        Ok(key)
    }
}

/// Writes an age key to disk and restricts permissions to owner-only on Unix.
fn write_key_file(path: &Path, key: &str) -> anyhow::Result<()> {
    std::fs::write(path, key)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

/// Reads all decrypted bytes from a decryptor and returns them as UTF-8.
fn finish_decrypt(
    decryptor: age::Decryptor<impl io::Read>,
    identity: &age::x25519::Identity,
) -> anyhow::Result<String> {
    let mut decrypted = Vec::new();
    let mut dec_reader = decryptor
        .decrypt(std::iter::once(identity as &dyn age::Identity))
        .map_err(|e| anyhow::anyhow!("age decrypt: {e}"))?;
    io::Read::read_to_end(&mut dec_reader, &mut decrypted)?;
    Ok(String::from_utf8(decrypted)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_svc(dir: &Path) -> AgeEncryptionService {
        AgeEncryptionService::new(dir)
    }

    #[test]
    fn generate_creates_key() {
        let dir = tempfile::tempdir().unwrap();
        let svc = make_svc(dir.path());
        assert!(!svc.is_available());
        svc.generate().unwrap();
        assert!(svc.is_available());
        assert!(svc.key_path.exists());
    }

    #[test]
    fn generate_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let svc = make_svc(dir.path());
        svc.generate().unwrap();
        let key1 = std::fs::read_to_string(&svc.key_path).unwrap();
        svc.generate().unwrap(); // second call — must not overwrite
        let key2 = std::fs::read_to_string(&svc.key_path).unwrap();
        assert_eq!(key1, key2);
    }

    #[test]
    fn encrypt_decrypt_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let svc = make_svc(dir.path());
        svc.generate().unwrap();

        let plaintext = "discord_bot_token_abc123";
        let encrypted = svc.encrypt(plaintext).unwrap();

        assert!(secrets::is_encrypted(&encrypted), "must be ENC[age:...]");
        assert!(!encrypted.contains(plaintext), "must not leak plaintext");

        let decrypted = svc.decrypt(&encrypted).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn encrypt_fails_without_key() {
        let dir = tempfile::tempdir().unwrap();
        let svc = make_svc(dir.path());
        assert!(svc.encrypt("secret").is_err());
    }

    #[test]
    fn rotate_changes_key() {
        let dir = tempfile::tempdir().unwrap();
        let svc = make_svc(dir.path());
        svc.generate().unwrap();

        let old_key = svc.rotate().unwrap();
        let new_key = std::fs::read_to_string(&svc.key_path).unwrap();
        assert_ne!(old_key.trim(), new_key.trim());
    }
}
