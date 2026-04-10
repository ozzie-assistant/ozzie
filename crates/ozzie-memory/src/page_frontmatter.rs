use chrono::{DateTime, Utc};

use ozzie_core::domain::WikiPage;

use crate::MemoryError;

/// Serializes a WikiPage + content into a markdown file with YAML frontmatter.
pub fn serialize(page: &WikiPage, content: &str) -> String {
    let mut fm = String::new();
    fm.push_str("---\n");
    fm.push_str(&format!("id: {}\n", page.id));
    fm.push_str(&format!("slug: {}\n", page.slug));
    if !page.tags.is_empty() {
        let tags: Vec<String> = page.tags.iter().map(|t| format!("\"{t}\"")).collect();
        fm.push_str(&format!("tags: [{}]\n", tags.join(", ")));
    }
    if !page.source_ids.is_empty() {
        let ids: Vec<String> = page.source_ids.iter().map(|s| format!("\"{s}\"")).collect();
        fm.push_str(&format!("source_ids: [{}]\n", ids.join(", ")));
    }
    fm.push_str(&format!("revision: {}\n", page.revision));
    fm.push_str(&format!("created: {}\n", page.created_at.to_rfc3339()));
    fm.push_str(&format!("updated: {}\n", page.updated_at.to_rfc3339()));
    fm.push_str("---\n\n");

    fm.push_str(&format!("# {}\n\n", page.title));
    fm.push_str(content);
    if !content.ends_with('\n') {
        fm.push('\n');
    }
    fm
}

/// Parses a markdown file with YAML frontmatter into (WikiPage, content).
pub fn parse(text: &str) -> Result<(WikiPage, String), MemoryError> {
    let (yaml, body) = split_frontmatter(text)
        .ok_or_else(|| MemoryError::Other("missing YAML frontmatter".into()))?;

    let mut id = String::new();
    let mut slug = String::new();
    let mut tags = Vec::new();
    let mut source_ids = Vec::new();
    let mut revision: u32 = 1;
    let mut created_at = Utc::now();
    let mut updated_at = Utc::now();

    for line in yaml.lines() {
        let line = line.trim();
        if let Some((key, value)) = line.split_once(':') {
            let key = key.trim();
            let value = value.trim();
            match key {
                "id" => id = value.to_string(),
                "slug" => slug = value.to_string(),
                "tags" => tags = parse_yaml_array(value),
                "source_ids" => source_ids = parse_yaml_array(value),
                "revision" => revision = value.parse().unwrap_or(1),
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

    let page = WikiPage {
        id,
        title,
        slug,
        tags,
        source_ids,
        created_at,
        updated_at,
        revision,
    };

    Ok((page, content))
}

/// Generates a filename for a wiki page: `{slug}.md`.
pub fn filename_for(page: &WikiPage) -> String {
    format!("{}.md", page.slug)
}

// ---- Internal helpers (shared logic with frontmatter.rs) ----

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

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_page() -> WikiPage {
        WikiPage {
            id: "page_rust-tooling".to_string(),
            title: "Rust Tooling Preferences".to_string(),
            slug: "rust-tooling".to_string(),
            tags: vec!["rust".to_string(), "tooling".to_string()],
            source_ids: vec!["mem_cosmic_asimov".to_string(), "mem_stellar_clarke".to_string()],
            created_at: DateTime::parse_from_rfc3339("2026-04-10T12:00:00Z")
                .unwrap()
                .with_timezone(&Utc),
            updated_at: DateTime::parse_from_rfc3339("2026-04-10T18:00:00Z")
                .unwrap()
                .with_timezone(&Utc),
            revision: 2,
        }
    }

    #[test]
    fn serialize_roundtrip() {
        let page = sample_page();
        let content = "Prefers clippy with zero warnings.";
        let markdown = serialize(&page, content);

        assert!(markdown.contains("id: page_rust-tooling"));
        assert!(markdown.contains("slug: rust-tooling"));
        assert!(markdown.contains("revision: 2"));
        assert!(markdown.contains("# Rust Tooling Preferences"));
        assert!(markdown.contains("Prefers clippy with zero warnings."));

        let (parsed, parsed_content) = parse(&markdown).unwrap();
        assert_eq!(parsed.id, "page_rust-tooling");
        assert_eq!(parsed.title, "Rust Tooling Preferences");
        assert_eq!(parsed.slug, "rust-tooling");
        assert_eq!(parsed.tags, vec!["rust", "tooling"]);
        assert_eq!(
            parsed.source_ids,
            vec!["mem_cosmic_asimov", "mem_stellar_clarke"]
        );
        assert_eq!(parsed.revision, 2);
        assert_eq!(parsed_content, content);
    }

    #[test]
    fn filename_uses_slug() {
        let page = sample_page();
        assert_eq!(filename_for(&page), "rust-tooling.md");
    }

    #[test]
    fn parse_no_frontmatter_fails() {
        assert!(parse("just some text").is_err());
    }

    #[test]
    fn parse_empty_content() {
        let md = "---\nid: page_test\nslug: test\nrevision: 1\n---\n\n# Title\n";
        let (page, content) = parse(md).unwrap();
        assert_eq!(page.id, "page_test");
        assert_eq!(page.title, "Title");
        assert!(content.is_empty());
    }

    #[test]
    fn parse_minimal_frontmatter() {
        let md = "---\nid: page_min\nslug: min\n---\n\n# Minimal\n\nSome text.";
        let (page, content) = parse(md).unwrap();
        assert_eq!(page.slug, "min");
        assert_eq!(page.revision, 1);
        assert!(page.tags.is_empty());
        assert!(page.source_ids.is_empty());
        assert_eq!(content, "Some text.");
    }
}
