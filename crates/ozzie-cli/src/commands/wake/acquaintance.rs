use std::io::{self, BufRead, Write as IoWrite};
use std::path::Path;
use std::sync::Arc;

use ozzie_core::config;
use ozzie_core::profile::{self, UserProfile};
use ozzie_core::prompt::load_persona;
use ozzie_llm::{ChatMessage, ChatRole, Provider};
use ozzie_utils::i18n;

/// Runs the "getting to know each other" onboarding step.
///
/// Asks structured questions, calls the LLM with the Ozzie persona,
/// then classifies answers into profile (identity) vs memory (context).
pub async fn run(ozzie_path: &Path, language: Option<&str>) -> anyhow::Result<()> {
    println!();
    println!("{}", i18n::t("wizard.acquaintance.title"));
    println!("==========================");
    println!();

    // Load config and build provider
    let config_path = ozzie_path.join("config.jsonc");
    let cfg = config::load(&config_path)
        .map_err(|e| anyhow::anyhow!("failed to load config: {e}"))?;

    let default_name = &cfg.models.default;
    let provider_cfg = cfg
        .models
        .providers
        .get(default_name)
        .ok_or_else(|| anyhow::anyhow!("default provider '{default_name}' not found in config"))?;

    let provider = crate::provider_factory::build_provider(default_name, provider_cfg, &crate::provider_factory::OzzieSecretResolver)?;

    // Load persona
    let persona = load_persona(ozzie_path);

    // Structured questions
    let name = ask_question(&i18n::t("wizard.acquaintance.name"))?;
    if name.is_empty() {
        println!("{}", i18n::t("wizard.acquaintance.skip"));
        return Ok(());
    }

    let context = ask_question(&i18n::t("wizard.acquaintance.context"))?;
    let tone = ask_question(&i18n::t("wizard.acquaintance.tone"))?;

    // Build LLM call: Ozzie introduces itself and reformulates
    let user_input = format!(
        "Name: {name}\nContext: {context}\nPreferred tone: {tone}"
    );

    println!();
    println!("{}", i18n::t("wizard.acquaintance.thinking"));

    let lang_instruction = language
        .map(|l| format!("\nAlways respond in the user's language: {l}."))
        .unwrap_or_default();
    let response = call_introduction(&provider, &persona, &user_input, &lang_instruction).await?;

    println!();
    println!("{response}");
    println!();

    // Confirm or adjust
    let adjust = ask_question(&i18n::t("wizard.acquaintance.confirm"))?;
    let final_response = if adjust.is_empty()
        || adjust.to_lowercase().starts_with('y')
        || adjust.to_lowercase().starts_with('o')
    {
        response
    } else {
        println!();
        println!("{}", i18n::t("wizard.acquaintance.adjusting"));
        let followup = call_adjustment(&provider, &persona, &user_input, &response, &adjust, &lang_instruction).await?;
        println!();
        println!("{followup}");
        println!();
        followup
    };

    // Classify: ask LLM to split info into profile vs memory entries
    let classified = call_classify(&provider, &name, &context, &tone, &final_response).await?;

    // Build profile
    let mut profile = UserProfile::new(name.clone(), Vec::new());
    profile.tone = if tone.is_empty() { None } else { Some(tone) };
    profile.language = language
        .map(String::from)
        .or_else(|| cfg.agent.preferred_language.clone());

    for entry in &classified.profile_entries {
        profile.whoami.push(ozzie_core::profile::WhoamiEntry {
            info: entry.clone(),
            created_at: chrono::Utc::now().date_naive(),
            source: ozzie_core::profile::WhoamiSource::Intro,
        });
    }

    // Save profile
    profile::save(ozzie_path, &profile)
        .map_err(|e| anyhow::anyhow!("failed to save profile: {e}"))?;
    println!("{}", i18n::t("wizard.acquaintance.saved"));

    // TODO: save classified.memory_entries to semantic memory once available at CLI level

    Ok(())
}

/// Prompts the user for input on stderr, reads from stdin.
fn ask_question(question: &str) -> io::Result<String> {
    eprint!("{question} ");
    io::stderr().flush()?;
    let mut input = String::new();
    io::stdin().lock().read_line(&mut input)?;
    Ok(input.trim().to_string())
}

/// Initial introduction call: Ozzie presents itself and reformulates.
async fn call_introduction(
    provider: &Arc<dyn Provider>,
    persona: &str,
    user_input: &str,
    lang_instruction: &str,
) -> anyhow::Result<String> {
    let system = format!(
        "{persona}\n\n\
        ## Context\n\
        This is the first interaction with your user during onboarding.\n\
        Introduce yourself briefly. Reformulate what you understood about the user.\n\
        Keep it short and natural — match the tone they asked for.\n\
        End by asking if this feels right or if they'd like to adjust anything.\
        {lang_instruction}"
    );

    let messages = vec![
        ChatMessage::text(ChatRole::System, system),
        ChatMessage::text(ChatRole::User, user_input),
    ];

    let response = provider.chat(&messages, &[]).await?;
    Ok(response.content)
}

