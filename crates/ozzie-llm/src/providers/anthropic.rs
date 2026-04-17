use std::pin::Pin;
use std::time::Duration;

use futures_core::Stream;
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE};
use serde::{Deserialize, Serialize};

use crate::{
    helpers::{self, LineProtocol},
    AuthKind, ChatDelta, ChatMessage, ChatResponse, ChatRole, LlmError, Provider, ResolvedAuth,
    StopReason, ToolCall, ToolDefinition, TokenUsage,
};

const DEFAULT_MODEL: &str = "claude-sonnet-4-6";
const DEFAULT_MAX_TOKENS: u32 = 4096;
const DEFAULT_BASE_URL: &str = "https://api.anthropic.com";
const API_VERSION: &str = "2023-06-01";

/// Prompt caching mode for Anthropic.
#[derive(Debug, Clone, Copy)]
pub enum CacheMode {
    /// Adds `cache_control` to the system prompt block — Anthropic manages placement.
    Automatic,
}

/// Anthropic Claude provider.
pub struct AnthropicProvider {
    client: reqwest::Client,
    base_url: String,
    model: String,
    max_tokens: u32,
    auth: ResolvedAuth,
    cache_mode: Option<CacheMode>,
}

impl AnthropicProvider {
    pub fn new(
        auth: ResolvedAuth,
        model: Option<&str>,
        base_url: Option<&str>,
        max_tokens: Option<u32>,
        timeout: Option<Duration>,
    ) -> Self {
        let timeout = timeout.unwrap_or(Duration::from_secs(300));
        let client = reqwest::Client::builder()
            .timeout(timeout)
            .build()
            .unwrap_or_default();

        Self {
            client,
            base_url: base_url.unwrap_or(DEFAULT_BASE_URL).trim_end_matches('/').to_string(),
            model: model.unwrap_or(DEFAULT_MODEL).to_string(),
            max_tokens: max_tokens.unwrap_or(DEFAULT_MAX_TOKENS),
            auth,
            cache_mode: None,
        }
    }

    /// Enables prompt caching.
    pub fn with_cache_mode(mut self, mode: CacheMode) -> Self {
        self.cache_mode = Some(mode);
        self
    }

    fn headers(&self) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert("anthropic-version", HeaderValue::from_static(API_VERSION));

        if self.cache_mode.is_some() {
            headers.insert(
                "anthropic-beta",
                HeaderValue::from_static("prompt-caching-2024-07-31"),
            );
        }

        match self.auth.kind {
            AuthKind::BearerToken => {
                if let Ok(v) = HeaderValue::from_str(&format!("Bearer {}", self.auth.value)) {
                    headers.insert("authorization", v);
                }
            }
            AuthKind::ApiKey => {
                if let Ok(v) = HeaderValue::from_str(&self.auth.value) {
                    headers.insert("x-api-key", v);
                }
            }
        }
        headers
    }

    fn build_request(&self, messages: &[ChatMessage], tools: &[ToolDefinition]) -> ApiRequest {
        let api_messages: Vec<ApiMessage> = messages
            .iter()
            .filter(|m| m.role != ChatRole::System)
            .map(|m| {
                if !m.tool_calls.is_empty() {
                    let mut blocks = Vec::new();
                    let text = m.text_content();
                    if !text.is_empty() {
                        blocks.push(ContentBlock::Text { text });
                    }
                    for tc in &m.tool_calls {
                        blocks.push(ContentBlock::ToolUse {
                            id: tc.id.clone(),
                            name: tc.name.clone(),
                            input: tc.arguments.clone(),
                        });
                    }
                    ApiMessage { role: m.role.to_string(), content: MessageContent::Blocks(blocks) }
                } else if m.role == ChatRole::Tool {
                    ApiMessage {
                        role: "user".to_string(),
                        content: MessageContent::Blocks(vec![ContentBlock::ToolResult {
                            tool_use_id: m.tool_call_id.clone().unwrap_or_default(),
                            content: m.text_content(),
                        }]),
                    }
                } else {
                    let blocks = content_parts_to_blocks(&m.content);
                    if blocks.len() == 1 && matches!(&blocks[0], ContentBlock::Text { .. }) {
                        ApiMessage { role: m.role.to_string(), content: MessageContent::Text(m.text_content()) }
                    } else {
                        ApiMessage { role: m.role.to_string(), content: MessageContent::Blocks(blocks) }
                    }
                }
            })
            .collect();

        let system_text: String = messages
            .iter()
            .filter(|m| m.role == ChatRole::System)
            .map(|m| m.text_content())
            .collect::<Vec<_>>()
            .join("\n");

        let system = if system_text.is_empty() {
            None
        } else if self.cache_mode.is_some() {
            // With caching: send as content blocks with cache_control.
            Some(serde_json::json!([{
                "type": "text",
                "text": system_text,
                "cache_control": {"type": "ephemeral"}
            }]))
        } else {
            Some(serde_json::Value::String(system_text))
        };

        let api_tools: Vec<ApiTool> = tools
            .iter()
            .map(|t| {
                let mut schema = t.parameters.clone();
                crate::schema::normalize_schema(
                    &mut schema,
                    &crate::schema::NormalizeOptions::anthropic(),
                );
                ApiTool {
                    name: t.name.clone(),
                    description: t.description.clone(),
                    input_schema: serde_json::to_value(&schema).unwrap_or_default(),
                }
            })
            .collect();

        ApiRequest {
            model: self.model.clone(),
            max_tokens: self.max_tokens,
            system,
            messages: api_messages,
            tools: if api_tools.is_empty() { None } else { Some(api_tools) },
            stream: false,
        }
    }
}

