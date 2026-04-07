use std::collections::HashMap;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use futures_core::Stream;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use serde::{Deserialize, Serialize};

use crate::{
    helpers::{self, LineProtocol},
    ChatDelta, ChatMessage, ChatResponse, LlmError, Provider, ResolvedAuth, StopReason, ToolCall,
    ToolDefinition, TokenUsage,
};

const DEFAULT_MODEL: &str = "gpt-4o";
const DEFAULT_BASE_URL: &str = "https://api.openai.com/v1";

/// OpenAI-compatible provider (works with OpenAI, Mistral, and other compatible APIs).
pub struct OpenAIProvider {
    client: reqwest::Client,
    base_url: String,
    model: String,
    max_tokens: Option<u32>,
    auth: ResolvedAuth,
    provider_name: String,
}

impl OpenAIProvider {
    /// Creates an OpenAI-compatible provider.
    ///
    /// `provider_name` controls what `name()` returns (e.g. "openai", "mistral",
    /// "lm-studio", "vllm"). The wire protocol is identical for all.
    pub fn new(
        auth: ResolvedAuth,
        model: Option<&str>,
        base_url: Option<&str>,
        max_tokens: Option<u32>,
        timeout: Option<Duration>,
        provider_name: Option<&str>,
    ) -> Self {
        let timeout = timeout.unwrap_or(Duration::from_secs(300));
        let client = reqwest::Client::builder()
            .timeout(timeout)
            .build()
            .unwrap_or_default();

        Self {
            client,
            base_url: base_url
                .unwrap_or(DEFAULT_BASE_URL)
                .trim_end_matches('/')
                .to_string(),
            model: model.unwrap_or(DEFAULT_MODEL).to_string(),
            max_tokens,
            auth,
            provider_name: provider_name.unwrap_or("openai").to_string(),
        }
    }

    fn headers(&self) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

        let token = format!("Bearer {}", self.auth.value);
        if let Ok(val) = HeaderValue::from_str(&token) {
            headers.insert(AUTHORIZATION, val);
        }

        headers
    }

    fn build_request(
        &self,
        messages: &[ChatMessage],
        tools: &[ToolDefinition],
        stream: bool,
    ) -> ApiRequest {
        let messages: Vec<ApiMessage> = messages
            .iter()
            .map(|m| {
                let tool_calls = if m.tool_calls.is_empty() {
                    None
                } else {
                    Some(
                        m.tool_calls
                            .iter()
                            .map(|tc| ApiToolCall {
                                id: tc.id.clone(),
                                r#type: "function".to_string(),
                                function: ApiFunction {
                                    name: tc.name.clone(),
                                    arguments: tc.arguments.to_string(),
                                },
                            })
                            .collect(),
                    )
                };

                ApiMessage {
                    role: m.role.to_string(),
                    content: Some(m.content.clone()),
                    tool_calls,
                    tool_call_id: m.tool_call_id.clone(),
                }
            })
            .collect();

        let tools: Option<Vec<ApiToolDef>> = if tools.is_empty() {
            None
        } else {
            Some(
                tools
                    .iter()
                    .map(|t| {
                        let mut schema = t.parameters.clone();
                        crate::schema::normalize_schema(
                            &mut schema,
                            &crate::schema::NormalizeOptions::openai(),
                        );
                        let mut params =
                            serde_json::to_value(&schema).unwrap_or_default();
                        // Ensure "properties" exists (llama.cpp crashes on empty
                        // tool_call when properties is missing).
                        if let Some(obj) = params.as_object_mut() {
                            obj.entry("properties")
                                .or_insert(serde_json::json!({}));
                        }
                        ApiToolDef {
                            r#type: "function".to_string(),
                            function: ApiToolFunction {
                                name: t.name.clone(),
                                description: t.description.clone(),
                                parameters: params,
                            },
                        }
                    })
                    .collect(),
            )
        };

        ApiRequest {
            model: self.model.clone(),
            messages,
            tool_choice: tools.as_ref().map(|_| "auto".to_string()),
            tools,
            max_completion_tokens: self.max_tokens,
            stream: Some(stream),
            stream_options: if stream {
                Some(StreamOptions {
                    include_usage: true,
                })
            } else {
                None
            },
        }
    }
}

