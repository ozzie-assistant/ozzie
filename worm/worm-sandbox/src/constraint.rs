use regex::Regex;

/// Per-tool runtime constraints (from task config or session).
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct ToolConstraints {
    /// Whitelist of allowed binary names.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed_commands: Vec<String>,
    /// Regex patterns that each subcommand must match.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed_patterns: Vec<String>,
    /// Regex patterns that block execution.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blocked_patterns: Vec<String>,
    /// Glob patterns for allowed file paths.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed_paths: Vec<String>,
    /// Domain whitelist for URLs (supports `*.example.com`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed_domains: Vec<String>,
}

/// Validates tool arguments against runtime constraints.
pub struct ConstraintGuard {
    tool_name: String,
    constraints: ToolConstraints,
}

impl ConstraintGuard {
    pub fn new(tool_name: &str, constraints: ToolConstraints) -> Self {
        Self {
            tool_name: tool_name.to_string(),
            constraints,
        }
    }

    /// Validates a shell command against constraints.
    pub fn validate_command(&self, command: &str) -> Result<(), ConstraintError> {
        let subcmds = split_subcommands(command);

        // Allowed commands: every binary must be in the whitelist
        if !self.constraints.allowed_commands.is_empty() {
            for subcmd in &subcmds {
                let binary = subcmd
                    .split_whitespace()
                    .next()
                    .unwrap_or("")
                    .rsplit('/')
                    .next()
                    .unwrap_or("");

                if !self.constraints.allowed_commands.iter().any(|c| c == binary) {
                    return Err(ConstraintError::CommandNotAllowed(format!(
                        "tool '{}': command '{}' not in allowed list",
                        self.tool_name, binary
                    )));
                }
            }
        }

        // Allowed patterns: each subcmd must match at least one
        if !self.constraints.allowed_patterns.is_empty() {
            for subcmd in &subcmds {
                if !matches_any_pattern(subcmd, &self.constraints.allowed_patterns) {
                    return Err(ConstraintError::PatternMismatch(format!(
                        "tool '{}': command '{}' doesn't match allowed patterns",
                        self.tool_name, subcmd
                    )));
                }
            }
        }

        // Blocked patterns: no subcmd may match
        if !self.constraints.blocked_patterns.is_empty() {
            for subcmd in &subcmds {
                if matches_any_pattern(subcmd, &self.constraints.blocked_patterns) {
                    return Err(ConstraintError::BlockedPattern(format!(
                        "tool '{}': command '{}' matches a blocked pattern",
                        self.tool_name, subcmd
                    )));
                }
            }
        }

        Ok(())
    }

    /// Validates a file path against constraints.
    pub fn validate_path(&self, path: &str) -> Result<(), ConstraintError> {
        if self.constraints.allowed_paths.is_empty() {
            return Ok(());
        }

        if !matches_any_glob(path, &self.constraints.allowed_paths) {
            return Err(ConstraintError::PathNotAllowed(format!(
                "tool '{}': path '{}' not in allowed paths",
                self.tool_name, path
            )));
        }

        Ok(())
    }

    /// Validates a URL domain against constraints.
    pub fn validate_domain(&self, url: &str) -> Result<(), ConstraintError> {
        if self.constraints.allowed_domains.is_empty() {
            return Ok(());
        }

        let domain = extract_domain(url).unwrap_or_default();
        if !matches_domain(&domain, &self.constraints.allowed_domains) {
            return Err(ConstraintError::DomainNotAllowed(format!(
                "tool '{}': domain '{}' not in allowed domains",
                self.tool_name, domain
            )));
        }

        Ok(())
    }
}

/// Splits a command on `&&`, `||`, `;`, `|`.
fn split_subcommands(cmd: &str) -> Vec<String> {
    // Reuse the same logic as sandbox but return owned strings
    let mut parts = Vec::new();
    let mut current = String::new();
    let chars: Vec<char> = cmd.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        match chars[i] {
            ';' => {
                let trimmed = current.trim().to_string();
                if !trimmed.is_empty() {
                    parts.push(trimmed);
                }
                current.clear();
            }
            '|' => {
                if i + 1 < chars.len() && chars[i + 1] == '|' {
                    let trimmed = current.trim().to_string();
                    if !trimmed.is_empty() {
                        parts.push(trimmed);
                    }
                    current.clear();
                    i += 1;
                } else {
                    let trimmed = current.trim().to_string();
                    if !trimmed.is_empty() {
                        parts.push(trimmed);
                    }
                    current.clear();
                }
            }
            '&' => {
                if i + 1 < chars.len() && chars[i + 1] == '&' {
                    let trimmed = current.trim().to_string();
                    if !trimmed.is_empty() {
                        parts.push(trimmed);
                    }
                    current.clear();
                    i += 1;
                } else {
                    current.push(chars[i]);
                }
            }
            c => current.push(c),
        }
        i += 1;
    }
    let trimmed = current.trim().to_string();
    if !trimmed.is_empty() {
        parts.push(trimmed);
    }
    parts
}

