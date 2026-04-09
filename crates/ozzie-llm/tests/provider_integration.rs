//! Provider integration tests — generic suite that runs against any live LLM API.
//!
//! Each provider is gated behind env vars. If the var is not set, tests are skipped.
//! Env vars are loaded from `.env.test` at repo root (via dotenvy, silent if missing).
//!
//! | Provider   | Env vars                            | Notes                            |
//! |------------|-------------------------------------|----------------------------------|
//! | llama.cpp  | `LLAMA_URLS`                        | Comma-separated URLs, no model   |
//! | OpenAI     | `OPENAI_API_KEY`, `OPENAI_MODELS`   | Comma-separated models           |
//! | Anthropic  | `ANTHROPIC_API_KEY`, `ANTHROPIC_MODELS` |                              |
//! | Gemini     | `GOOGLE_API_KEY`, `GEMINI_MODELS`   |                                  |
//! | Ollama     | `OLLAMA_URL`, `OLLAMA_MODELS`       |                                  |
//! | Mistral    | `MISTRAL_API_KEY`, `MISTRAL_MODELS` |                                  |
//! | Groq       | `GROQ_API_KEY`, `GROQ_MODELS`       |                                  |
//! | xAI        | `XAI_API_KEY`, `XAI_MODELS`         |                                  |
//!
//! Run: `cargo test -p ozzie-llm --test provider_integration`

use std::sync::{Arc, Once};

use futures_util::StreamExt;
use ozzie_llm::{AuthKind, ChatMessage, ChatRole, Provider, ResolvedAuth, ToolDefinition};
use schemars::JsonSchema;
use serde::Deserialize;

// ============================================================
// .env.test loader
// ============================================================

static INIT_ENV: Once = Once::new();

fn load_env() {
    INIT_ENV.call_once(|| {
        let _ = dotenvy::from_filename_override(".env.test");
    });
}

// ============================================================
// Test tool definitions
// ============================================================

/// Simple tool: single required string arg.
#[derive(Deserialize, JsonSchema)]
#[allow(dead_code)]
struct GetWeatherArgs {
    /// City name to get weather for.
    city: String,
}

/// Tool with optional args (produces `type: ["string", "null"]` in schemars).
#[derive(Deserialize, JsonSchema)]
#[allow(dead_code)]
struct SearchArgs {
    /// Search query.
    query: String,
    /// Maximum number of results.
    #[serde(default)]
    limit: Option<u32>,
    /// Language filter.
    #[serde(default)]
    language: Option<String>,
}

/// Tool with no args.
#[derive(Deserialize, JsonSchema)]
#[allow(dead_code)]
struct NoArgsInput {}

fn schema_for<T: JsonSchema>() -> schemars::schema::RootSchema {
    let settings = schemars::r#gen::SchemaSettings::draft07().with(|s| {
        s.inline_subschemas = true;
    });
    settings.into_generator().into_root_schema_for::<T>()
}

fn tool_get_weather() -> ToolDefinition {
    ToolDefinition {
        name: "get_weather".to_string(),
        description: "Get the current weather for a city.".to_string(),
        parameters: schema_for::<GetWeatherArgs>(),
    }
}

fn tool_search() -> ToolDefinition {
    ToolDefinition {
        name: "search".to_string(),
        description: "Search the web for information.".to_string(),
        parameters: schema_for::<SearchArgs>(),
    }
}

fn tool_no_args() -> ToolDefinition {
    ToolDefinition {
        name: "get_time".to_string(),
        description: "Get the current time.".to_string(),
        parameters: schema_for::<NoArgsInput>(),
    }
}

fn user_msg(content: &str) -> ChatMessage {
    ChatMessage::text(ChatRole::User, content)
}

fn system_msg(content: &str) -> ChatMessage {
    ChatMessage::text(ChatRole::System, content)
}

// ============================================================
// Provider factory
// ============================================================

struct ProviderConfig {
    label: String,
    provider: Arc<dyn Provider>,
}