fn parse_api_response(api_resp: ApiResponse) -> Result<ChatResponse, LlmError> {
    let choice = api_resp
        .choices
        .into_iter()
        .next()
        .ok_or_else(|| LlmError::Other("no choices in response".into()))?;

    let stop_reason = map_finish_reason(choice.finish_reason.as_deref());

    let tool_calls: Vec<ToolCall> = choice
        .message
        .tool_calls
        .unwrap_or_default()
        .into_iter()
        .map(|tc| ToolCall {
            id: tc.id,
            name: tc.function.name,
            arguments: serde_json::from_str(&tc.function.arguments)
                .unwrap_or(serde_json::Value::Object(Default::default())),
        })
        .collect();

    // XML fallback: some models (Qwen3-Coder, GLM) emit XML tool calls in
    // the content field instead of using the structured tool_calls array.
    let raw_content = choice.message.content.unwrap_or_default();
    let (tool_calls, content) = if tool_calls.is_empty() {
        match parse_xml_tool_calls(&raw_content) {
            Some((calls, remaining)) if !calls.is_empty() => {
                tracing::debug!(
                    "XML fallback: parsed {} tool call(s) from content",
                    calls.len()
                );
                (calls, remaining)
            }
            _ => (tool_calls, raw_content),
        }
    } else {
        (tool_calls, raw_content)
    };

    Ok(ChatResponse {
        content,
        tool_calls,
        usage: api_resp
            .usage
            .map(|u| TokenUsage {
                input_tokens: u.prompt_tokens,
                output_tokens: u.completion_tokens,
                reasoning_tokens: u.completion_tokens_details
                    .and_then(|d| d.reasoning_tokens)
                    .unwrap_or(0),
                ..Default::default()
            })
            .unwrap_or_default(),
        stop_reason,
        model: api_resp.model,
    })
}

fn process_stream_event(
    event: &StreamEvent,
    tool_id_map: &mut HashMap<usize, String>,
) -> Vec<ChatDelta> {
    let mut deltas = Vec::new();
    for choice in &event.choices {
        if let Some(ref content) = choice.delta.content
            && !content.is_empty()
        {
            deltas.push(ChatDelta::Content(content.clone()));
        }
        if let Some(ref reasoning) = choice.delta.reasoning_content
            && !reasoning.is_empty()
        {
            deltas.push(ChatDelta::Reasoning(reasoning.clone()));
        }
        if let Some(ref tcs) = choice.delta.tool_calls {
            for tc in tcs {
                // Track tool IDs by index — OpenAI only sends the id in the first chunk.
                if let Some(ref id) = tc.id
                    && !id.is_empty()
                {
                    tool_id_map.insert(tc.index, id.clone());
                }
                let resolved_id = tool_id_map
                    .get(&tc.index)
                    .cloned()
                    .unwrap_or_default();

                if let Some(ref func) = tc.function {
                    if let Some(ref name) = func.name {
                        deltas.push(ChatDelta::ToolCallStart {
                            id: resolved_id.clone(),
                            name: name.clone(),
                        });
                    }
                    if let Some(ref args) = func.arguments
                        && !args.is_empty()
                    {
                        deltas.push(ChatDelta::ToolCallDelta {
                            id: resolved_id,
                            arguments: args.clone(),
                        });
                    }
                }
            }
        }
    }
    if let Some(ref usage) = event.usage {
        deltas.push(ChatDelta::Done {
            usage: TokenUsage {
                input_tokens: usage.prompt_tokens,
                output_tokens: usage.completion_tokens,
                ..Default::default()
            },
            stop_reason: None,
            model: event.model.clone(),
        });
    }
    deltas
}

fn map_finish_reason(reason: Option<&str>) -> Option<StopReason> {
    reason.map(|r| match r {
        "stop" => StopReason::Stop,
        "tool_calls" => StopReason::ToolUse,
        "length" => StopReason::MaxTokens,
        "content_filter" => StopReason::Safety,
        other => StopReason::Other(other.to_string()),
    })
}

