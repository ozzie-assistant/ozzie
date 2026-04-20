use chrono::{DateTime, Utc};

use crate::entry::{ImportanceLevel, MemoryEntry, MemoryType};
use crate::error::MemoryError;

/// Serializes a MemoryEntry + content into a markdown file with YAML frontmatter.
pub fn serialize(entry: &MemoryEntry, content: &str) -> String {
    let mut fm = String::new();
    fm.push_str("---\n");
    fm.push_str(&format!("id: {}\n", entry.id));
    fm.push_str(&format!("type: {}\n", entry.memory_type.as_str()));
    if !entry.tags.is_empty() {
        let tags: Vec<String> = entry.tags.iter().map(|t| format!("\"{t}\"")).collect();
        fm.push_str(&format!("tags: [{}]\n", tags.join(", ")));
    }
    fm.push_str(&format!("importance: {}\n", entry.importance.as_str()));
    fm.push_str(&format!("confidence: {}\n", entry.confidence));
    if !entry.source.is_empty() {
        fm.push_str(&format!("source: {}\n", entry.source));
    }
    fm.push_str(&format!("created: {}\n", entry.created_at.to_rfc3339()));
    fm.push_str(&format!("updated: {}\n", entry.updated_at.to_rfc3339()));
    fm.push_str("---\n\n");

    fm.push_str(&format!("# {}\n\n", entry.title));
    fm.push_str(content);
    if !content.ends_with('\n') {
        fm.push('\n');
    }
    fm
}

/// Parses a markdown file with YAML frontmatter into (MemoryEntry, content).
pub fn parse(text: &str) -> Result<(MemoryEntry, String), MemoryError> {
    let (yaml, body) = split_frontmatter(text)
        .ok_or_else(|| MemoryError::Other("missing YAML frontmatter".into()))?;

    let mut id = String::new();
    let mut memory_type = MemoryType::Fact;
    let mut tags = Vec::new();
    let mut importance = ImportanceLevel::Normal;
    let mut confidence = 0.8;
    let mut source = String::new();
    let mut created_at = Utc::now();
    let mut updated_at = Utc::now();

    for line in yaml.lines() {
        let line = line.trim();
        if let Some((key, value)) = line.split_once(':') {
            let key = key.trim();
            let value = value.trim();
            match key {
                "id" => id = value.to_string(),
                "type" => memory_type = value.parse().unwrap_or(MemoryType::Fact),
                "tags" => tags = parse_yaml_array(value),
                "importance" => importance = value.parse().unwrap_or_default(),
                "confidence" => confidence = value.parse().unwrap_or(0.8),
                "source" => source = value.to_string(),
                "created" => {
                    if let Ok(dt) = DateTime::parse_from_rfc3339(value) {
                        created_at = dt.with_timezone(&Utc);
                    }
                }
                "updated" => {
                    if let Ok(dt) = DateTime::parse_from_rfc3339(value) {
                        updated_at = dt.with_timezone(&Utc);
                    }
                }
                _ => {}
            }
        }
    }

    let (title, content) = extract_title_and_content(body);

    let entry = MemoryEntry {
        id,
        title,
        source,
        memory_type,
        tags,
        created_at,
        updated_at,
        last_used_at: updated_at,
        confidence,
        importance,
        embedding_model: String::new(),
        indexed_at: None,
        merged_into: None,
    };

    Ok((entry, content))
}

/// Generates a slug-based filename: `{slug}_{short_id}.md`.
pub fn filename_for(entry: &MemoryEntry) -> String {
    let slug = slugify(&entry.title, 50);
    let short_id = if entry.id.len() >= 8 {
        &entry.id[entry.id.len().saturating_sub(4)..]
    } else {
        &entry.id
    };
    format!("{slug}_{short_id}.md")
}

fn split_frontmatter(text: &str) -> Option<(&str, &str)> {
    let text = text.trim_start();
    if !text.starts_with("---") {
        return None;
    }
    let after_first = &text[3..].trim_start_matches(['\r', '\n']);
    let end = after_first.find("\n---")?;
    let yaml = &after_first[..end];
    let body = after_first[end + 4..].trim_start_matches(['\r', '\n']);
    Some((yaml, body))
}

