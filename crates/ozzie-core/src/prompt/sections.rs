use std::collections::HashMap;

/// Metadata for a single memory entry in the prompt.
#[derive(Debug, Clone)]
pub struct MemoryInfo {
    /// Memory type (preference, fact, procedure, context).
    pub memory_type: String,
    /// Human-readable title.
    pub title: String,
    /// Memory content.
    pub content: String,
}

/// Builds the "## Relevant Memories" block.
/// If `content_max > 0`, each memory's content is truncated to that length.
pub fn memory_section(memories: &[MemoryInfo], content_max: usize) -> String {
    if memories.is_empty() {
        return String::new();
    }

    let mut lines = vec!["## Relevant Memories\n".to_string()];
    for m in memories {
        let content = if content_max > 0 && m.content.len() > content_max {
            format!("{}...", truncate_utf8(&m.content, content_max))
        } else {
            m.content.clone()
        };
        lines.push(format!("- **[{}] {}**: {}", m.memory_type, m.title, content));
    }

    lines.join("\n")
}

/// Truncates a string to at most `max_bytes` bytes, respecting UTF-8 char boundaries.
pub fn truncate_utf8(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    // Find the last valid char boundary at or before max_bytes
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

/// Builds the tools section for the system prompt.
pub fn tool_section(
    active_names: &[String],
    all_descriptions: &HashMap<String, String>,
    compact: bool,
) -> String {
    if active_names.is_empty() {
        return String::new();
    }

    let mut lines = vec!["## Available Tools".to_string()];

    if compact {
        let names: Vec<&str> = active_names.iter().map(|s| s.as_str()).collect();
        lines.push(format!("Tools: {}", names.join(", ")));
    } else {
        for name in active_names {
            let desc = all_descriptions
                .get(name)
                .map(|s| s.as_str())
                .unwrap_or("(no description)");
            lines.push(format!("- **{name}**: {desc}"));
        }
    }

    // Show inactive tools for activation hint
    let inactive: Vec<&String> = all_descriptions
        .keys()
        .filter(|k| !active_names.contains(k))
        .collect();

    if !inactive.is_empty() {
        let names: Vec<&str> = inactive.iter().map(|s| s.as_str()).collect();
        lines.push(format!(
            "\nAdditional tools available via `activate`: {}",
            names.join(", ")
        ));
    }

    lines.join("\n")
}

/// Builds the session context section.
pub fn session_section(
    root_dir: Option<&str>,
    language: Option<&str>,
    title: Option<&str>,
    message_count: usize,
) -> String {
    let mut parts = Vec::new();

    if let Some(dir) = root_dir {
        parts.push(format!("Working directory: {dir}"));
    }
    if let Some(lang) = language {
        parts.push(format!("Language: {lang}"));
    }
    if let Some(t) = title {
        parts.push(format!("Conversation: {t}"));
    }
    if message_count > 0 {
        parts.push(format!("Messages in history: {message_count}"));
    }

    if parts.is_empty() {
        return String::new();
    }

    format!("## Conversation Context\n{}", parts.join("\n"))
}

/// Builds the skills section.
pub fn skill_section(skills: &HashMap<String, String>, compact: bool) -> String {
    if skills.is_empty() {
        return String::new();
    }

    let mut lines = vec!["## Available Skills".to_string()];

    if compact {
        let names: Vec<&str> = skills.keys().map(|s| s.as_str()).collect();
        lines.push(format!("Skills: {}", names.join(", ")));
    } else {
        let mut sorted: Vec<(&String, &String)> = skills.iter().collect();
        sorted.sort_by_key(|(k, _)| *k);
        for (name, desc) in sorted {
            lines.push(format!("- **{name}**: {desc}"));
        }
    }

    lines.join("\n")
}

/// Builds the user profile section for the system prompt.
///
/// Renders the user's identity-level knowledge compactly.
/// Always loaded — this is Ozzie's "working memory" about the user.
pub fn user_profile_section(profile: &crate::profile::UserProfile) -> String {
    let mut lines = vec![format!(
        "This is what you know about the user. Use this to personalize responses.\n\
         When asked \"what do you know about me?\", answer ONLY from this section and from memories — never list tools, capabilities, or system context.\n\n\
         Name: {}",
        profile.name
    )];

    if let Some(ref tone) = profile.tone {
        lines.push(format!("Tone: {tone}"));
    }
    if let Some(ref lang) = profile.language {
        lines.push(format!("Language: {lang}"));
    }

    if !profile.whoami.is_empty() {
        lines.push(String::new());
        for entry in &profile.whoami {
            lines.push(format!("- {}", entry.info));
        }
    }

    lines.join("\n")
}

/// Builds the actor info section (for multi-actor setups).
pub fn actor_section(actors: &[super::super::actors::ActorInfo]) -> String {
    if actors.is_empty() {
        return String::new();
    }

    let mut lines = vec!["## Available Actors".to_string()];
    for actor in actors {
        let mut parts = vec![actor.provider_name.clone()];
        if !actor.tags.is_empty() {
            parts.push(format!("tags: {}", actor.tags.join(", ")));
        }
        if !actor.capabilities.is_empty() {
            let caps: Vec<String> = actor.capabilities.iter().map(|c| c.to_string()).collect();
            parts.push(format!("caps: {}", caps.join(", ")));
        }
        lines.push(format!("- {}", parts.join(" | ")));
    }

    lines.join("\n")
}

/// Builds the active project section for the system prompt.
pub fn project_section(name: &str, description: &str, path: &str, body: &str) -> String {
    let mut lines = vec![
        "## Active Project".to_string(),
        format!("Name: {name}"),
    ];

    if !description.is_empty() {
        lines.push(format!("Description: {description}"));
    }

    lines.push(format!("Path: {path}"));

    if !body.is_empty() {
        lines.push(String::new());
        lines.push(body.to_string());
    }

    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_section_full() {
        let mut descs = HashMap::new();
        descs.insert("execute".to_string(), "Run shell commands".to_string());
        descs.insert("file_read".to_string(), "Read files".to_string());
        descs.insert("web_fetch".to_string(), "Fetch URLs".to_string());

        let active = vec!["execute".to_string(), "file_read".to_string()];
        let section = tool_section(&active, &descs, false);

        assert!(section.contains("**execute**"));
        assert!(section.contains("**file_read**"));
        assert!(section.contains("activate"));
        assert!(section.contains("web_fetch"));
    }

    #[test]
    fn tool_section_compact() {
        let mut descs = HashMap::new();
        descs.insert("execute".to_string(), "Run commands".to_string());

        let active = vec!["execute".to_string()];
        let section = tool_section(&active, &descs, true);

        assert!(section.contains("Tools: execute"));
    }

    #[test]
    fn session_section_full() {
        let section = session_section(Some("/home/user"), Some("fr"), Some("Debug"), 42);
        assert!(section.contains("/home/user"));
        assert!(section.contains("fr"));
        assert!(section.contains("42"));
    }

    #[test]
    fn session_section_empty() {
        let section = session_section(None, None, None, 0);
        assert!(section.is_empty());
    }

    #[test]
    fn memory_section_basic() {
        let memories = vec![
            MemoryInfo {
                memory_type: "fact".to_string(),
                title: "Project Name".to_string(),
                content: "The project is called Ozzie.".to_string(),
            },
            MemoryInfo {
                memory_type: "preference".to_string(),
                title: "Code Style".to_string(),
                content: "User prefers explicit error handling.".to_string(),
            },
        ];
        let section = memory_section(&memories, 0);
        assert!(section.contains("## Relevant Memories"));
        assert!(section.contains("**[fact] Project Name**"));
        assert!(section.contains("The project is called Ozzie."));
        assert!(section.contains("**[preference] Code Style**"));
    }

    #[test]
    fn memory_section_empty() {
        let section = memory_section(&[], 0);
        assert!(section.is_empty());
    }

    #[test]
    fn memory_section_truncated() {
        let memories = vec![MemoryInfo {
            memory_type: "procedure".to_string(),
            title: "Long Content".to_string(),
            content: "a".repeat(200),
        }];
        let section = memory_section(&memories, 50);
        assert!(section.contains("..."));
        // Content should be truncated, not the full 200 chars
        assert!(!section.contains(&"a".repeat(200)));
    }

    #[test]
    fn truncate_utf8_ascii() {
        assert_eq!(truncate_utf8("hello world", 5), "hello");
        assert_eq!(truncate_utf8("hello", 10), "hello");
    }

    #[test]
    fn user_profile_section_full() {
        let profile = crate::profile::UserProfile {
            name: "Michael".to_string(),
            tone: Some("casual, direct".to_string()),
            language: Some("fr".to_string()),
            whoami: vec![
                crate::profile::WhoamiEntry {
                    info: "Fullstack dev, AI/agents".to_string(),
                    created_at: chrono::NaiveDate::from_ymd_opt(2026, 3, 28).unwrap(),
                    source: crate::profile::WhoamiSource::Intro,
                },
            ],
            created_at: chrono::NaiveDate::from_ymd_opt(2026, 3, 28).unwrap(),
            updated_at: chrono::NaiveDate::from_ymd_opt(2026, 3, 28).unwrap(),
        };
        let section = user_profile_section(&profile);
        assert!(section.contains("Name: Michael"));
        assert!(section.contains("Tone: casual, direct"));
        assert!(section.contains("Language: fr"));
        assert!(section.contains("- Fullstack dev, AI/agents"));
    }

    #[test]
    fn user_profile_section_minimal() {
        let profile = crate::profile::UserProfile::new("Alice".to_string(), Vec::new());
        let section = user_profile_section(&profile);
        assert!(section.contains("Name: Alice"));
        assert!(!section.contains("Tone:"));
    }

    #[test]
    fn truncate_utf8_multibyte() {
        // "é" is 2 bytes in UTF-8
        let s = "café";
        // "caf" = 3 bytes, "é" = 2 bytes, total = 5
        assert_eq!(truncate_utf8(s, 4), "caf"); // Can't split é, so stops at 3
        assert_eq!(truncate_utf8(s, 5), "café");
    }
}
