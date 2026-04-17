/// Trait for resolving secrets (API keys, tokens).
///
/// The consumer provides their own implementation — encrypted store,
/// env vars, vault, etc. ozzie-llm never manages secret storage directly.
pub trait SecretResolver: Send + Sync {
    /// Retrieves a secret by key name (e.g. "ANTHROPIC_API_KEY").
    fn get(&self, key: &str) -> Option<String>;

    /// Resolves a value that may reference a secret via `${VAR_NAME}` syntax.
    ///
    /// If the value matches `${...}`, looks up via `get()`, then falls back to env.
    /// Otherwise returns the value as-is.
    fn resolve_value(&self, value: &str) -> String {
        let trimmed = value.trim();
        if trimmed.starts_with("${") && trimmed.ends_with('}') {
            let var_name = &trimmed[2..trimmed.len() - 1];
            if let Some(secret) = self.get(var_name) {
                return secret;
            }
            return std::env::var(var_name).unwrap_or_default();
        }
        trimmed.to_string()
    }
}

/// Fallback resolver that only checks environment variables.
///
/// Used when no secret store is configured.
pub struct EnvSecretResolver;

impl SecretResolver for EnvSecretResolver {
    fn get(&self, key: &str) -> Option<String> {
        std::env::var(key).ok().filter(|v| !v.is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_resolver_gets_from_env() {
        let resolver = EnvSecretResolver;
        // HOME is always set on unix
        let home = resolver.get("HOME");
        assert!(home.is_some() || cfg!(windows));
    }

    #[test]
    fn resolve_value_literal() {
        let resolver = EnvSecretResolver;
        assert_eq!(resolver.resolve_value("plain-value"), "plain-value");
    }

    #[test]
    fn resolve_value_env_ref() {
        let resolver = EnvSecretResolver;
        let result = resolver.resolve_value("${HOME}");
        assert!(!result.is_empty() || cfg!(windows));
    }
}
