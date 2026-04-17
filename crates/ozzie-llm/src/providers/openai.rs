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
    /// When false, tools are injected as XML in the system prompt instead of
    /// using the native OpenAI tool calling API. Useful for llama.cpp and other
    /// backends where models lack reliable tool calling support.
    native_tool_calling: bool,
}

impl OpenAIProvider {
    /// Creates an OpenAI-compatible provider with native tool calling enabled.
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
        Self::with_native_tools(auth, model, base_url, max_tokens, timeout, provider_name, true)
    }

    pub fn with_native_tools(
        auth: ResolvedAuth,
        model: Option<&str>,
        base_url: Option<&str>,
        max_tokens: Option<u32>,
        timeout: Option<Duration>,
        provider_name: Option<&str>,
        native_tool_calling: bool,
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
            native_tool_calling,
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
        let use_shim = !self.native_tool_calling && !tools.is_empty();

        let messages: Vec<ApiMessage> = messages
            .iter()
            .enumerate()
            .map(|(i, m)| {
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

                let mut content = content_parts_to_openai(&m.content);

                // Prepend tool template to the first system message
                if use_shim
                    && i == 0
                    && m.role == crate::ChatRole::System
                    && let serde_json::Value::String(ref text) = content
                {
                    let template = crate::toolshim::tool_prompt_template(tools);
                    content = serde_json::Value::String(format!("{text}\n\n{template}"));
                }

                ApiMessage {
                    role: m.role.to_string(),
                    content: Some(content),
                    tool_calls,
                    tool_call_id: m.tool_call_id.clone(),
                }
            })
            .collect();

        let tools: Option<Vec<ApiToolDef>> = if tools.is_empty() || use_shim {
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
        match crate::toolshim::parse_xml_tool_calls(&raw_content) {
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

/// Converts domain content parts to OpenAI `content` field.
///
/// Text-only → plain string. Mixed → array of content objects.
fn content_parts_to_openai(parts: &[crate::Content]) -> serde_json::Value {
    let has_images = parts.iter().any(|p| p.is_image());
    if !has_images {
        return serde_json::Value::String(crate::parts_to_text(parts));
    }
    let blocks: Vec<serde_json::Value> = parts.iter().map(|p| match p {
        crate::Content::Text { text } => serde_json::json!({
            "type": "text",
            "text": text,
        }),
        crate::Content::Image { media_type, data, .. } => serde_json::json!({
            "type": "image_url",
            "image_url": {
                "url": format!("data:{media_type};base64,{data}"),
            },
        }),
    }).collect();
    serde_json::Value::Array(blocks)
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
    content: Option<serde_json::Value>,
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