fn extract_title_and_content(body: &str) -> (String, String) {
    let body = body.trim();
    if let Some(rest) = body.strip_prefix("# ") {
        if let Some(nl) = rest.find('\n') {
            let title = rest[..nl].trim().to_string();
            let content = rest[nl..].trim().to_string();
            return (title, content);
        }
        return (rest.trim().to_string(), String::new());
    }
    if let Some(nl) = body.find('\n') {
        let title = body[..nl].trim().to_string();
        let content = body[nl..].trim().to_string();
        (title, content)
    } else {
        (body.to_string(), String::new())
    }
}

fn parse_yaml_array(s: &str) -> Vec<String> {
    let s = s.trim();
    if !s.starts_with('[') || !s.ends_with(']') {
        return if s.is_empty() {
            Vec::new()
        } else {
            vec![s.to_string()]
        };
    }
    let inner = &s[1..s.len() - 1];
    inner
        .split(',')
        .map(|item| {
            item.trim()
                .trim_matches('"')
                .trim_matches('\'')
                .to_string()
        })
        .filter(|s| !s.is_empty())
        .collect()
}

fn slugify(text: &str, max_len: usize) -> String {
    let slug: String = text
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let mut result = String::new();
    let mut prev_hyphen = false;
    for c in slug.chars() {
        if c == '-' {
            if !prev_hyphen && !result.is_empty() {
                result.push('-');
            }
            prev_hyphen = true;
        } else {
            result.push(c);
            prev_hyphen = false;
        }
    }
    let result = result.trim_end_matches('-').to_string();
    if result.len() > max_len {
        let truncated = &result[..max_len];
        if let Some(last_hyphen) = truncated.rfind('-') {
            truncated[..last_hyphen].to_string()
        } else {
            truncated.to_string()
        }
    } else {
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_entry() -> MemoryEntry {
        MemoryEntry {
            id: "mem_cosmic_asimov".to_string(),
            title: "Project Architecture".to_string(),
            source: "user".to_string(),
            memory_type: MemoryType::Fact,
            tags: vec!["architecture".to_string(), "rust".to_string()],
            created_at: DateTime::parse_from_rfc3339("2026-03-23T06:00:00Z")
                .unwrap()
                .with_timezone(&Utc),
            updated_at: DateTime::parse_from_rfc3339("2026-03-23T06:00:00Z")
                .unwrap()
                .with_timezone(&Utc),
            last_used_at: DateTime::parse_from_rfc3339("2026-03-23T06:00:00Z")
                .unwrap()
                .with_timezone(&Utc),
            confidence: 0.9,
            importance: ImportanceLevel::Core,
            embedding_model: String::new(),
            indexed_at: None,
            merged_into: None,
        }
    }

    #[test]
    fn serialize_roundtrip() {
        let entry = sample_entry();
        let content = "Ozzie follows hexagonal architecture.";
        let markdown = serialize(&entry, content);

        assert!(markdown.contains("id: mem_cosmic_asimov"));
        assert!(markdown.contains("type: fact"));
        assert!(markdown.contains("# Project Architecture"));

        let (parsed_entry, parsed_content) = parse(&markdown).unwrap();
        assert_eq!(parsed_entry.id, "mem_cosmic_asimov");
        assert_eq!(parsed_entry.title, "Project Architecture");
        assert_eq!(parsed_entry.memory_type, MemoryType::Fact);
        assert_eq!(parsed_entry.tags, vec!["architecture", "rust"]);
        assert_eq!(parsed_content, content);
    }

    #[test]
    fn filename_generation() {
        let entry = sample_entry();
        let name = filename_for(&entry);
        assert_eq!(name, "project-architecture_imov.md");
    }

    #[test]
    fn slugify_basic() {
        assert_eq!(slugify("Hello World!", 50), "hello-world");
        assert_eq!(slugify("API Key Rotation", 50), "api-key-rotation");
    }

    #[test]
    fn parse_yaml_array_basic() {
        assert_eq!(
            parse_yaml_array("[\"arch\", \"rust\"]"),
            vec!["arch", "rust"]
        );
        assert_eq!(parse_yaml_array("[a, b, c]"), vec!["a", "b", "c"]);
    }

    #[test]
    fn parse_no_frontmatter_fails() {
        assert!(parse("just some text").is_err());
    }

    #[test]
    fn parse_empty_content() {
        let md = "---\nid: test\ntype: fact\n---\n\n# Title\n";
        let (entry, content) = parse(md).unwrap();
        assert_eq!(entry.id, "test");
        assert_eq!(entry.title, "Title");
        assert!(content.is_empty());
    }
}