/// Parse comma-separated values, trimming whitespace and ignoring empties.
fn parse_csv(val: &str) -> Vec<String> {
    val.split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

fn try_llama() -> Vec<ProviderConfig> {
    let urls = match std::env::var("LLAMA_URLS") {
        Ok(v) => parse_csv(&v),
        Err(_) => return vec![],
    };
    let auth = ResolvedAuth {
        kind: AuthKind::ApiKey,
        value: "no-key".to_string(),
    };
    urls.into_iter()
        .enumerate()
        .map(|(i, url)| {
            let provider = Arc::new(ozzie_llm::providers::OpenAIProvider::new(
                auth.clone(),
                None,
                Some(&url),
                None,
                None,
                Some("llama.cpp"),
            ));
            ProviderConfig {
                label: format!("llama.cpp#{}", i + 1),
                provider,
            }
        })
        .collect()
}

fn try_openai() -> Vec<ProviderConfig> {
    let key = match std::env::var("OPENAI_API_KEY") {
        Ok(v) => v,
        Err(_) => return vec![],
    };
    let models = std::env::var("OPENAI_MODELS")
        .map(|v| parse_csv(&v))
        .unwrap_or_else(|_| vec!["gpt-4o-mini".to_string()]);
    let auth = ResolvedAuth {
        kind: AuthKind::ApiKey,
        value: key,
    };
    models
        .into_iter()
        .map(|model| {
            let provider = Arc::new(ozzie_llm::providers::OpenAIProvider::new(
                auth.clone(),
                Some(&model),
                None,
                None,
                None,
                None,
            ));
            ProviderConfig {
                label: format!("openai/{model}"),
                provider,
            }
        })
        .collect()
}

fn try_anthropic() -> Vec<ProviderConfig> {
    let key = match std::env::var("ANTHROPIC_API_KEY") {
        Ok(v) => v,
        Err(_) => return vec![],
    };
    let models = std::env::var("ANTHROPIC_MODELS")
        .map(|v| parse_csv(&v))
        .unwrap_or_else(|_| vec!["claude-sonnet-4-6".to_string()]);
    let auth = ResolvedAuth {
        kind: AuthKind::ApiKey,
        value: key,
    };
    models
        .into_iter()
        .map(|model| {
            let provider = Arc::new(ozzie_llm::providers::AnthropicProvider::new(
                auth.clone(),
                Some(&model),
                None,
                None,
                None,
            ));
            ProviderConfig {
                label: format!("anthropic/{model}"),
                provider,
            }
        })
        .collect()
}

fn try_gemini() -> Vec<ProviderConfig> {
    let key = match std::env::var("GOOGLE_API_KEY") {
        Ok(v) => v,
        Err(_) => return vec![],
    };
    let models = std::env::var("GEMINI_MODELS")
        .map(|v| parse_csv(&v))
        .unwrap_or_else(|_| vec!["gemini-2.5-flash".to_string()]);
    let auth = ResolvedAuth {
        kind: AuthKind::ApiKey,
        value: key,
    };
    models
        .into_iter()
        .map(|model| {
            let provider = Arc::new(ozzie_llm::providers::GeminiProvider::new(
                auth.clone(),
                Some(&model),
                None,
                None,
                None,
            ));
            ProviderConfig {
                label: format!("gemini/{model}"),
                provider,
            }
        })
        .collect()
}

fn try_ollama() -> Vec<ProviderConfig> {
    let url = match std::env::var("OLLAMA_URL") {
        Ok(v) => v,
        Err(_) => return vec![],
    };
    let models = std::env::var("OLLAMA_MODELS")
        .map(|v| parse_csv(&v))
        .unwrap_or_else(|_| vec!["llama3".to_string()]);
    models
        .into_iter()
        .map(|model| {
            let provider = Arc::new(ozzie_llm::providers::OllamaProvider::new(
                &model,
                Some(url.as_str()),
                None,
            ));
            ProviderConfig {
                label: format!("ollama/{model}"),
                provider,
            }
        })
        .collect()
}

fn try_mistral() -> Vec<ProviderConfig> {
    let key = match std::env::var("MISTRAL_API_KEY") {
        Ok(v) => v,
        Err(_) => return vec![],
    };
    let models = std::env::var("MISTRAL_MODELS")
        .map(|v| parse_csv(&v))
        .unwrap_or_else(|_| vec!["mistral-small-latest".to_string()]);
    let auth = ResolvedAuth {
        kind: AuthKind::ApiKey,
        value: key,
    };
    models
        .into_iter()
        .map(|model| {
            let provider = Arc::new(ozzie_llm::providers::MistralProvider::new(
                auth.clone(),
                Some(&model),
                None,
                None,
                None,
            ));
            ProviderConfig {
                label: format!("mistral/{model}"),
                provider,
            }
        })
        .collect()
}

fn try_groq() -> Vec<ProviderConfig> {
    let key = match std::env::var("GROQ_API_KEY") {
        Ok(v) => v,
        Err(_) => return vec![],
    };
    let models = std::env::var("GROQ_MODELS")
        .map(|v| parse_csv(&v))
        .unwrap_or_else(|_| vec!["qwen/qwen3-32b".to_string()]);
    let auth = ResolvedAuth {
        kind: AuthKind::ApiKey,
        value: key,
    };
    models
        .into_iter()
        .map(|model| {
            let provider = Arc::new(ozzie_llm::providers::GroqProvider::new(
                auth.clone(),
                Some(&model),
                None,
                None,
                None,
            ));
            ProviderConfig {
                label: format!("groq/{model}"),
                provider,
            }
        })
        .collect()
}

fn try_xai() -> Vec<ProviderConfig> {
    let key = match std::env::var("XAI_API_KEY") {
        Ok(v) => v,
        Err(_) => return vec![],
    };
    let models = std::env::var("XAI_MODELS")
        .map(|v| parse_csv(&v))
        .unwrap_or_else(|_| vec!["grok-3-mini-fast".to_string()]);
    let auth = ResolvedAuth {
        kind: AuthKind::ApiKey,
        value: key,
    };
    models
        .into_iter()
        .map(|model| {
            let provider = Arc::new(ozzie_llm::providers::XaiProvider::new(
                auth.clone(),
                Some(&model),
                None,
                None,
                None,
            ));
            ProviderConfig {
                label: format!("xai/{model}"),
                provider,
            }
        })
        .collect()
}

/// Returns all configured providers (one entry per URL/model combination).
fn configured_providers() -> Vec<ProviderConfig> {
    load_env();
    let mut providers = Vec::new();
    providers.extend(try_llama());
    providers.extend(try_openai());
    providers.extend(try_anthropic());
    providers.extend(try_gemini());
    providers.extend(try_ollama());
    providers.extend(try_mistral());
    providers.extend(try_groq());
    providers.extend(try_xai());
    providers
}

macro_rules! skip_if_no_providers {
    ($providers:expr) => {
        if $providers.is_empty() {
            eprintln!("SKIP: no provider configured (set LLAMA_URLS, OPENAI_API_KEY, etc. in .env.test)");
            return;
        }
    };
}

// ============================================================
// Generic test suite — each returns Result, never panics
// ============================================================

fn check(ok: bool, msg: String) -> Result<(), String> {
    if ok { Ok(()) } else { Err(msg) }
}

async fn test_simple_chat(provider: &dyn Provider, label: &str) -> Result<(), String> {
    let messages = vec![user_msg("Respond with exactly: INTEGRATION_OK")];
    let r = provider
        .chat(&messages, &[])
        .await
        .map_err(|e| format!("[{label}] {e}"))?;
    check(!r.content.is_empty(), format!("[{label}] empty response"))?;
    eprintln!("[{label}] simple_chat: OK ({} chars)", r.content.len());
    Ok(())
}

async fn test_chat_with_system_prompt(provider: &dyn Provider, label: &str) -> Result<(), String> {
    let messages = vec![
        system_msg("You are a helpful assistant. Always respond in French."),
        user_msg("Say hello."),
    ];
    let r = provider
        .chat(&messages, &[])
        .await
        .map_err(|e| format!("[{label}] {e}"))?;
    check(!r.content.is_empty(), format!("[{label}] empty response"))?;
    eprintln!("[{label}] system_prompt: OK ({} chars)", r.content.len());
    Ok(())
}

async fn test_tool_call_simple(provider: &dyn Provider, label: &str) -> Result<(), String> {
    let messages = vec![user_msg(
        "What's the weather in Paris? Use the get_weather tool.",
    )];
    let tools = vec![tool_get_weather()];
    let r = provider
        .chat(&messages, &tools)
        .await
        .map_err(|e| format!("[{label}] {e}"))?;
    check(
        !r.tool_calls.is_empty(),
        format!(
            "[{label}] expected tool call, got text: {}",
            &r.content[..r.content.len().min(100)]
        ),
    )?;
    let tc = &r.tool_calls[0];
    check(
        tc.name == "get_weather",
        format!("[{label}] wrong tool: {}", tc.name),
    )?;
    check(
        tc.arguments.get("city").is_some(),
        format!("[{label}] missing 'city' in args: {}", tc.arguments),
    )?;
    eprintln!(
        "[{label}] tool_call_simple: OK (tool={}, args={})",
        tc.name, tc.arguments
    );
    Ok(())
}

async fn test_tool_call_optional_args(provider: &dyn Provider, label: &str) -> Result<(), String> {
    let messages = vec![user_msg(
        "Search for 'Rust programming' using the search tool.",
    )];
    let tools = vec![tool_search()];
    let r = provider
        .chat(&messages, &tools)
        .await
        .map_err(|e| format!("[{label}] {e}"))?;
    check(
        !r.tool_calls.is_empty(),
        format!(
            "[{label}] expected tool call, got text: {}",
            &r.content[..r.content.len().min(100)]
        ),
    )?;
    let tc = &r.tool_calls[0];
    check(
        tc.name == "search",
        format!("[{label}] wrong tool: {}", tc.name),
    )?;
    check(
        tc.arguments.get("query").is_some(),
        format!("[{label}] missing 'query' in args: {}", tc.arguments),
    )?;
    eprintln!("[{label}] tool_call_optional: OK (args={})", tc.arguments);
    Ok(())
}

async fn test_tool_call_no_args(provider: &dyn Provider, label: &str) -> Result<(), String> {
    let messages = vec![user_msg("What time is it? Use the get_time tool.")];
    let tools = vec![tool_no_args()];
    let r = provider
        .chat(&messages, &tools)
        .await
        .map_err(|e| format!("[{label}] {e}"))?;
    check(
        !r.tool_calls.is_empty(),
        format!(
            "[{label}] expected tool call, got text: {}",
            &r.content[..r.content.len().min(100)]
        ),
    )?;
    check(
        r.tool_calls[0].name == "get_time",
        format!("[{label}] wrong tool: {}", r.tool_calls[0].name),
    )?;
    eprintln!("[{label}] tool_call_no_args: OK");
    Ok(())
}

async fn test_tool_call_multiple_tools(
    provider: &dyn Provider,
    label: &str,
) -> Result<(), String> {
    let messages = vec![user_msg(
        "What's the weather in Paris? Use the get_weather tool. You also have search and get_time available but don't use them.",
    )];
    let tools = vec![tool_get_weather(), tool_search(), tool_no_args()];
    let r = provider
        .chat(&messages, &tools)
        .await
        .map_err(|e| format!("[{label}] {e}"))?;
    check(
        !r.tool_calls.is_empty(),
        format!(
            "[{label}] expected tool call, got text: {}",
            &r.content[..r.content.len().min(100)]
        ),
    )?;
    check(
        r.tool_calls[0].name == "get_weather",
        format!("[{label}] expected get_weather, got: {}", r.tool_calls[0].name),
    )?;
    eprintln!(
        "[{label}] tool_call_multi: OK (called {} tool(s))",
        r.tool_calls.len()
    );
    Ok(())
}

async fn test_tool_result_roundtrip(provider: &dyn Provider, label: &str) -> Result<(), String> {
    // Turn 1: user asks, model calls tool
    let messages = vec![user_msg(
        "What's the weather in Paris? Use the get_weather tool.",
    )];
    let tools = vec![tool_get_weather()];

    let resp1 = provider
        .chat(&messages, &tools)
        .await
        .map_err(|e| format!("[{label}] turn 1: {e}"))?;
    if resp1.tool_calls.is_empty() {
        eprintln!("[{label}] tool_roundtrip: SKIP (model didn't call tool)");
        return Ok(());
    }

    // Turn 2: send tool result, model should respond with text
    let tc = &resp1.tool_calls[0];
    let messages = vec![
        user_msg("What's the weather in Paris? Use the get_weather tool."),
        ChatMessage {
            role: ChatRole::Assistant,
            content: Vec::new(),
            tool_calls: resp1.tool_calls.clone(),
            tool_call_id: None,
        },
        ChatMessage {
            role: ChatRole::Tool,
            content: ozzie_types::text_to_parts(r#"{"temperature": 18, "condition": "cloudy"}"#),
            tool_calls: vec![],
            tool_call_id: Some(tc.id.clone()),
        },
    ];

    let r = provider
        .chat(&messages, &tools)
        .await
        .map_err(|e| format!("[{label}] turn 2: {e}"))?;
    check(
        !r.content.is_empty(),
        format!("[{label}] expected text response after tool result"),
    )?;
    eprintln!(
        "[{label}] tool_roundtrip: OK ({})",
        &r.content[..r.content.len().min(80)]
    );
    Ok(())
}

async fn test_streaming_basic(provider: &dyn Provider, label: &str) -> Result<(), String> {
    let messages = vec![user_msg("Count from 1 to 5.")];
    let mut s = provider
        .chat_stream(&messages, &[])
        .await
        .map_err(|e| format!("[{label}] {e}"))?;

    let mut chunks = 0;
    let mut text = String::new();
    while let Some(delta) = s.next().await {
        match delta {
            Ok(ozzie_llm::ChatDelta::Content(c)) => {
                text.push_str(&c);
                chunks += 1;
            }
            Ok(ozzie_llm::ChatDelta::Done { .. }) => break,
            Ok(_) => {}
            Err(e) => return Err(format!("[{label}] stream error: {e}")),
        }
    }
    check(chunks > 0, format!("[{label}] no chunks received"))?;
    check(!text.is_empty(), format!("[{label}] empty text"))?;
    eprintln!(
        "[{label}] streaming: OK ({chunks} chunks, {} chars)",
        text.len()
    );
    Ok(())
}

async fn test_streaming_with_tools(provider: &dyn Provider, label: &str) -> Result<(), String> {
    let messages = vec![user_msg(
        "What's the weather in Paris? Use the get_weather tool.",
    )];
    let tools = vec![tool_get_weather()];
    let mut s = provider
        .chat_stream(&messages, &tools)
        .await
        .map_err(|e| format!("[{label}] {e}"))?;

    let mut got_tool_start = false;
    let mut tool_name = String::new();
    let mut tool_args = String::new();
    while let Some(delta) = s.next().await {
        match delta {
            Ok(ozzie_llm::ChatDelta::ToolCallStart { name: n, .. }) => {
                got_tool_start = true;
                tool_name = n;
            }
            Ok(ozzie_llm::ChatDelta::ToolCallDelta { arguments, .. }) => {
                tool_args.push_str(&arguments);
            }
            Ok(ozzie_llm::ChatDelta::Done { .. }) => break,
            Ok(_) => {}
            Err(e) => return Err(format!("[{label}] stream error: {e}")),
        }
    }
    check(
        got_tool_start,
        format!("[{label}] no tool call in stream"),
    )?;
    check(
        tool_name == "get_weather",
        format!("[{label}] wrong tool: {tool_name}"),
    )?;
    eprintln!("[{label}] streaming+tools: OK (tool={tool_name}, args={tool_args})");
    Ok(())
}

// ============================================================
// Stream variants — same tool call tests but via chat_stream
// ============================================================

/// Helper: collect a streaming tool call response into (name, args_json).
async fn collect_stream_tool_call(
    provider: &dyn Provider,
    messages: &[ChatMessage],
    tools: &[ToolDefinition],
    label: &str,
) -> Result<(String, String), String> {
    let mut s = provider
        .chat_stream(messages, tools)
        .await
        .map_err(|e| format!("[{label}] {e}"))?;

    let mut tool_name = String::new();
    let mut tool_args = String::new();
    let mut got_tool = false;

    while let Some(delta) = s.next().await {
        match delta {
            Ok(ozzie_llm::ChatDelta::ToolCallStart { name, .. }) => {
                got_tool = true;
                tool_name = name;
            }
            Ok(ozzie_llm::ChatDelta::ToolCallDelta { arguments, .. }) => {
                tool_args.push_str(&arguments);
            }
            Ok(ozzie_llm::ChatDelta::Done { .. }) => break,
            Ok(_) => {}
            Err(e) => return Err(format!("[{label}] stream error: {e}")),
        }
    }

    if !got_tool {
        return Err(format!("[{label}] no tool call in stream"));
    }
    Ok((tool_name, tool_args))
}

async fn test_stream_tool_call_simple(
    provider: &dyn Provider,
    label: &str,
) -> Result<(), String> {
    let messages = vec![user_msg(
        "What's the weather in Paris? Use the get_weather tool.",
    )];
    let tools = vec![tool_get_weather()];
    let (tool_name, tool_args) =
        collect_stream_tool_call(provider, &messages, &tools, label).await?;
    check(
        tool_name == "get_weather",
        format!("[{label}] wrong tool: {tool_name}"),
    )?;
    let args: serde_json::Value = serde_json::from_str(&tool_args)
        .map_err(|e| format!("[{label}] invalid args JSON: {e}"))?;
    check(
        args.get("city").is_some(),
        format!("[{label}] missing city"),
    )?;
    eprintln!("[{label}] stream_tool_simple: OK (args={args})");
    Ok(())
}

async fn test_stream_tool_call_optional_args(
    provider: &dyn Provider,
    label: &str,
) -> Result<(), String> {
    let messages = vec![user_msg(
        "Search for 'Rust programming' using the search tool.",
    )];
    let tools = vec![tool_search()];
    let (tool_name, tool_args) =
        collect_stream_tool_call(provider, &messages, &tools, label).await?;
    check(
        tool_name == "search",
        format!("[{label}] wrong tool: {tool_name}"),
    )?;
    let args: serde_json::Value = serde_json::from_str(&tool_args)
        .map_err(|e| format!("[{label}] invalid args JSON: {e}"))?;
    check(
        args.get("query").is_some(),
        format!("[{label}] missing query"),
    )?;
    eprintln!("[{label}] stream_tool_optional: OK (args={args})");
    Ok(())
}

async fn test_stream_tool_call_no_args(
    provider: &dyn Provider,
    label: &str,
) -> Result<(), String> {
    let messages = vec![user_msg("What time is it? Use the get_time tool.")];
    let tools = vec![tool_no_args()];
    let (tool_name, _) = collect_stream_tool_call(provider, &messages, &tools, label).await?;
    check(
        tool_name == "get_time",
        format!("[{label}] wrong tool: {tool_name}"),
    )?;
    eprintln!("[{label}] stream_tool_no_args: OK");
    Ok(())
}

async fn test_stream_tool_call_multiple(
    provider: &dyn Provider,
    label: &str,
) -> Result<(), String> {
    let messages = vec![user_msg(
        "What's the weather in Paris? Use the get_weather tool. You also have search and get_time available but don't use them.",
    )];
    let tools = vec![tool_get_weather(), tool_search(), tool_no_args()];
    let (tool_name, _) = collect_stream_tool_call(provider, &messages, &tools, label).await?;
    check(
        tool_name == "get_weather",
        format!("[{label}] wrong tool: {tool_name}"),
    )?;
    eprintln!("[{label}] stream_tool_multi: OK");
    Ok(())
}

// ============================================================
// Test runner — runs all providers, collects errors, reports at end
// ============================================================

macro_rules! run_all {
    ($test_fn:ident) => {
        #[tokio::test]
        async fn $test_fn() {
            let providers = configured_providers();
            skip_if_no_providers!(providers);
            let mut errors = Vec::new();
            for p in &providers {
                if let Err(e) = paste::paste! { [<test_ $test_fn>] }(p.provider.as_ref(), &p.label).await {
                    eprintln!("FAIL: {e}");
                    errors.push(e);
                }
            }
            if !errors.is_empty() {
                panic!(
                    "\n{} provider(s) failed:\n  - {}",
                    errors.len(),
                    errors.join("\n  - ")
                );
            }
        }
    };
}

run_all!(simple_chat);
run_all!(chat_with_system_prompt);
run_all!(tool_call_simple);
run_all!(tool_call_optional_args);
run_all!(tool_call_no_args);
run_all!(tool_call_multiple_tools);
run_all!(tool_result_roundtrip);
run_all!(streaming_basic);
run_all!(streaming_with_tools);
run_all!(stream_tool_call_simple);
run_all!(stream_tool_call_optional_args);
run_all!(stream_tool_call_no_args);
run_all!(stream_tool_call_multiple);