fn map_stop_reason(reason: Option<&str>) -> Option<StopReason> {
    reason.map(|r| match r {
        "end_turn" | "stop" => StopReason::Stop,
        "tool_use" => StopReason::ToolUse,
        "max_tokens" => StopReason::MaxTokens,
        other => StopReason::Other(other.to_string()),
    })
}

fn parse_response(api_resp: ApiResponse, model: &str) -> Result<ChatResponse, LlmError> {
    let mut content = String::new();
    let mut tool_calls = Vec::new();

    for block in &api_resp.content {
        match block {
            ContentBlock::Text { text } => content.push_str(text),
            ContentBlock::ToolUse { id, name, input } => {
                tool_calls.push(ToolCall {
                    id: id.clone(),
                    name: name.clone(),
                    arguments: input.clone(),
                });
            }
            _ => {}
        }
    }

    let stop_reason = map_stop_reason(api_resp.stop_reason.as_deref());

    Ok(ChatResponse {
        content,
        tool_calls,
        usage: TokenUsage {
            input_tokens: api_resp.usage.input_tokens,
            output_tokens: api_resp.usage.output_tokens,
            cache_read_tokens: api_resp.usage.cache_read_input_tokens.unwrap_or(0),
            cache_write_tokens: api_resp.usage.cache_creation_input_tokens.unwrap_or(0),
            ..Default::default()
        },
        stop_reason,
        model: Some(model.to_string()),
    })
}

#[async_trait::async_trait]
impl Provider for AnthropicProvider {
    async fn chat(
        &self,
        messages: &[ChatMessage],
        tools: &[ToolDefinition],
    ) -> Result<ChatResponse, LlmError> {
        let mut request = self.build_request(messages, tools);
        request.stream = false;

        let url = format!("{}/v1/messages", self.base_url);
        let model = self.model.clone();

        helpers::post_and_parse::<_, ApiResponse, _>(
            &self.client,
            &url,
            self.headers(),
            &request,
            "anthropic",
            |api_resp| parse_response(api_resp, &model),
        )
        .await
    }

    async fn chat_stream(
        &self,
        messages: &[ChatMessage],
        tools: &[ToolDefinition],
    ) -> Result<Pin<Box<dyn Stream<Item = Result<ChatDelta, LlmError>> + Send>>, LlmError> {
        let mut request = self.build_request(messages, tools);
        request.stream = true;

        let url = format!("{}/v1/messages", self.base_url);
        let model = self.model.clone();
        let mut current_tool_id = String::new();

        helpers::send_and_stream(
            &self.client,
            &url,
            self.headers(),
            &request,
            LineProtocol::Sse,
            move |data: &str| {
                if data == "[DONE]" {
                    return None;
                }
                let event: StreamEvent = match serde_json::from_str(data) {
                    Ok(e) => e,
                    Err(_) => return Some(vec![]),
                };
                let (delta, new_tool_id) = process_stream_event(event, &current_tool_id, &model);
                if let Some(id) = new_tool_id {
                    current_tool_id = id;
                }
                Some(delta.into_iter().collect())
            },
            Vec::new,
        )
        .await
    }

    fn name(&self) -> &str {
        "anthropic"
    }
}

