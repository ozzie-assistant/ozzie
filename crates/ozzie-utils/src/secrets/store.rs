use std::collections::HashMap;
use std::sync::{OnceLock, RwLock};

/// Global in-memory secret store.
///
/// Secrets (decrypted API keys, tokens, etc.) are stored here instead of
/// in `std::env`, so they are never exposed via `printenv`, `/proc/self/environ`,
/// or any tool that reads the process environment.
///
/// Secrets are only resolved at the moment they are needed (config templating,
/// auth resolution) via `SecretStore::get()`.
static STORE: OnceLock<SecretStore> = OnceLock::new();

pub struct SecretStore {
    secrets: RwLock<HashMap<String, String>>,
}

impl SecretStore {
    fn new() -> Self {
        Self {
            secrets: RwLock::new(HashMap::new()),
        }
    }

    /// Returns the global secret store instance.
    pub fn global() -> &'static SecretStore {
        STORE.get_or_init(SecretStore::new)
    }

    /// Stores a secret. Overwrites if key already exists.
    pub fn set(&self, key: impl Into<String>, value: impl Into<String>) {
        let mut secrets = self.secrets.write().unwrap_or_else(|e| e.into_inner());
        secrets.insert(key.into(), value.into());
    }

    /// Retrieves a secret by key.
    pub fn get(&self, key: &str) -> Option<String> {
        let secrets = self.secrets.read().unwrap_or_else(|e| e.into_inner());
        secrets.get(key).cloned()
    }

    /// Resolves a value that may reference a secret via `${VAR_NAME}` syntax.
    ///
    /// Resolution order:
    /// 1. SecretStore (decrypted secrets from .env)
    /// 2. Process environment (for non-secret vars like OZZIE_PATH, HOME)
    pub fn resolve_value(&self, value: &str) -> String {
        let trimmed = value.trim();
        if trimmed.starts_with("${") && trimmed.ends_with('}') {
            let var_name = &trimmed[2..trimmed.len() - 1];
            if let Some(secret) = self.get(var_name) {
                return secret;
            }
            // Fallback to env for non-secret vars
            return std::env::var(var_name).unwrap_or_default();
        }
        trimmed.to_string()
    }

    /// Resolves a `${{ .Env.VAR }}` template variable.
    ///
    /// Same resolution order as `resolve_value`: SecretStore first, then env.
    pub fn resolve_template_var(&self, var_name: &str) -> String {
        if let Some(secret) = self.get(var_name) {
            return secret;
        }
        std::env::var(var_name).unwrap_or_default()
    }

    /// Returns the number of stored secrets.
    pub fn len(&self) -> usize {
        let secrets = self.secrets.read().unwrap_or_else(|e| e.into_inner());
        secrets.len()
    }

    /// Returns true if the store is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_store() -> SecretStore {
        SecretStore::new()
    }

    #[test]
    fn set_and_get() {
        let store = make_store();
        store.set("ANTHROPIC_API_KEY", "sk-ant-secret");
        assert_eq!(store.get("ANTHROPIC_API_KEY").unwrap(), "sk-ant-secret");
    }

    #[test]
    fn get_missing_returns_none() {
        let store = make_store();
        assert!(store.get("NONEXISTENT").is_none());
    }

    #[test]
    fn resolve_value_from_store() {
        let store = make_store();
        store.set("MY_SECRET", "decrypted_value");
        assert_eq!(store.resolve_value("${MY_SECRET}"), "decrypted_value");
    }

    #[test]
    fn resolve_value_falls_back_to_env() {
        let store = make_store();
        // HOME is always set in env, never in secret store
        let result = store.resolve_value("${HOME}");
        assert!(!result.is_empty());
    }

    #[test]
    fn resolve_value_literal() {
        let store = make_store();
        assert_eq!(store.resolve_value("plain-value"), "plain-value");
    }

    #[test]
    fn resolve_template_var_prefers_store() {
        let store = make_store();
        store.set("DUAL_VAR", "from_store");
        // Even if env has a different value, store wins
        let result = store.resolve_template_var("DUAL_VAR");
        assert_eq!(result, "from_store");
    }

    #[test]
    fn overwrite_existing() {
        let store = make_store();
        store.set("KEY", "v1");
        store.set("KEY", "v2");
        assert_eq!(store.get("KEY").unwrap(), "v2");
    }

    #[test]
    fn len_and_empty() {
        let store = make_store();
        assert!(store.is_empty());
        store.set("K", "V");
        assert_eq!(store.len(), 1);
        assert!(!store.is_empty());
    }
}
