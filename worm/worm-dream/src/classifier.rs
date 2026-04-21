use crate::types::{DreamExtraction, DreamRecord};
use tracing::warn;

/// System prompt for the dream classifier LLM call.
pub const CLASSIFIER_SYSTEM_PROMPT: &str = r#"You are a knowledge extractor. Analyze a conversation between a user and their AI assistant Ozzie. Extract ONLY lasting, reusable knowledge — ignore transient task details, greetings, and ephemeral requests.

Classify each piece into exactly one category:
- **profile**: Identity-level facts about the user (role, preferences, communication style, expertise, personal details). Stable, rarely changes. Write concise sentences.
- **memory**: Contextual knowledge (project details, technical decisions, procedures, tools, architecture choices). Useful for future conversations. Structured with title, content, type, and tags.

Rules:
- Do NOT repeat previously extracted knowledge (listed below if any).
- Only extract NEW information from the new messages.
- If nothing worth extracting, respond with empty arrays.
- Respond with JSON only, no markdown fences.

Response format:
{"profile": ["sentence 1", ...], "memory": [{"title": "...", "content": "...", "type": "preference|fact|procedure|context", "tags": ["..."]}]}"#;

/// Builds the full system prompt, including previous extraction context.
pub fn build_system_prompt(previous: Option<&DreamRecord>) -> String {
    let mut prompt = CLASSIFIER_SYSTEM_PROMPT.to_string();

    if let Some(record) = previous {
        let has_previous =
            !record.profile_entries.is_empty() || !record.memory_ids.is_empty();

        if has_previous {
            prompt.push_str("\n\nPreviously extracted from this session (do NOT re-extract):");

            if !record.profile_entries.is_empty() {
                prompt.push_str("\nProfile:");
                for entry in &record.profile_entries {
                    prompt.push_str("\n- ");
                    prompt.push_str(entry);
                }
            }

            if !record.memory_ids.is_empty() {
                prompt.push_str("\nMemory IDs already created: ");
                prompt.push_str(&record.memory_ids.join(", "));
            }
        }
    }

    prompt
}

/// Formats conversation messages into a text block for the LLM.
pub fn format_messages(messages: &[Message]) -> String {
    let mut out = String::with_capacity(messages.len() * 100);
    for msg in messages {
        if msg.content.trim().is_empty() {
            continue;
        }
        if let Some(ts) = &msg.ts {
            out.push_str(&format!("[{}] ", ts.format("%H:%M")));
        }
        out.push_str(&msg.role);
        out.push_str(": ");
        out.push_str(&msg.content);
        out.push('\n');
    }
    out
}

/// Parses a raw LLM response into a DreamExtraction.
/// Tolerant of markdown fences and garbage — returns empty on failure.
pub fn parse_extraction(raw: &str) -> DreamExtraction {
    if let Ok(extraction) = serde_json::from_str::<DreamExtraction>(raw) {
        return extraction;
    }

    let stripped = extract_json(raw);
    if let Ok(extraction) = serde_json::from_str::<DreamExtraction>(stripped) {
        return extraction;
    }

    warn!(
        raw_len = raw.len(),
        "failed to parse dream extraction, returning empty"
    );
    DreamExtraction::default()
}

/// Extracts JSON content from a string that may contain markdown code fences.
pub fn extract_json(s: &str) -> &str {
    let s = s.trim();

    if let Some(start) = s.find("```json") {
        let after = &s[start + 7..];
        if let Some(end) = after.find("```") {
            return after[..end].trim();
        }
    }
    if let Some(start) = s.find("```") {
        let after = &s[start + 3..];
        if let Some(end) = after.find("```") {
            return after[..end].trim();
        }
    }

    if let (Some(start), Some(end)) = (s.find('{'), s.rfind('}')) {
        return &s[start..=end];
    }

    s
}

/// Extracts a JSON array from a string that may contain markdown fences.
pub fn extract_json_array(s: &str) -> &str {
    let s = s.trim();
    if let Some(start) = s.find("```json") {
        let after = &s[start + 7..];
        if let Some(end) = after.find("```") {
            return after[..end].trim();
        }
    }
    if let Some(start) = s.find("```") {
        let after = &s[start + 3..];
        if let Some(end) = after.find("```") {
            return after[..end].trim();
        }
    }
    if let (Some(start), Some(end)) = (s.find('['), s.rfind(']')) {
        return &s[start..=end];
    }
    s
}

/// Message type for dream classification input.
/// Mirrors the domain Message but only needs role, content, and optional timestamp.
pub struct Message {
    pub role: String,
    pub content: String,
    pub ts: Option<chrono::DateTime<chrono::Utc>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_clean_json() {
        let json = r#"{"profile": ["user is a Rust developer"], "memory": [{"title": "Ozzie architecture", "content": "Uses hexagonal DDD", "type": "fact", "tags": ["architecture"]}]}"#;
        let extraction = parse_extraction(json);
        assert_eq!(extraction.profile.len(), 1);
        assert_eq!(extraction.memory.len(), 1);
        assert_eq!(extraction.memory[0].title, "Ozzie architecture");
    }

    #[test]
    fn parse_with_markdown_fences() {
        let raw = "Here's the result:\n```json\n{\"profile\": [\"speaks French\"], \"memory\": []}\n```\n";
        let extraction = parse_extraction(raw);
        assert_eq!(extraction.profile, vec!["speaks French"]);
    }

    #[test]
    fn parse_garbage_returns_empty() {
        let extraction = parse_extraction("this is not json at all");
        assert!(extraction.profile.is_empty());
        assert!(extraction.memory.is_empty());
    }

    #[test]
    fn build_system_prompt_no_previous() {
        let prompt = build_system_prompt(None);
        assert!(!prompt.contains("Previously extracted"));
    }

    #[test]
    fn build_system_prompt_with_previous() {
        let record = DreamRecord {
            session_id: "sess_test".to_string(),
            consolidated_up_to: 5,
            profile_entries: vec!["user speaks French".to_string()],
            memory_ids: vec!["mem_abc".to_string()],
            updated_at: chrono::Utc::now(),
        };
        let prompt = build_system_prompt(Some(&record));
        assert!(prompt.contains("Previously extracted"));
        assert!(prompt.contains("user speaks French"));
        assert!(prompt.contains("mem_abc"));
    }

    #[test]
    fn format_messages_skips_empty() {
        let messages = vec![
            Message {
                role: "user".to_string(),
                content: "hello".to_string(),
                ts: None,
            },
            Message {
                role: "assistant".to_string(),
                content: "   ".to_string(),
                ts: None,
            },
        ];
        let formatted = format_messages(&messages);
        assert!(formatted.contains("hello"));
        assert!(!formatted.contains("   \n"));
    }
}
