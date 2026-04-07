use std::collections::HashMap;
use std::path::Path;

use super::SecretsError;

/// Prefix for encrypted values in .env files.
const ENC_PREFIX: &str = "ENC[age:";
const ENC_SUFFIX: &str = "]";

/// Checks if a value is encrypted (age format).
pub fn is_encrypted(value: &str) -> bool {
    value.starts_with(ENC_PREFIX) && value.ends_with(ENC_SUFFIX)
}

/// Wraps ciphertext in the `ENC[age:base64]` format.
pub fn wrap_encrypted(base64_ciphertext: &str) -> String {
    format!("{ENC_PREFIX}{base64_ciphertext}{ENC_SUFFIX}")
}

/// Extracts the base64 ciphertext from `ENC[age:base64]`.
pub fn unwrap_encrypted(value: &str) -> Option<&str> {
    value
        .strip_prefix(ENC_PREFIX)
        .and_then(|s| s.strip_suffix(ENC_SUFFIX))
}

/// Reads a .env file and returns key-value pairs.
pub fn load_dotenv(path: &Path) -> Result<HashMap<String, String>, SecretsError> {
    if !path.exists() {
        return Ok(HashMap::new());
    }

    let content = std::fs::read_to_string(path)
        .map_err(|e| SecretsError::Io(format!("read dotenv: {e}")))?;

    let mut entries = HashMap::new();

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            let key = key.trim().to_string();
            let value = unquote(value.trim());
            entries.insert(key, value);
        }
    }

    Ok(entries)
}

/// Sets a key-value entry in a .env file. Creates the file if missing.
pub fn set_entry(path: &Path, key: &str, value: &str) -> Result<(), SecretsError> {
    let content = if path.exists() {
        std::fs::read_to_string(path)
            .map_err(|e| SecretsError::Io(format!("read dotenv: {e}")))?
    } else {
        String::new()
    };

    let quoted = quote_value(value);
    let new_line = format!("{key}={quoted}");
    let mut found = false;
    let mut lines: Vec<String> = content
        .lines()
        .map(|line| {
            let trimmed = line.trim();
            if let Some((k, _)) = trimmed.split_once('=')
                && k.trim() == key
            {
                found = true;
                return new_line.clone();
            }
            line.to_string()
        })
        .collect();

    if !found {
        lines.push(new_line);
    }

    let output = lines.join("\n") + "\n";

    // Atomic write
    let tmp = path.with_extension("env.tmp");
    std::fs::write(&tmp, &output)
        .map_err(|e| SecretsError::Io(format!("write tmp: {e}")))?;
    std::fs::rename(&tmp, path)
        .map_err(|e| SecretsError::Io(format!("rename: {e}")))?;

    Ok(())
}

/// Deletes a key from a .env file.
pub fn delete_entry(path: &Path, key: &str) -> Result<(), SecretsError> {
    if !path.exists() {
        return Ok(());
    }

    let content = std::fs::read_to_string(path)
        .map_err(|e| SecretsError::Io(format!("read dotenv: {e}")))?;

    let lines: Vec<&str> = content
        .lines()
        .filter(|line| {
            let trimmed = line.trim();
            if let Some((k, _)) = trimmed.split_once('=') {
                k.trim() != key
            } else {
                true
            }
        })
        .collect();

    let output = lines.join("\n") + "\n";
    std::fs::write(path, &output)
        .map_err(|e| SecretsError::Io(format!("write dotenv: {e}")))?;

    Ok(())
}

/// Returns all keys in a .env file.
pub fn list_keys(path: &Path) -> Result<Vec<String>, SecretsError> {
    let entries = load_dotenv(path)?;
    let mut keys: Vec<String> = entries.keys().cloned().collect();
    keys.sort();
    Ok(keys)
}

/// Quotes a value if it contains special characters.
fn quote_value(v: &str) -> String {
    if v.contains(' ')
        || v.contains('\t')
        || v.contains('"')
        || v.contains('\\')
        || v.contains('#')
        || v.contains('$')
    {
        let escaped = v.replace('\\', "\\\\").replace('"', "\\\"");
        format!("\"{escaped}\"")
    } else {
        v.to_string()
    }
}

/// Removes surrounding quotes from a value.
fn unquote(v: &str) -> String {
    if (v.starts_with('"') && v.ends_with('"')) || (v.starts_with('\'') && v.ends_with('\'')) {
        let inner = &v[1..v.len() - 1];
        inner.replace("\\\"", "\"").replace("\\\\", "\\")
    } else {
        v.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_encrypted_check() {
        assert!(is_encrypted("ENC[age:abc123]"));
        assert!(!is_encrypted("plaintext"));
        assert!(!is_encrypted("ENC[age:abc123")); // missing suffix
    }

    #[test]
    fn wrap_unwrap_roundtrip() {
        let ct = "base64ciphertext==";
        let wrapped = wrap_encrypted(ct);
        assert_eq!(wrapped, "ENC[age:base64ciphertext==]");
        assert_eq!(unwrap_encrypted(&wrapped), Some(ct));
    }

    #[test]
    fn dotenv_load_and_set() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".env");

        // Set entries
        set_entry(&path, "KEY1", "value1").unwrap();
        set_entry(&path, "KEY2", "value with spaces").unwrap();

        // Load
        let entries = load_dotenv(&path).unwrap();
        assert_eq!(entries.get("KEY1").unwrap(), "value1");
        assert_eq!(entries.get("KEY2").unwrap(), "value with spaces");

        // Update
        set_entry(&path, "KEY1", "updated").unwrap();
        let entries = load_dotenv(&path).unwrap();
        assert_eq!(entries.get("KEY1").unwrap(), "updated");
    }

    #[test]
    fn dotenv_delete() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".env");

        set_entry(&path, "KEY1", "v1").unwrap();
        set_entry(&path, "KEY2", "v2").unwrap();
        delete_entry(&path, "KEY1").unwrap();

        let entries = load_dotenv(&path).unwrap();
        assert!(!entries.contains_key("KEY1"));
        assert!(entries.contains_key("KEY2"));
    }

    #[test]
    fn dotenv_list_keys() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".env");

        set_entry(&path, "B_KEY", "v").unwrap();
        set_entry(&path, "A_KEY", "v").unwrap();

        let keys = list_keys(&path).unwrap();
        assert_eq!(keys, vec!["A_KEY", "B_KEY"]);
    }

    #[test]
    fn quote_special_chars() {
        assert_eq!(quote_value("simple"), "simple");
        assert_eq!(quote_value("has space"), "\"has space\"");
        assert_eq!(quote_value("has\"quote"), "\"has\\\"quote\"");
    }

    #[test]
    fn unquote_values() {
        assert_eq!(unquote("\"quoted\""), "quoted");
        assert_eq!(unquote("'single'"), "single");
        assert_eq!(unquote("plain"), "plain");
    }
}
