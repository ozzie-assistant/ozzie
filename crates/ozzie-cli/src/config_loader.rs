use std::path::Path;

use regex::Regex;

use ozzie_core::config::Config;

/// Function type for decrypting `ENC[age:...]` values.
pub type DecryptFn = Box<dyn Fn(&str) -> Result<String, String> + Send + Sync>;

/// Function type for resolving environment/secret variables by name.
/// Returns the resolved value for a given variable name.
pub type ResolverFn = Box<dyn Fn(&str) -> String + Send + Sync>;

/// Function signature for decryption callbacks (used in expand_env_templates).
type DecryptRef<'a> = Option<&'a (dyn Fn(&str) -> Result<String, String> + Send + Sync)>;

/// Function signature for resolver callbacks.
type ResolverRef<'a> = Option<&'a (dyn Fn(&str) -> String + Send + Sync)>;

/// Options for config loading.
#[derive(Default)]
pub struct LoadOptions {
    pub decrypt: Option<DecryptFn>,
    /// Custom variable resolver. When set, `${{ .Env.VAR }}` is resolved
    /// via this function instead of `std::env::var`.
    /// Used to resolve secrets from SecretStore without exposing them in env.
    pub resolver: Option<ResolverFn>,
}

/// Reads a JSONC config file, strips comments, expands `${{ .Env.VAR }}` templates,
/// unmarshals it into Config, and applies defaults.
pub fn load(path: &Path) -> Result<Config, ConfigError> {
    load_with_options(path, LoadOptions::default())
}

/// Reads a JSONC file and deserializes it into any type `T`.
///
/// Expands `${{ .Env.VAR }}` templates using the current process environment.
/// Useful for loading partial configs (e.g. `connectors/discord.jsonc`).
pub fn load_partial<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, ConfigError> {
    let data = std::fs::read_to_string(path)
        .map_err(|e| ConfigError::Read(format!("read config: {e}")))?;
    let expanded = expand_env_templates(&data, None, None);
    let standardized = strip_jsonc(&expanded);
    serde_json::from_str(&standardized)
        .map_err(|e| ConfigError::Parse(format!("unmarshal config: {e}")))
}

/// Reads a JSONC config file with options (e.g. decryption).
pub fn load_with_options(path: &Path, opts: LoadOptions) -> Result<Config, ConfigError> {
    let data = std::fs::read_to_string(path)
        .map_err(|e| ConfigError::Read(format!("read config: {e}")))?;

    // Expand env templates before stripping comments
    let expanded = expand_env_templates(
        &data,
        opts.decrypt.as_deref(),
        opts.resolver.as_deref(),
    );

    // Strip JSONC comments and trailing commas
    let standardized = strip_jsonc(&expanded);

    let cfg: Config = serde_json::from_str(&standardized)
        .map_err(|e| ConfigError::Parse(format!("unmarshal config: {e}")))?;

    Ok(cfg)
}

/// Replaces `${{ .Env.VAR }}` with the resolved variable value.
///
/// If `resolver` is provided, uses it (SecretStore → env fallback).
/// Otherwise falls back to `std::env::var`.
/// Values are JSON-escaped before injection to prevent template injection.
fn expand_env_templates(
    s: &str,
    decrypt: DecryptRef<'_>,
    resolver: ResolverRef<'_>,
) -> String {
    let re = Regex::new(r"\$\{\{\s*\.Env\.(\w+)\s*\}\}").unwrap();
    re.replace_all(s, |caps: &regex::Captures| {
        let var_name = &caps[1];
        let value = match resolver {
            Some(resolve) => resolve(var_name),
            None => std::env::var(var_name).unwrap_or_default(),
        };
        if let Some(decrypt_fn) = decrypt
            && let Ok(decrypted) = decrypt_fn(&value)
        {
            return json_escape_value(&decrypted);
        }
        json_escape_value(&value)
    })
    .into_owned()
}

/// Escapes a string for safe embedding inside a JSON string literal.
fn json_escape_value(s: &str) -> String {
    let json = serde_json::to_string(s).unwrap_or_else(|_| format!("\"{s}\""));
    // Strip the surrounding quotes
    json[1..json.len() - 1].to_string()
}

