/// Environment variables that must be stripped from subprocesses spawned by
/// the agent. Two categories:
///
/// 1. **Secrets** — API keys, tokens, credentials that should never leak to
///    arbitrary shell commands.
/// 2. **Hijack vectors** — variables that alter library loading, interpreter
///    behaviour, or package resolution. An attacker-controlled command could
///    use these to execute arbitrary code even inside an OS sandbox.
pub static BLOCKED_ENV_VARS: &[&str] = &[
    // ── Secrets ──
    "ANTHROPIC_API_KEY",
    "OPENAI_API_KEY",
    "GEMINI_API_KEY",
    "MISTRAL_API_KEY",
    "GROQ_API_KEY",
    "XAI_API_KEY",
    "OLLAMA_API_KEY",
    "GITHUB_TOKEN",
    "GH_TOKEN",
    "GITLAB_TOKEN",
    "AWS_ACCESS_KEY_ID",
    "AWS_SECRET_ACCESS_KEY",
    "AWS_SESSION_TOKEN",
    "AZURE_OPENAI_KEY",
    "GOOGLE_APPLICATION_CREDENTIALS",
    "HOMEBREW_GITHUB_API_TOKEN",
    "NPM_TOKEN",
    "CARGO_REGISTRY_TOKEN",
    "DOCKER_PASSWORD",
    // ── Hijack vectors ──
    "LD_PRELOAD",
    "LD_LIBRARY_PATH",
    "DYLD_INSERT_LIBRARIES",
    "DYLD_LIBRARY_PATH",
    "DYLD_FRAMEWORK_PATH",
    "PYTHONPATH",
    "PYTHONSTARTUP",
    "RUBYOPT",
    "RUBYLIB",
    "PERL5OPT",
    "PERL5LIB",
    "NODE_OPTIONS",
    "NODE_PATH",
    "CLASSPATH",
    "JAVA_TOOL_OPTIONS",
    "_JAVA_OPTIONS",
    "GOFLAGS",
    "RUSTFLAGS",
    "CARGO_BUILD_RUSTFLAGS",
    "BASH_ENV",
    "ENV",
    "ZDOTDIR",
    "PROMPT_COMMAND",
    "GIT_SSH_COMMAND",
    "GIT_EXEC_PATH",
    "SSL_CERT_FILE",
    "CURL_CA_BUNDLE",
];

/// Applies env-var filtering to a [`std::process::Command`].
///
/// Removes every variable in [`BLOCKED_ENV_VARS`] from the command's
/// inherited environment.
pub fn strip_blocked_env_std(cmd: &mut std::process::Command) {
    for var in BLOCKED_ENV_VARS {
        cmd.env_remove(var);
    }
}

/// Applies env-var filtering to a [`tokio::process::Command`].
pub fn strip_blocked_env(cmd: &mut tokio::process::Command) {
    for var in BLOCKED_ENV_VARS {
        cmd.env_remove(var);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocked_list_not_empty() {
        assert!(!BLOCKED_ENV_VARS.is_empty());
    }

    #[test]
    fn contains_known_secrets() {
        assert!(BLOCKED_ENV_VARS.contains(&"ANTHROPIC_API_KEY"));
        assert!(BLOCKED_ENV_VARS.contains(&"OPENAI_API_KEY"));
    }

    #[test]
    fn contains_hijack_vectors() {
        assert!(BLOCKED_ENV_VARS.contains(&"LD_PRELOAD"));
        assert!(BLOCKED_ENV_VARS.contains(&"NODE_OPTIONS"));
    }
}
