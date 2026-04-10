use std::path::Path;

use tracing::debug;

/// Governance rules for wiki page synthesis.
///
/// Loaded from `$OZZIE_PATH/memory_schema.md` (markdown with YAML frontmatter).
/// When the file is absent, defaults are used. The markdown body is injected
/// as additional instructions into the LLM synthesis prompts.
#[derive(Debug, Clone)]
pub struct MemorySchema {
    /// Max content length in chars before triggering a page split.
    pub max_page_chars: usize,
    /// Language directive for page content (e.g. "fr", "en").
    pub language: Option<String>,
    /// Extra instructions injected into synthesis prompts (the markdown body).
    pub instructions: String,
}

const DEFAULT_MAX_PAGE_CHARS: usize = 6000;

impl Default for MemorySchema {
    fn default() -> Self {
        Self {
            max_page_chars: DEFAULT_MAX_PAGE_CHARS,
            language: None,
            instructions: String::new(),
        }
    }
}

impl MemorySchema {
    /// Loads the schema from `$OZZIE_PATH/memory_schema.md`.
    /// Returns defaults if the file doesn't exist.
    pub fn load(ozzie_path: &Path) -> Self {
        let path = ozzie_path.join("memory_schema.md");
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(_) => {
                debug!("no memory_schema.md found, using defaults");
                return Self::default();
            }
        };

        parse(&text).unwrap_or_default()
    }
}

fn parse(text: &str) -> Option<MemorySchema> {
    let (yaml, body) = split_frontmatter(text)?;

    let mut schema = MemorySchema::default();
    schema.instructions = body.trim().to_string();

    for line in yaml.lines() {
        let line = line.trim();
        if let Some((key, value)) = line.split_once(':') {
            let key = key.trim();
            let value = value.trim();
            match key {
                "max_page_chars" => {
                    if let Ok(n) = value.parse::<usize>() {
                        schema.max_page_chars = n;
                    }
                }
                "language" => {
                    if !value.is_empty() {
                        schema.language = Some(value.to_string());
                    }
                }
                _ => {}
            }
        }
    }

    Some(schema)
}

fn split_frontmatter(text: &str) -> Option<(&str, &str)> {
    let text = text.trim_start();
    if !text.starts_with("---") {
        // No frontmatter — treat entire text as instructions
        return Some(("", text));
    }
    let after_first = &text[3..].trim_start_matches(['\r', '\n']);
    let end = after_first.find("\n---")?;
    let yaml = &after_first[..end];
    let body = after_first[end + 4..].trim_start_matches(['\r', '\n']);
    Some((yaml, body))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_full_schema() {
        let text = "---\nmax_page_chars: 4000\nlanguage: fr\n---\n\n## Conventions\n\nUse headers.\n";
        let schema = parse(text).unwrap();
        assert_eq!(schema.max_page_chars, 4000);
        assert_eq!(schema.language.as_deref(), Some("fr"));
        assert!(schema.instructions.contains("Use headers."));
    }

    #[test]
    fn parse_minimal_frontmatter() {
        let text = "---\nlanguage: en\n---\n\nKeep it simple.";
        let schema = parse(text).unwrap();
        assert_eq!(schema.max_page_chars, DEFAULT_MAX_PAGE_CHARS);
        assert_eq!(schema.language.as_deref(), Some("en"));
        assert_eq!(schema.instructions, "Keep it simple.");
    }

    #[test]
    fn parse_no_frontmatter() {
        let text = "Just some instructions.";
        let schema = parse(text).unwrap();
        assert_eq!(schema.max_page_chars, DEFAULT_MAX_PAGE_CHARS);
        assert!(schema.language.is_none());
        assert_eq!(schema.instructions, "Just some instructions.");
    }

    #[test]
    fn load_missing_file_returns_defaults() {
        let schema = MemorySchema::load(Path::new("/nonexistent"));
        assert_eq!(schema.max_page_chars, DEFAULT_MAX_PAGE_CHARS);
        assert!(schema.language.is_none());
        assert!(schema.instructions.is_empty());
    }

    #[test]
    fn load_from_tempdir() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("memory_schema.md"),
            "---\nmax_page_chars: 3000\n---\n\nCustom rules.",
        )
        .unwrap();

        let schema = MemorySchema::load(dir.path());
        assert_eq!(schema.max_page_chars, 3000);
        assert_eq!(schema.instructions, "Custom rules.");
    }
}