/// Follow-up adjustment call.
async fn call_adjustment(
    provider: &Arc<dyn Provider>,
    persona: &str,
    user_input: &str,
    previous_response: &str,
    adjustment: &str,
    lang_instruction: &str,
) -> anyhow::Result<String> {
    let system = format!(
        "{persona}\n\n\
        ## Context\n\
        Onboarding adjustment. The user wants to tweak your introduction.\n\
        Apply their feedback and present the updated version. Stay concise.\
        {lang_instruction}"
    );

    let messages = vec![
        ChatMessage::text(ChatRole::System, system),
        ChatMessage::text(ChatRole::User, user_input),
        ChatMessage::text(ChatRole::Assistant, previous_response),
        ChatMessage::text(ChatRole::User, adjustment),
    ];

    let response = provider.chat(&messages, &[]).await?;
    Ok(response.content)
}

/// Classification result from the LLM.
#[derive(Debug, Default)]
struct ClassifiedInfo {
    profile_entries: Vec<String>,
    #[allow(dead_code)]
    memory_entries: Vec<String>,
}

/// Asks the LLM to classify user info into profile (identity) vs memory (contextual).
async fn call_classify(
    provider: &Arc<dyn Provider>,
    name: &str,
    context: &str,
    tone: &str,
    ozzie_response: &str,
) -> anyhow::Result<ClassifiedInfo> {
    let system = r#"You are a knowledge synthesizer. Given raw information about a user collected during onboarding, your job is to:

1. **Synthesize** — don't copy raw answers verbatim. Extract the essential meaning and compress into concise, self-contained statements.
2. **Classify** each synthesized fact into exactly one category:
   - **profile**: Identity-level facts that help personalize ALL future interactions (who they are, how they communicate, their role, core values). These are stable and rarely change. Keep each entry to one clear sentence.
   - **memory**: Contextual knowledge useful for specific tasks (current projects, tools, technical stack). These change over time.
3. **Discard** noise — greetings, filler, redundant info.

Examples of good synthesis:
- Raw: "I'm a fullstack dev working on AI agents, I work alone" → Profile: "Solo fullstack developer specializing in AI agents"
- Raw: "casual, direct, en français, pas de bullshit" → Profile: "Prefers direct, casual communication in French — no filler"

Respond in JSON only:
{"profile": ["synthesized sentence 1", "synthesized sentence 2"], "memory": ["synthesized sentence 1"]}"#;

    let user_msg = format!(
        "User name: {name}\nUser context: {context}\nPreferred tone: {tone}\n\nOzzie's understanding:\n{ozzie_response}"
    );

    let messages = vec![
        ChatMessage::text(ChatRole::System, system),
        ChatMessage::text(ChatRole::User, user_msg),
    ];

    let response = provider.chat(&messages, &[]).await?;

    // Parse JSON response — be lenient with LLM output
    parse_classification(&response.content)
}

/// Parses the LLM classification JSON response.
fn parse_classification(content: &str) -> anyhow::Result<ClassifiedInfo> {
    // Try to find JSON in the response (LLM might wrap it in markdown)
    let json_str = extract_json(content).unwrap_or(content);

    let parsed: serde_json::Value = serde_json::from_str(json_str)
        .unwrap_or_else(|_| serde_json::json!({"profile": [], "memory": []}));

    let profile_entries = parsed
        .get("profile")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    let memory_entries = parsed
        .get("memory")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    Ok(ClassifiedInfo {
        profile_entries,
        memory_entries,
    })
}

/// Extracts the first JSON object from a string (handles ```json ... ``` wrappers).
fn extract_json(s: &str) -> Option<&str> {
    // Try to find a JSON block in markdown code fences
    if let Some(start) = s.find("```json") {
        let content_start = start + 7;
        if let Some(end) = s[content_start..].find("```") {
            return Some(s[content_start..content_start + end].trim());
        }
    }
    if let Some(start) = s.find("```") {
        let content_start = start + 3;
        // Skip optional language identifier on same line
        let line_end = s[content_start..]
            .find('\n')
            .map(|i| content_start + i + 1)
            .unwrap_or(content_start);
        if let Some(end) = s[line_end..].find("```") {
            return Some(s[line_end..line_end + end].trim());
        }
    }
    // Try to find raw JSON object
    let start = s.find('{')?;
    let end = s.rfind('}')?;
    if end > start {
        Some(&s[start..=end])
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_json_from_markdown() {
        let input = "Here is the result:\n```json\n{\"profile\": [\"dev\"], \"memory\": []}\n```";
        let json = extract_json(input).unwrap();
        assert!(json.starts_with('{'));
        let parsed: serde_json::Value = serde_json::from_str(json).unwrap();
        assert_eq!(parsed["profile"][0], "dev");
    }

    #[test]
    fn extract_json_raw() {
        let input = "{\"profile\": [\"dev\"], \"memory\": [\"uses rust\"]}";
        let json = extract_json(input).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(json).unwrap();
        assert_eq!(parsed["memory"][0], "uses rust");
    }

    #[test]
    fn parse_classification_lenient() {
        let result = parse_classification("not json at all").unwrap();
        assert!(result.profile_entries.is_empty());
        assert!(result.memory_entries.is_empty());
    }

    #[test]
    fn parse_classification_valid() {
        let input = r#"{"profile": ["Solo founder", "Prefers direct tone"], "memory": ["Works on Ozzie"]}"#;
        let result = parse_classification(input).unwrap();
        assert_eq!(result.profile_entries.len(), 2);
        assert_eq!(result.memory_entries.len(), 1);
    }
}