#[async_trait::async_trait]
impl Provider for OpenAIProvider {
    async fn chat(
        &self,
        messages: &[ChatMessage],
        tools: &[ToolDefinition],
    ) -> Result<ChatResponse, LlmError> {
        let body = self.build_request(messages, tools, false);
        let url = format!("{}/chat/completions", self.base_url);

        helpers::post_and_parse::<_, ApiResponse, _>(
            &self.client,
            &url,
            self.headers(),
            &body,
            &self.provider_name,
            parse_api_response,
        )
        .await
    }

    async fn chat_stream(
        &self,
        messages: &[ChatMessage],
        tools: &[ToolDefinition],
    ) -> Result<Pin<Box<dyn Stream<Item = Result<ChatDelta, LlmError>> + Send>>, LlmError> {
        let body = self.build_request(messages, tools, true);
        let url = format!("{}/chat/completions", self.base_url);

        let mut tool_id_map: HashMap<usize, String> = HashMap::new();
        let got_done = Arc::new(AtomicBool::new(false));
        let got_done_finish = Arc::clone(&got_done);

        helpers::send_and_stream(
            &self.client,
            &url,
            self.headers(),
            &body,
            LineProtocol::Sse,
            move |data| {
                if data == "[DONE]" {
                    got_done.store(true, Ordering::Relaxed);
                    return None;
                }
                let Ok(event) = serde_json::from_str::<StreamEvent>(data) else {
                    return Some(vec![]);
                };
                let deltas = process_stream_event(&event, &mut tool_id_map);
                if deltas.iter().any(|d| matches!(d, ChatDelta::Done { .. })) {
                    got_done.store(true, Ordering::Relaxed);
                }
                Some(deltas)
            },
            move || {
                // Stream ended without a Done delta — emit a synthetic one so
                // consumers always see a clean termination signal.
                if got_done_finish.load(Ordering::Relaxed) {
                    return vec![];
                }
                vec![ChatDelta::Done {
                    usage: TokenUsage::default(),
                    stop_reason: Some(StopReason::Stop),
                    model: None,
                }]
            },
        )
        .await
    }

    fn name(&self) -> &str {
        &self.provider_name
    }
}

// ---- API types ----

#[derive(Serialize)]
struct ApiRequest {
    model: String,
    messages: Vec<ApiMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<ApiToolDef>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_choice: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_completion_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream_options: Option<StreamOptions>,
}

#[derive(Serialize)]
struct StreamOptions {
    include_usage: bool,
}

#[derive(Serialize)]
struct ApiMessage {
    role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<ApiToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct ApiToolCall {
    id: String,
    #[serde(default = "default_tool_type")]
    r#type: String,
    function: ApiFunction,
}

fn default_tool_type() -> String {
    "function".to_string()
}

#[derive(Serialize, Deserialize)]
struct ApiFunction {
    name: String,
    arguments: String,
}

#[derive(Serialize)]
struct ApiToolDef {
    r#type: String,
    function: ApiToolFunction,
}

#[derive(Serialize)]
struct ApiToolFunction {
    name: String,
    description: String,
    parameters: serde_json::Value,
}

#[derive(Deserialize)]
struct ApiResponse {
    choices: Vec<ApiChoice>,
    usage: Option<ApiUsage>,
    #[serde(default)]
    model: Option<String>,
}

#[derive(Deserialize)]
struct ApiChoice {
    message: ApiChoiceMessage,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Deserialize)]
struct ApiChoiceMessage {
    content: Option<String>,
    tool_calls: Option<Vec<ApiToolCall>>,
}

#[derive(Deserialize)]
struct ApiUsage {
    prompt_tokens: u64,
    completion_tokens: u64,
    #[serde(default)]
    completion_tokens_details: Option<CompletionTokensDetails>,
}

#[derive(Deserialize)]
struct CompletionTokensDetails {
    #[serde(default)]
    reasoning_tokens: Option<u64>,
}

// ---- Streaming types ----

#[derive(Deserialize)]
struct StreamEvent {
    choices: Vec<StreamChoice>,
    usage: Option<ApiUsage>,
    #[serde(default)]
    model: Option<String>,
}

#[derive(Deserialize)]
struct StreamChoice {
    delta: StreamDelta,
}

