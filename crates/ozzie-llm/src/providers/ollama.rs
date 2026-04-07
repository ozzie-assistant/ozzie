use std::pin::Pin;
use std::time::Duration;

use futures_core::Stream;
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE};
use serde::{Deserialize, Serialize};

use crate::{
    helpers::{self, LineProtocol},
    ChatDelta, ChatMessage, ChatResponse, LlmError, Provider, StopReason, ToolCall,
    ToolDefinition, TokenUsage,
};

const DEFAULT_BASE_URL: &str = "http://localhost:11434";

/// Ollama local model provider (no auth needed).
pub struct OllamaProvider {
    client: reqwest::Client,
    base_url: String,
    model: String,
}

impl OllamaProvider {
    pub fn new(model: &str, base_url: Option<&str>, timeout: Option<Duration>) -> Self {
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
            model: model.to_string(),
        }
    }

    fn headers() -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers
    }

    fn build_request(
        &self,
        messages: &[ChatMessage],
        tools: &[ToolDefinition],
    ) -> OllamaRequest {
        let api_messages: Vec<OllamaMessage> = messages
            .iter()
            .map(|m| OllamaMessage {
                role: m.role.to_string(),
                content: m.content.clone(),
                tool_calls: if m.tool_calls.is_empty() {
                    None
                } else {
                    Some(
                        m.tool_calls
                            .iter()
                            .map(|tc| OllamaToolCall {
                                function: OllamaFunction {
                                    name: tc.name.clone(),
                                    arguments: tc.arguments.clone(),
                                },
                            })
                            .collect(),
                    )
                },
            })
            .collect();

        let api_tools: Option<Vec<OllamaTool>> = if tools.is_empty() {
            None
        } else {
            Some(
                tools
                    .iter()
                    .map(|t| OllamaTool {
                        r#type: "function".to_string(),
                        function: OllamaToolDef {
                            name: t.name.clone(),
                            description: t.description.clone(),
                            parameters: serde_json::to_value(&t.parameters).unwrap_or_default(),
                        },
                    })
                    .collect(),
            )
        };

        OllamaRequest {
            model: self.model.clone(),
            messages: api_messages,
            tools: api_tools,
            stream: false,
        }
    }
}

#[async_trait::async_trait]
impl Provider for OllamaProvider {
    async fn chat(
        &self,
        messages: &[ChatMessage],
        tools: &[ToolDefinition],
    ) -> Result<ChatResponse, LlmError> {
        let mut request = self.build_request(messages, tools);
        request.stream = false;

        let resp = self
            .client
            .post(format!("{}/api/chat", self.base_url))
            .headers(Self::headers())
            .json(&request)
            .send()
            .await
            .map_err(|e| LlmError::ModelUnavailable {
                provider: "ollama".to_string(),
                body: e.to_string(),
            })?;

        let status = resp.status();
        let content_type = resp
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();

        let body = resp
            .text()
            .await
            .map_err(|e| LlmError::Connection(e.to_string()))?;

        if !status.is_success() {
            return Err(LlmError::ModelUnavailable {
                provider: "ollama".to_string(),
                body: truncate(&body, 512),
            });
        }

        // Validate content type (detect reverse proxy errors)
        if !content_type.contains("json") {
            return Err(LlmError::ModelUnavailable {
                provider: "ollama".to_string(),
                body: truncate(&body, 512),
            });
        }

        let api_resp: OllamaResponse = serde_json::from_str(&body)
            .map_err(|e| LlmError::Other(format!("parse response: {e}")))?;

        let mut tool_calls = Vec::new();
        let has_tools;
        if let Some(tcs) = &api_resp.message.tool_calls {
            has_tools = !tcs.is_empty();
            for (i, tc) in tcs.iter().enumerate() {
                tool_calls.push(ToolCall {
                    id: format!("call_{i}"),
                    name: tc.function.name.clone(),
                    arguments: tc.function.arguments.clone(),
                });
            }
        } else {
            has_tools = false;
        }

        let stop_reason = if has_tools {
            Some(StopReason::ToolUse)
        } else {
            Some(StopReason::Stop)
        };

        Ok(ChatResponse {
            content: api_resp.message.content,
            tool_calls,
            usage: TokenUsage {
                input_tokens: api_resp.prompt_eval_count.unwrap_or(0),
                output_tokens: api_resp.eval_count.unwrap_or(0),
                ..Default::default()
            },
            stop_reason,
            model: Some(self.model.clone()),
        })
    }

    async fn chat_stream(
        &self,
        messages: &[ChatMessage],
        tools: &[ToolDefinition],
    ) -> Result<Pin<Box<dyn Stream<Item = Result<ChatDelta, LlmError>> + Send>>, LlmError> {
        let mut request = self.build_request(messages, tools);
        request.stream = true;

        let url = format!("{}/api/chat", self.base_url);
        let model = self.model.clone();

        helpers::send_and_stream(
            &self.client,
            &url,
            Self::headers(),
            &request,
            LineProtocol::NdJson,
            move |data| {
                let chunk: OllamaStreamChunk = match serde_json::from_str(data) {
                    Ok(c) => c,
                    Err(_) => return Some(vec![]),
                };

                if chunk.done {
                    return Some(vec![ChatDelta::Done {
                        usage: TokenUsage {
                            input_tokens: chunk.prompt_eval_count.unwrap_or(0),
                            output_tokens: chunk.eval_count.unwrap_or(0),
                            ..Default::default()
                        },
                        stop_reason: Some(StopReason::Stop),
                        model: Some(model.clone()),
                    }]);
                }

                let mut deltas = Vec::new();

                if let Some(tool_calls) = &chunk.message.tool_calls {
                    for tc in tool_calls {
                        let id = format!("call_{}", tc.function.name);
                        let args =
                            serde_json::to_string(&tc.function.arguments).unwrap_or_default();
                        deltas.push(ChatDelta::ToolCallStart {
                            id: id.clone(),
                            name: tc.function.name.clone(),
                        });
                        deltas.push(ChatDelta::ToolCallDelta { id, arguments: args });
                    }
                }

                if !chunk.message.content.is_empty() {
                    deltas.push(ChatDelta::Content(chunk.message.content));
                }

                Some(deltas)
            },
            Vec::new,
        )
        .await
    }

    fn name(&self) -> &str {
        "ollama"
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}...", &s[..max])
    }
}

// ---- API types ----

#[derive(Serialize)]
struct OllamaRequest {
    model: String,
    messages: Vec<OllamaMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<OllamaTool>>,
    stream: bool,
}

#[derive(Serialize, Deserialize)]
struct OllamaMessage {
    role: String,
    content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<OllamaToolCall>>,
}

#[derive(Serialize, Deserialize)]
struct OllamaToolCall {
    function: OllamaFunction,
}

#[derive(Serialize, Deserialize)]
struct OllamaFunction {
    name: String,
    arguments: serde_json::Value,
}

#[derive(Serialize)]
struct OllamaTool {
    r#type: String,
    function: OllamaToolDef,
}

#[derive(Serialize)]
struct OllamaToolDef {
    name: String,
    description: String,
    parameters: serde_json::Value,
}

#[derive(Deserialize)]
struct OllamaResponse {
    message: OllamaMessage,
    #[serde(default)]
    prompt_eval_count: Option<u64>,
    #[serde(default)]
    eval_count: Option<u64>,
}

#[derive(Deserialize)]
struct OllamaStreamChunk {
    message: OllamaMessage,
    done: bool,
    #[serde(default)]
    prompt_eval_count: Option<u64>,
    #[serde(default)]
    eval_count: Option<u64>,
}