/// Returns (optional delta, optional new current_tool_id).
fn process_stream_event(event: StreamEvent, current_tool_id: &str, model: &str) -> (Option<ChatDelta>, Option<String>) {
    match event {
        StreamEvent::ContentBlockDelta { delta, .. } => match delta {
            Delta::Text { text } => (Some(ChatDelta::Content(text)), None),
            Delta::Thinking { thinking } => (Some(ChatDelta::Reasoning(thinking)), None),
            Delta::InputJson { partial_json } => (Some(ChatDelta::ToolCallDelta {
                id: current_tool_id.to_string(),
                arguments: partial_json,
            }), None),
        },
        StreamEvent::ContentBlockStart { content_block, .. } => {
            if let Some(ContentBlock::ToolUse { id, name, .. }) = content_block {
                let new_id = id.clone();
                (Some(ChatDelta::ToolCallStart { id, name }), Some(new_id))
            } else {
                (None, None)
            }
        }
        StreamEvent::MessageDelta { usage, stop_reason, .. } => {
            if let Some(u) = usage {
                let stop = map_stop_reason(stop_reason.as_deref());
                (Some(ChatDelta::Done {
                    usage: TokenUsage {
                        input_tokens: u.input_tokens.unwrap_or(0),
                        output_tokens: u.output_tokens.unwrap_or(0),
                        cache_read_tokens: u.cache_read_input_tokens.unwrap_or(0),
                        cache_write_tokens: u.cache_creation_input_tokens.unwrap_or(0),
                        ..Default::default()
                    },
                    stop_reason: stop,
                    model: Some(model.to_string()),
                }), None)
            } else {
                (None, None)
            }
        }
        _ => (None, None),
    }
}

/// Converts domain `ContentPart`s to Anthropic content blocks.
///
/// Images are sent as base64 `source` blocks — the caller must have loaded
/// the blob bytes before calling this (see `BlobStore`).
fn content_parts_to_blocks(parts: &[crate::Content]) -> Vec<ContentBlock> {
    parts.iter().map(|p| match p {
        crate::Content::Text { text } => ContentBlock::Text { text: text.clone() },
        crate::Content::Image { media_type, data, .. } => ContentBlock::Image {
            source: ImageSource {
                source_type: "base64".to_string(),
                media_type: media_type.clone(),
                data: data.clone(),
            },
        },
    }).collect()
}

// ---- API types ----

#[derive(Serialize)]
struct ApiRequest {
    model: String,
    max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<serde_json::Value>,
    messages: Vec<ApiMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<ApiTool>>,
    stream: bool,
}

#[derive(Serialize)]
struct ApiMessage {
    role: String,
    content: MessageContent,
}

#[derive(Serialize)]
#[serde(untagged)]
enum MessageContent {
    Text(String),
    Blocks(Vec<ContentBlock>),
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
enum ContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "image")]
    Image { source: ImageSource },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    #[serde(rename = "tool_result")]
    ToolResult {
        tool_use_id: String,
        content: String,
    },
}

#[derive(Debug, Serialize, Deserialize)]
struct ImageSource {
    #[serde(rename = "type")]
    source_type: String,
    media_type: String,
    data: String,
}

#[derive(Serialize)]
struct ApiTool {
    name: String,
    description: String,
    input_schema: serde_json::Value,
}

#[derive(Deserialize)]
struct ApiResponse {
    content: Vec<ContentBlock>,
    usage: ApiUsage,
    #[serde(default)]
    stop_reason: Option<String>,
}

#[derive(Deserialize)]
struct ApiUsage {
    input_tokens: u64,
    output_tokens: u64,
    #[serde(default)]
    cache_read_input_tokens: Option<u64>,
    #[serde(default)]
    cache_creation_input_tokens: Option<u64>,
}

#[derive(Deserialize)]
#[serde(tag = "type")]
enum StreamEvent {
    #[serde(rename = "content_block_start")]
    ContentBlockStart {
        #[serde(default)]
        content_block: Option<ContentBlock>,
    },
    #[serde(rename = "content_block_delta")]
    ContentBlockDelta {
        delta: Delta,
        /// Block index in the Anthropic SSE stream (not consumed).
        #[serde(default)]
        #[allow(dead_code)]
        index: usize,
    },
    #[serde(rename = "message_delta")]
    MessageDelta {
        #[serde(default)]
        usage: Option<StreamUsage>,
        #[serde(default)]
        stop_reason: Option<String>,
    },
    #[serde(rename = "message_start")]
    MessageStart {},
    #[serde(rename = "message_stop")]
    MessageStop {},
    #[serde(rename = "content_block_stop")]
    ContentBlockStop {},
    #[serde(rename = "ping")]
    Ping {},
}

#[derive(Deserialize)]
#[serde(tag = "type")]
enum Delta {
    #[serde(rename = "text_delta")]
    Text { text: String },
    #[serde(rename = "thinking_delta")]
    Thinking { thinking: String },
    #[serde(rename = "input_json_delta")]
    InputJson { partial_json: String },
}

#[derive(Deserialize)]
struct StreamUsage {
    #[serde(default)]
    input_tokens: Option<u64>,
    #[serde(default)]
    output_tokens: Option<u64>,
    #[serde(default)]
    cache_read_input_tokens: Option<u64>,
    #[serde(default)]
    cache_creation_input_tokens: Option<u64>,
}