/// Returns true if `s` matches any of the regex patterns.
fn matches_any_pattern(s: &str, patterns: &[String]) -> bool {
    patterns.iter().any(|p| {
        Regex::new(p)
            .map(|re| re.is_match(s))
            .unwrap_or(false)
    })
}

/// Returns true if `path` matches any of the glob patterns.
fn matches_any_glob(path: &str, globs: &[String]) -> bool {
    globs.iter().any(|g| {
        // Simple glob matching: * matches any sequence, ** matches dirs
        let pattern = glob_to_regex(g);
        Regex::new(&pattern)
            .map(|re| re.is_match(path))
            .unwrap_or(false)
    })
}

/// Converts a simple glob pattern to a regex.
fn glob_to_regex(glob: &str) -> String {
    let mut regex = String::from("^");
    let mut chars = glob.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '*' => {
                if chars.peek() == Some(&'*') {
                    chars.next();
                    if chars.peek() == Some(&'/') {
                        chars.next();
                        regex.push_str("(.*/)?");
                    } else {
                        regex.push_str(".*");
                    }
                } else {
                    regex.push_str("[^/]*");
                }
            }
            '?' => regex.push_str("[^/]"),
            '.' | '+' | '^' | '$' | '(' | ')' | '{' | '}' | '[' | ']' | '|' | '\\' => {
                regex.push('\\');
                regex.push(c);
            }
            _ => regex.push(c),
        }
    }
    regex.push('$');
    regex
}

/// Extracts the domain from a URL.
fn extract_domain(url: &str) -> Option<String> {
    let without_scheme = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url);
    let domain = without_scheme.split('/').next()?;
    let domain = domain.split(':').next()?;
    Some(domain.to_lowercase())
}

/// Returns true if `domain` matches any of the allowed domains.
/// Supports wildcards like `*.example.com`.
fn matches_domain(domain: &str, allowed: &[String]) -> bool {
    allowed.iter().any(|pattern| {
        if let Some(suffix) = pattern.strip_prefix("*.") {
            domain == suffix || domain.ends_with(&format!(".{suffix}"))
        } else {
            domain == pattern
        }
    })
}

#[derive(Debug, thiserror::Error)]
pub enum ConstraintError {
    #[error("{0}")]
    CommandNotAllowed(String),
    #[error("{0}")]
    PatternMismatch(String),
    #[error("{0}")]
    BlockedPattern(String),
    #[error("{0}")]
    PathNotAllowed(String),
    #[error("{0}")]
    DomainNotAllowed(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allowed_commands_blocks() {
        let guard = ConstraintGuard::new(
            "cmd",
            ToolConstraints {
                allowed_commands: vec!["ls".to_string(), "cat".to_string()],
                ..Default::default()
            },
        );
        assert!(guard.validate_command("ls -la").is_ok());
        assert!(guard.validate_command("rm file").is_err());
    }

    #[test]
    fn blocked_patterns() {
        let guard = ConstraintGuard::new(
            "cmd",
            ToolConstraints {
                blocked_patterns: vec!["rm.*-rf".to_string()],
                ..Default::default()
            },
        );
        assert!(guard.validate_command("rm -rf /").is_err());
        assert!(guard.validate_command("rm file.txt").is_ok());
    }

    #[test]
    fn domain_matching() {
        assert!(matches_domain("api.example.com", &["*.example.com".to_string()]));
        assert!(matches_domain("example.com", &["*.example.com".to_string()]));
        assert!(!matches_domain("evil.com", &["*.example.com".to_string()]));
        assert!(matches_domain("exact.com", &["exact.com".to_string()]));
    }

    #[test]
    fn glob_matching() {
        assert!(matches_any_glob("/home/user/project/src/main.rs", &["/home/user/project/**".to_string()]));
        assert!(!matches_any_glob("/etc/passwd", &["/home/user/project/**".to_string()]));
        assert!(matches_any_glob("/tmp/file.txt", &["/tmp/*.txt".to_string()]));
    }

    #[test]
    fn extract_domain_works() {
        assert_eq!(extract_domain("https://api.example.com/v1"), Some("api.example.com".to_string()));
        assert_eq!(extract_domain("http://localhost:8080/path"), Some("localhost".to_string()));
    }
}