/// Strips JSONC comments (// and /* */) and trailing commas.
fn strip_jsonc(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let chars: Vec<char> = s.chars().collect();
    let len = chars.len();
    let mut i = 0;
    let mut in_string = false;

    while i < len {
        // Handle string literals
        if chars[i] == '"' && !is_escaped(&chars, i) {
            in_string = !in_string;
            result.push(chars[i]);
            i += 1;
            continue;
        }

        if in_string {
            result.push(chars[i]);
            i += 1;
            continue;
        }

        // Single-line comment
        if i + 1 < len && chars[i] == '/' && chars[i + 1] == '/' {
            while i < len && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }

        // Multi-line comment
        if i + 1 < len && chars[i] == '/' && chars[i + 1] == '*' {
            i += 2;
            while i + 1 < len && !(chars[i] == '*' && chars[i + 1] == '/') {
                i += 1;
            }
            if i + 1 < len {
                i += 2; // skip */
            }
            continue;
        }

        // Trailing comma before } or ]
        if chars[i] == ',' {
            // Look ahead for } or ] (skipping whitespace)
            let mut j = i + 1;
            while j < len && chars[j].is_whitespace() {
                j += 1;
            }
            if j < len && (chars[j] == '}' || chars[j] == ']') {
                i += 1; // skip the trailing comma
                continue;
            }
        }

        result.push(chars[i]);
        i += 1;
    }

    result
}

fn is_escaped(chars: &[char], pos: usize) -> bool {
    if pos == 0 {
        return false;
    }
    let mut backslashes = 0;
    let mut i = pos - 1;
    loop {
        if chars[i] == '\\' {
            backslashes += 1;
        } else {
            break;
        }
        if i == 0 {
            break;
        }
        i -= 1;
    }
    backslashes % 2 != 0
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("{0}")]
    Read(String),
    #[error("{0}")]
    Parse(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn strip_jsonc_comments() {
        let input = r#"{
    // This is a comment
    "key": "value", // inline comment
    /* block comment */
    "key2": "value2",
}"#;
        let result = strip_jsonc(input);
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["key"], "value");
        assert_eq!(parsed["key2"], "value2");
    }

    #[test]
    fn strip_jsonc_trailing_commas() {
        let input = r#"{"a": 1, "b": 2,}"#;
        let result = strip_jsonc(input);
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["a"], 1);
    }

    #[test]
    fn json_escape_special_chars() {
        assert_eq!(json_escape_value("hello"), "hello");
        assert_eq!(json_escape_value("he\"llo"), r#"he\"llo"#);
        assert_eq!(json_escape_value("he\\llo"), r"he\\llo");
    }

    #[test]
    fn expand_env_templates_basic() {
        // SAFETY: test runs single-threaded for this env var
        unsafe { std::env::set_var("OZZIE_TEST_KEY_42", "my_secret") };
        let input = r#"{"api_key": "${{ .Env.OZZIE_TEST_KEY_42 }}"}"#;
        let result = expand_env_templates(input, None, None);
        assert_eq!(result, r#"{"api_key": "my_secret"}"#);
        unsafe { std::env::remove_var("OZZIE_TEST_KEY_42") };
    }

    #[test]
    fn load_config_from_jsonc() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.jsonc");
        let mut f = std::fs::File::create(&path).unwrap();
        write!(
            f,
            r#"{{
    // Gateway config
    "gateway": {{
        "host": "0.0.0.0",
        "port": 9999,
    }},
    "events": {{
        "buffer_size": 512,
    }},
}}"#
        )
        .unwrap();

        let cfg = load(&path).unwrap();
        assert_eq!(cfg.gateway.host, "0.0.0.0");
        assert_eq!(cfg.gateway.port, 9999);
        assert_eq!(cfg.events.buffer_size, 512);
    }

    #[test]
    fn defaults_applied() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.jsonc");
        std::fs::write(&path, "{}").unwrap();

        let cfg = load(&path).unwrap();
        assert_eq!(cfg.gateway.host, "127.0.0.1");
        assert_eq!(cfg.gateway.port, 18420);
        assert_eq!(cfg.events.buffer_size, 1024);
        assert_eq!(cfg.events.log_level, "info");
    }
}