#[derive(Deserialize)]
struct StreamDelta {
    content: Option<String>,
    /// Reasoning/thinking content (DeepSeek, o1/o3 via OpenAI-compat).
    reasoning_content: Option<String>,
    tool_calls: Option<Vec<StreamToolCall>>,
}

#[derive(Deserialize)]
struct StreamToolCall {
    #[serde(default)]
    index: usize,
    id: Option<String>,
    function: Option<StreamFunction>,
}

#[derive(Deserialize)]
struct StreamFunction {
    name: Option<String>,
    arguments: Option<String>,
}

// ---------------------------------------------------------------------------
// XML tool-call fallback parser
// ---------------------------------------------------------------------------
//
// Some models (Qwen3-Coder, GLM-4, MiniMax) emit tool calls as XML in the
// content field instead of using the structured `tool_calls` array.  The
// llama.cpp server is supposed to convert these, but it fails intermittently
// when the opening `<tool_call>` tag is missing.
//
// Format (opening `<tool_call>` and closing `</tool_call>` are optional):
//
//   <function=TOOL_NAME>
//   <parameter=KEY>
//   VALUE (may span multiple lines)
//   </parameter>
//   </function>

/// Try to extract XML-style tool calls from content.
/// Returns `Some((tool_calls, remaining_text))` if at least one call was
/// parsed, `None` otherwise.
fn parse_xml_tool_calls(content: &str) -> Option<(Vec<ToolCall>, String)> {
    // Quick check — avoid the regex machinery for the common (no-XML) case.
    if !content.contains("<function=") {
        return None;
    }

    let mut calls = Vec::new();
    let mut remaining = String::new();
    let mut cursor = 0;

    while let Some(fn_start) = content[cursor..].find("<function=") {
        // Text before this function tag is kept as remaining content,
        // but strip any leading <tool_call> wrapper tag.
        let prefix = &content[cursor..cursor + fn_start];
        if let Some(tc_pos) = prefix.rfind("<tool_call>") {
            // Keep text before <tool_call>, discard the tag itself
            remaining.push_str(prefix[..tc_pos].trim_end());
            // (text between <tool_call> and <function= is whitespace, skip it)
        } else {
            remaining.push_str(prefix);
        }
        let fn_start_abs = cursor + fn_start;

        // Extract tool name: <function=NAME>
        let after_eq = fn_start_abs + "<function=".len();
        let name_end = match content[after_eq..].find('>') {
            Some(i) => after_eq + i,
            None => break, // malformed
        };
        let name = content[after_eq..name_end].trim().to_string();
        if name.is_empty() {
            break;
        }

        // Find the closing </function>
        let body_start = name_end + 1;
        let fn_end = match content[body_start..].find("</function>") {
            Some(i) => body_start + i,
            None => break,
        };
        let body = &content[body_start..fn_end];

        // Extract parameters: <parameter=KEY>VALUE</parameter>
        let mut args = serde_json::Map::new();
        let mut param_cursor = 0;
        while let Some(ps) = body[param_cursor..].find("<parameter=") {
            let ps_abs = param_cursor + ps;
            let after_p_eq = ps_abs + "<parameter=".len();
            let key_end = match body[after_p_eq..].find('>') {
                Some(i) => after_p_eq + i,
                None => break,
            };
            let key = body[after_p_eq..key_end].trim().to_string();

            let val_start = key_end + 1;
            let val_end = match body[val_start..].find("</parameter>") {
                Some(i) => val_start + i,
                None => break,
            };
            let val = body[val_start..val_end].trim();

            // Try parsing as JSON value first (handles numbers, bools, arrays, objects).
            let json_val = serde_json::from_str(val)
                .unwrap_or(serde_json::Value::String(val.to_string()));
            args.insert(key, json_val);

            param_cursor = val_end + "</parameter>".len();
        }

        // Generate a random ID (32 alphanumeric chars, matching llama.cpp style).
        let id: String = (0..32)
            .map(|_| {
                let idx = rand::random::<u8>() % 62;
                (match idx {
                    0..=9 => b'0' + idx,
                    10..=35 => b'a' + idx - 10,
                    _ => b'A' + idx - 36,
                }) as char
            })
            .collect();

        calls.push(ToolCall {
            id,
            name,
            arguments: serde_json::Value::Object(args),
        });

        // Advance past </function> and optional </tool_call>
        cursor = fn_end + "</function>".len();
        let after_fn = content[cursor..].trim_start();
        if after_fn.starts_with("</tool_call>") {
            cursor = content.len() - after_fn.len() + "</tool_call>".len();
        }
    }

    // Append any trailing text after the last parsed call.
    if cursor < content.len() {
        remaining.push_str(&content[cursor..]);
    }

    if calls.is_empty() {
        None
    } else {
        Some((calls, remaining.trim().to_string()))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xml_with_tool_call_wrapper() {
        let content = "<tool_call>\n<function=store_memory>\n<parameter=content>\nHello world\n</parameter>\n<parameter=type>\nnote\n</parameter>\n</function>\n</tool_call>";
        let (calls, remaining) = parse_xml_tool_calls(content).unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "store_memory");
        assert_eq!(calls[0].arguments["content"], "Hello world");
        assert_eq!(calls[0].arguments["type"], "note");
        assert!(remaining.is_empty());
    }

    #[test]
    fn xml_without_tool_call_wrapper() {
        // The common Qwen failure case: no opening <tool_call>
        let content = "<function=store_memory>\n<parameter=content>\nclé ABC\n</parameter>\n<parameter=type>\nmemo\n</parameter>\n</function>\n</tool_call>";
        let (calls, remaining) = parse_xml_tool_calls(content).unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "store_memory");
        assert_eq!(calls[0].arguments["content"], "clé ABC");
        assert_eq!(calls[0].arguments["type"], "memo");
        assert!(remaining.is_empty());
    }

    #[test]
    fn xml_with_reasoning_before() {
        let content = "Let me store this in memory.\n\n<function=store_memory>\n<parameter=content>\ntest\n</parameter>\n</function>";
        let (calls, remaining) = parse_xml_tool_calls(content).unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "store_memory");
        assert_eq!(remaining, "Let me store this in memory.");
    }

    #[test]
    fn xml_multiline_value() {
        let content = "<function=file_write>\n<parameter=path>\n/tmp/test.txt\n</parameter>\n<parameter=content>\nLine 1\nLine 2\nLine 3\n</parameter>\n</function>";
        let (calls, remaining) = parse_xml_tool_calls(content).unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].arguments["content"], "Line 1\nLine 2\nLine 3");
        assert!(remaining.is_empty());
    }

    #[test]
    fn xml_multiple_calls() {
        let content = "<function=file_read>\n<parameter=path>\na.txt\n</parameter>\n</function>\n<function=file_read>\n<parameter=path>\nb.txt\n</parameter>\n</function>";
        let (calls, _) = parse_xml_tool_calls(content).unwrap();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].arguments["path"], "a.txt");
        assert_eq!(calls[1].arguments["path"], "b.txt");
    }

    #[test]
    fn no_xml_returns_none() {
        assert!(parse_xml_tool_calls("Just a normal response.").is_none());
        assert!(parse_xml_tool_calls("").is_none());
    }

    #[test]
    fn malformed_xml_returns_none() {
        // Missing closing </function>
        assert!(parse_xml_tool_calls("<function=test>\n<parameter=a>\nval\n</parameter>").is_none());
    }

    #[test]
    fn json_parameter_value() {
        let content = "<function=calculator>\n<parameter=numbers>\n[1, 2, 3]\n</parameter>\n<parameter=count>\n42\n</parameter>\n</function>";
        let (calls, _) = parse_xml_tool_calls(content).unwrap();
        assert_eq!(calls[0].arguments["numbers"], serde_json::json!([1, 2, 3]));
        assert_eq!(calls[0].arguments["count"], serde_json::json!(42));
    }

    #[test]
    fn unique_ids_generated() {
        let content = "<function=a>\n</function>\n<function=b>\n</function>";
        let (calls, _) = parse_xml_tool_calls(content).unwrap();
        assert_eq!(calls.len(), 2);
        assert_ne!(calls[0].id, calls[1].id);
        assert_eq!(calls[0].id.len(), 32);
    }
}
