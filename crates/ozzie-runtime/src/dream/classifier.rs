use ozzie_core::domain::{DreamExtraction, DreamRecord, Message};
use ozzie_llm::{ChatMessage, ChatRole, Provider};
use tracing::warn;

const SYSTEM_PROMPT: &str = r#"You are a knowledge extractor. Analyze a conversation between a user and their AI assistant Ozzie. Extract ONLY lasting, reusable knowledge — ignore transient task details, greetings, and ephemeral requests.

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

/// Classifies a session's messages into profile and memory extractions.
pub async fn classify_session(
    provider: &dyn Provider,
    messages: &[Message],
    previous: Option<&DreamRecord>,
) -> anyhow::Result<DreamExtraction> {
    if messages.is_empty() {
        return Ok(DreamExtraction::default());
    }

    let system = build_system_prompt(previous);
    let user_content = format_messages(messages);

    let chat_messages = vec![
        ChatMessage {
            role: ChatRole::System,
            content: system,
            tool_calls: Vec::new(),
            tool_call_id: None,
        },
        ChatMessage {
            role: ChatRole::User,
            content: user_content,
            tool_calls: Vec::new(),
            tool_call_id: None,
        },
    ];

    let response = provider.chat(&chat_messages, &[]).await?;
    parse_extraction(&response.content)
}

fn build_system_prompt(previous: Option<&DreamRecord>) -> String {
    let mut prompt = SYSTEM_PROMPT.to_string();

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

fn format_messages(messages: &[Message]) -> String {
    let mut out = String::with_capacity(messages.len() * 100);
    for msg in messages {
        // Skip empty or system-only messages
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

fn parse_extraction(raw: &str) -> anyhow::Result<DreamExtraction> {
    // Try direct parse first
    if let Ok(extraction) = serde_json::from_str::<DreamExtraction>(raw) {
        return Ok(extraction);
    }

    // Strip markdown fences if present
    let stripped = extract_json(raw);
    if let Ok(extraction) = serde_json::from_str::<DreamExtraction>(stripped) {
        return Ok(extraction);
    }

    warn!(
        raw_len = raw.len(),
        "failed to parse dream extraction, returning empty"
    );
    Ok(DreamExtraction::default())
}

/// Extracts JSON content from a string that may contain markdown code fences.
fn extract_json(s: &str) -> &str {
    let s = s.trim();

    // Try to find JSON between code fences
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

    // Try to find a JSON object
    if let (Some(start), Some(end)) = (s.find('{'), s.rfind('}')) {
        return &s[start..=end];
    }

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_clean_json() {
        let json = r#"{"profile": ["user is a Rust developer"], "memory": [{"title": "Ozzie architecture", "content": "Uses hexagonal DDD", "type": "fact", "tags": ["architecture"]}]}"#;
        let extraction = parse_extraction(json).unwrap();
        assert_eq!(extraction.profile.len(), 1);
        assert_eq!(extraction.memory.len(), 1);
        assert_eq!(extraction.memory[0].title, "Ozzie architecture");
    }

    #[test]
    fn parse_with_markdown_fences() {
        let raw = "Here's the result:\n```json\n{\"profile\": [\"speaks French\"], \"memory\": []}\n```\n";
        let extraction = parse_extraction(raw).unwrap();
        assert_eq!(extraction.profile, vec!["speaks French"]);
        assert!(extraction.memory.is_empty());
    }

    #[test]
    fn parse_garbage_returns_empty() {
        let extraction = parse_extraction("this is not json at all").unwrap();
        assert!(extraction.profile.is_empty());
        assert!(extraction.memory.is_empty());
    }

    #[test]
    fn parse_empty_arrays() {
        let json = r#"{"profile": [], "memory": []}"#;
        let extraction = parse_extraction(json).unwrap();
        assert!(extraction.profile.is_empty());
        assert!(extraction.memory.is_empty());
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
            Message {
                role: "user".to_string(),
                content: "goodbye".to_string(),
                ts: None,
            },
        ];
        let formatted = format_messages(&messages);
        assert!(formatted.contains("hello"));
        assert!(!formatted.contains("   "));
        assert!(formatted.contains("goodbye"));
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
}
