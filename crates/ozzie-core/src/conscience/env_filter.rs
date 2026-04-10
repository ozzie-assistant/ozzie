/// Environment variables that must be stripped from subprocesses spawned by
/// the agent. Two categories:
///
/// 1. **Secrets** — API keys, tokens, credentials that should never leak to
///    arbitrary shell commands.
/// 2. **Hijack vectors** — variables that alter library loading, interpreter
///    behaviour, or package resolution. An attacker-controlled command could
///    use these to execute arbitrary code even inside an OS sandbox.
///
/// Inspired by Goose's 31-item blocklist, extended with Ozzie-specific keys.
/// Variables stripped from every subprocess environment.
pub const BLOCKED_ENV_VARS: &[&str] = &[
    // ── Secrets ──────────────────────────────────────────────────────────
    "ANTHROPIC_API_KEY",
    "OPENAI_API_KEY",
    "GOOGLE_API_KEY",
    "AZURE_OPENAI_API_KEY",
    "MISTRAL_API_KEY",
    "GROQ_API_KEY",
    "XAI_API_KEY",
    "COHERE_API_KEY",
    "HUGGING_FACE_TOKEN",
    "HF_TOKEN",
    "AWS_SECRET_ACCESS_KEY",
    "AWS_SESSION_TOKEN",
    "GH_TOKEN",
    "GITHUB_TOKEN",
    "GITLAB_TOKEN",
    "SLACK_TOKEN",
    "DISCORD_TOKEN",
    "DATABASE_URL",
    "REDIS_URL",
    "OZZIE_GATEWAY_TOKEN",
    // ── Library / interpreter hijacking ──────────────────────────────────
    "LD_PRELOAD",
    "LD_LIBRARY_PATH",
    "DYLD_INSERT_LIBRARIES",
    "DYLD_LIBRARY_PATH",
    "DYLD_FRAMEWORK_PATH",
    "PYTHONPATH",
    "PYTHONSTARTUP",
    "PYTHONHOME",
    "NODE_OPTIONS",
    "NODE_PATH",
    "RUBYOPT",
    "RUBYLIB",
    "GEM_PATH",
    "PERL5LIB",
    "PERL5OPT",
    "CLASSPATH",
    "JAVA_TOOL_OPTIONS",
    "BASH_ENV",
    "ENV",
    "ZDOTDIR",
    "COMPOSER_HOME",
    "npm_config_prefix",
    "npm_config_userconfig",
    "PIP_CONFIG_FILE",
    "PIP_INDEX_URL",
    "GIT_SSH_COMMAND",
    "GIT_EXEC_PATH",
    "SSL_CERT_FILE",
    "CURL_CA_BUNDLE",
];

/// Applies env-var filtering to a [`tokio::process::Command`].
///
/// Removes every variable in [`BLOCKED_ENV_VARS`] from the command's
/// inherited environment.
pub fn strip_blocked_env(cmd: &mut tokio::process::Command) {
    for var in BLOCKED_ENV_VARS {
        cmd.env_remove(var);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocklist_has_no_duplicates() {
        let mut seen = std::collections::HashSet::new();
        for var in BLOCKED_ENV_VARS {
            assert!(seen.insert(var), "duplicate env var in blocklist: {var}");
        }
    }

    #[test]
    fn blocklist_excludes_safe_vars() {
        // These must never be blocked — they'd break every subprocess.
        for safe in &["PATH", "HOME", "USER", "SHELL", "TERM", "LANG", "LC_ALL"] {
            assert!(
                !BLOCKED_ENV_VARS.contains(safe),
                "{safe} must not be in BLOCKED_ENV_VARS"
            );
        }
    }
}
