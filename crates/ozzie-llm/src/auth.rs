use ozzie_core::config::Driver;
use ozzie_utils::secrets::SecretStore;
use tracing::warn;

/// Distinguishes between API key and Bearer token auth.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthKind {
    ApiKey,
    BearerToken,
}

/// Resolved credentials with their kind.
#[derive(Debug, Clone)]
pub struct ResolvedAuth {
    pub kind: AuthKind,
    pub value: String,
}

/// Resolves credentials for a provider.
///
/// Resolution order: direct token -> direct api_key -> driver default secret/env.
///
/// Values may use `${VAR_NAME}` syntax, resolved via SecretStore first, then env.
/// Logs a warning if the API key prefix doesn't match the expected driver.
pub fn resolve_auth(
    driver: Driver,
    api_key: Option<&str>,
    token: Option<&str>,
) -> Result<ResolvedAuth, AuthError> {
    let store = SecretStore::global();

    // Direct Bearer token (OAuth)
    if let Some(t) = token {
        let t = store.resolve_value(t);
        if !t.is_empty() {
            return Ok(ResolvedAuth {
                kind: AuthKind::BearerToken,
                value: t,
            });
        }
    }

    // Direct API key
    if let Some(k) = api_key {
        let k = store.resolve_value(k);
        if !k.is_empty() {
            warn_on_key_mismatch(driver, &k);
            return Ok(ResolvedAuth {
                kind: AuthKind::ApiKey,
                value: k,
            });
        }
    }

    // Driver-specific secret/env fallback
    if !driver.needs_api_key() {
        return Ok(ResolvedAuth {
            kind: AuthKind::ApiKey,
            value: String::new(),
        });
    }

    let auth = secret_or_env_auth(store, driver.env_var())?;

    if !auth.value.is_empty() {
        warn_on_key_mismatch(driver, &auth.value);
    }

    Ok(auth)
}

/// Known API key prefix → expected driver mapping.
const KEY_PREFIX_DRIVERS: &[(&str, Driver)] = &[
    ("sk-ant-", Driver::Anthropic),
    ("sk-proj-", Driver::OpenAi),
    ("gsk_", Driver::Groq),
    ("xai-", Driver::Xai),
    ("sk-", Driver::OpenAi),
];

/// Emits a warning if the API key prefix suggests a different provider.
fn warn_on_key_mismatch(driver: Driver, key: &str) {
    for &(prefix, expected_driver) in KEY_PREFIX_DRIVERS {
        if key.starts_with(prefix) && driver != expected_driver {
            // Don't warn for openai-compatible drivers using sk- keys
            if prefix == "sk-"
                && matches!(
                    driver,
                    Driver::OpenAiCompatible | Driver::LmStudio | Driver::Vllm | Driver::Mistral
                )
            {
                return;
            }
            warn!(
                driver = driver.as_str(),
                expected_driver = expected_driver.as_str(),
                key_prefix = prefix,
                "API key prefix suggests a different provider — possible misconfiguration"
            );
            return;
        }
    }
}

/// Looks up a credential by name: SecretStore first, then env.
fn secret_or_env_auth(
    store: &SecretStore,
    var_name: &str,
) -> Result<ResolvedAuth, AuthError> {
    // SecretStore (decrypted secrets from .env, never in process env)
    if let Some(v) = store.get(var_name)
        && !v.is_empty()
    {
        return Ok(ResolvedAuth {
            kind: AuthKind::ApiKey,
            value: v,
        });
    }
    // Fallback: process env (for non-secret vars or CI environments)
    match std::env::var(var_name) {
        Ok(v) if !v.is_empty() => Ok(ResolvedAuth {
            kind: AuthKind::ApiKey,
            value: v,
        }),
        _ => Err(AuthError::MissingCredentials(format!(
            "{var_name} not set"
        ))),
    }
}

#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("missing credentials: {0}")]
    MissingCredentials(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direct_token_takes_priority() {
        let auth = resolve_auth(Driver::Anthropic, Some("key123"), Some("token456")).unwrap();
        assert_eq!(auth.kind, AuthKind::BearerToken);
        assert_eq!(auth.value, "token456");
    }

    #[test]
    fn direct_api_key() {
        let auth = resolve_auth(Driver::Anthropic, Some("key123"), None).unwrap();
        assert_eq!(auth.kind, AuthKind::ApiKey);
        assert_eq!(auth.value, "key123");
    }

    #[test]
    fn ollama_no_auth_needed() {
        let auth = resolve_auth(Driver::Ollama, None, None).unwrap();
        assert!(auth.value.is_empty());
    }

    #[test]
    fn resolves_from_secret_store() {
        let store = SecretStore::global();
        store.set("OZZIE_TEST_SECRET_KEY", "from_store");
        let auth =
            resolve_auth(Driver::Anthropic, Some("${OZZIE_TEST_SECRET_KEY}"), None).unwrap();
        assert_eq!(auth.value, "from_store");
    }

    #[test]
    fn driver_fallback_from_secret_store() {
        let store = SecretStore::global();
        store.set("ANTHROPIC_API_KEY", "sk-ant-from-store");
        let auth = resolve_auth(Driver::Anthropic, None, None).unwrap();
        assert_eq!(auth.value, "sk-ant-from-store");
    }

    #[test]
    fn openai_compatible_no_auth_needed() {
        let auth = resolve_auth(Driver::OpenAiCompatible, None, None).unwrap();
        assert!(auth.value.is_empty());
    }

    #[test]
    fn lm_studio_no_auth_needed() {
        let auth = resolve_auth(Driver::LmStudio, None, None).unwrap();
        assert!(auth.value.is_empty());
    }

    #[test]
    fn vllm_no_auth_needed() {
        let auth = resolve_auth(Driver::Vllm, None, None).unwrap();
        assert!(auth.value.is_empty());
    }

    #[test]
    fn warn_on_key_mismatch_does_not_panic() {
        warn_on_key_mismatch(Driver::OpenAi, "sk-ant-api03-test123");
        warn_on_key_mismatch(Driver::Anthropic, "sk-ant-api03-test123");
        warn_on_key_mismatch(Driver::Mistral, "sk-proj-test123");
        warn_on_key_mismatch(Driver::OpenAi, "custom-key-123");
    }
}
