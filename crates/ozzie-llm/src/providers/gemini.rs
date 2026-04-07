use std::pin::Pin;
use std::time::Duration;

use futures_core::Stream;
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE};
use serde::{Deserialize, Serialize};

use crate::helpers::{self, LineProtocol};
use crate::{
    ChatDelta, ChatMessage, ChatResponse, ChatRole, LlmError, Provider, ResolvedAuth, StopReason,
    ToolCall, ToolDefinition, TokenUsage,
};

const DEFAULT_MODEL: &str = "gemini-2.5-flash";
const DEFAULT_BASE_URL: &str = "https://generativelanguage.googleapis.com/v1beta";

/// Google Gemini provider (REST API).
pub struct GeminiProvider {
    client: reqwest::Client,
    base_url: String,
    model: String,
    max_tokens: Option<u32>,
    api_key: String,
}

impl GeminiProvider {
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
            base_url: base_url
                .unwrap_or(DEFAULT_BASE_URL)
                .trim_end_matches('/')
                .to_string(),
            model: model.unwrap_or(DEFAULT_MODEL).to_string(),
            max_tokens,
            api_key: auth.value,
        }
    }

    fn headers(&self) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers
    }

    fn build_request(
        &self,
        messages: &[ChatMessage],
        tools: &[ToolDefinition],
    ) -> GeminiRequest {
        let mut contents = Vec::new();
        let mut system_instruction = None;

        for msg in messages {
            if msg.role == ChatRole::System {
                system_instruction = Some(GeminiContent {
                    role: "user".to_string(),
                    parts: vec![GeminiPart::Text {
                        text: msg.content.clone(),
                    }],
                });
                continue;
            }

            let role = match msg.role {
                ChatRole::Assistant => "model",
                ChatRole::Tool | ChatRole::User | ChatRole::System => "user",
            };

            let mut parts = Vec::new();

            if !msg.content.is_empty() {
                parts.push(GeminiPart::Text {
                    text: msg.content.clone(),
                });
            }

            for tc in &msg.tool_calls {
                parts.push(GeminiPart::FunctionCall {
                    function_call: GeminiFunctionCall {
                        name: tc.name.clone(),
                        args: tc.arguments.clone(),
                    },
                });
            }

            if let Some(ref tc_id) = msg.tool_call_id {
                parts.push(GeminiPart::FunctionResponse {
                    function_response: GeminiFunctionResponse {
                        name: tc_id.clone(),
                        response: serde_json::json!({"result": msg.content}),
                    },
                });
            }

            contents.push(GeminiContent {
                role: role.to_string(),
                parts,
            });
        }

        let tool_declarations = if tools.is_empty() {
            None
        } else {
            let declarations: Vec<GeminiFunctionDeclaration> = tools
                .iter()
                .map(|t| GeminiFunctionDeclaration {
                    name: t.name.clone(),
                    description: t.description.clone(),
                    parameters_json_schema: {
                        let mut schema = t.parameters.clone();
                        crate::schema::normalize_schema(
                            &mut schema,
                            &crate::schema::NormalizeOptions::gemini(),
                        );
                        serde_json::to_value(&schema).unwrap_or_default()
                    },
                })
                .collect();
            Some(vec![GeminiToolDeclaration {
                function_declarations: declarations,
            }])
        };

        let generation_config = Some(GeminiGenerationConfig {
            max_output_tokens: self.max_tokens,
        });

        GeminiRequest {
            contents,
            system_instruction,
            tools: tool_declarations,
            generation_config,
        }
    }
}

fn map_finish_reason(reason: Option<&str>) -> Option<StopReason> {
    reason.map(|r| match r {
        "STOP" => StopReason::Stop,
        "MAX_TOKENS" => StopReason::MaxTokens,
        "SAFETY" | "RECITATION" | "BLOCKLIST" => StopReason::Safety,
        other => StopReason::Other(other.to_string()),
    })
}

#[async_trait::async_trait]
impl Provider for GeminiProvider {
    async fn chat(
        &self,
        messages: &[ChatMessage],
        tools: &[ToolDefinition],
    ) -> Result<ChatResponse, LlmError> {
        let body = self.build_request(messages, tools);
        let url = format!(
            "{}/models/{}:generateContent?key={}",
            self.base_url, self.model, self.api_key
        );
        let model = self.model.clone();

        helpers::post_and_parse::<_, GeminiResponse, _>(
            &self.client,
            &url,
            self.headers(),
            &body,
            "gemini",
            |resp| parse_gemini_response(resp, &model),
        )
        .await
    }

    async fn chat_stream(
        &self,
        messages: &[ChatMessage],
        tools: &[ToolDefinition],
    ) -> Result<Pin<Box<dyn Stream<Item = Result<ChatDelta, LlmError>> + Send>>, LlmError> {
        let body = self.build_request(messages, tools);
        let url = format!(
            "{}/models/{}:streamGenerateContent?alt=sse&key={}",
            self.base_url, self.model, self.api_key
        );
        let model = self.model.clone();

        helpers::send_and_stream(
            &self.client,
            &url,
            self.headers(),
            &body,
            LineProtocol::Sse,
            move |data| {
                let Ok(event) = serde_json::from_str::<GeminiResponse>(data) else {
                    return Some(vec![]);
                };
                Some(process_gemini_stream_event(&event, &model))
            },
            Vec::new,
        )
        .await
    }

    fn name(&self) -> &str {
        "gemini"
    }
}

fn process_gemini_stream_event(event: &GeminiResponse, model: &str) -> Vec<ChatDelta> {
    let mut deltas = Vec::new();
    let mut is_final = false;

    for candidate in &event.candidates {
        if candidate.finish_reason.is_some() {
            is_final = true;
        }
        let parts = candidate
            .content
            .as_ref()
            .map(|c| &c.parts[..])
            .unwrap_or_default();
        for part in parts {
            match part {
                GeminiPart::Text { text } if !text.is_empty() => {
                    deltas.push(ChatDelta::Content(text.clone()));
                }
                GeminiPart::FunctionCall { function_call } => {
                    let args =
                        serde_json::to_string(&function_call.args).unwrap_or_default();
                    let id = format!("call_{}", function_call.name);
                    deltas.push(ChatDelta::ToolCallStart {
                        id: id.clone(),
                        name: function_call.name.clone(),
                    });
                    deltas.push(ChatDelta::ToolCallDelta { id, arguments: args });
                }
                _ => {}
            }
        }
    }

    if is_final {
        let usage = event
            .usage_metadata
            .as_ref()
            .map(|meta| TokenUsage {
                input_tokens: meta.prompt_token_count.unwrap_or(0),
                output_tokens: meta.candidates_token_count.unwrap_or(0),
                ..Default::default()
            })
            .unwrap_or_default();
        deltas.push(ChatDelta::Done {
            usage,
            stop_reason: None,
            model: Some(model.to_string()),
        });
    }

    deltas
}

fn parse_gemini_response(resp: GeminiResponse, model: &str) -> Result<ChatResponse, LlmError> {
    // Check for embedded error (Gemini sometimes returns error inside a 200)
    if let Some(err) = resp.error {
        return Err(LlmError::classify(&format!(
            "Gemini API error ({}): {}",
            err.code, err.message
        )));
    }

    let candidate = resp
        .candidates
        .into_iter()
        .next()
        .ok_or_else(|| LlmError::Other("no candidates in response".into()))?;

    let stop_reason = map_finish_reason(candidate.finish_reason.as_deref());

    // Candidate may lack content (e.g. blocked by safety filters)
    let parts = candidate
        .content
        .map(|c| c.parts)
        .unwrap_or_default();

    if parts.is_empty() {
        let reason = candidate.finish_reason.unwrap_or_else(|| "unknown".into());
        if reason != "STOP" {
            return Err(LlmError::Other(format!(
                "Gemini returned empty response (finishReason: {reason})"
            )));
        }
    }

    let mut content = String::new();
    let mut tool_calls = Vec::new();

    for part in parts {
        match part {
            GeminiPart::Text { text } => content.push_str(&text),
            GeminiPart::FunctionCall { function_call } => {
                tool_calls.push(ToolCall {
                    id: format!("call_{}", function_call.name),
                    name: function_call.name,
                    arguments: function_call.args,
                });
            }
            _ => {}
        }
    }

    let usage = resp
        .usage_metadata
        .map(|m| TokenUsage {
            input_tokens: m.prompt_token_count.unwrap_or(0),
            output_tokens: m.candidates_token_count.unwrap_or(0),
            ..Default::default()
        })
        .unwrap_or_default();

    Ok(ChatResponse {
        content,
        tool_calls,
        usage,
        stop_reason,
        model: Some(model.to_string()),
    })
}

// ---- Gemini API types ----

#[derive(Serialize)]
struct GeminiRequest {
    contents: Vec<GeminiContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    system_instruction: Option<GeminiContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<GeminiToolDeclaration>>,
    #[serde(rename = "generationConfig", skip_serializing_if = "Option::is_none")]
    generation_config: Option<GeminiGenerationConfig>,
}

#[derive(Serialize, Deserialize)]
struct GeminiContent {
    role: String,
    #[serde(default)]
    parts: Vec<GeminiPart>,
}

#[derive(Serialize, Deserialize)]
#[serde(untagged)]
enum GeminiPart {
    Text {
        text: String,
    },
    FunctionCall {
        #[serde(rename = "functionCall")]
        function_call: GeminiFunctionCall,
    },
    FunctionResponse {
        #[serde(rename = "functionResponse")]
        function_response: GeminiFunctionResponse,
    },
}

#[derive(Serialize, Deserialize)]
struct GeminiFunctionCall {
    name: String,
    args: serde_json::Value,
}

#[derive(Serialize, Deserialize)]
struct GeminiFunctionResponse {
    name: String,
    response: serde_json::Value,
}

#[derive(Serialize)]
struct GeminiToolDeclaration {
    #[serde(rename = "functionDeclarations")]
    function_declarations: Vec<GeminiFunctionDeclaration>,
}

#[derive(Serialize)]
struct GeminiFunctionDeclaration {
    name: String,
    description: String,
    #[serde(rename = "parametersJsonSchema")]
    parameters_json_schema: serde_json::Value,
}

#[derive(Serialize)]
struct GeminiGenerationConfig {
    #[serde(rename = "maxOutputTokens", skip_serializing_if = "Option::is_none")]
    max_output_tokens: Option<u32>,
}

#[derive(Deserialize)]
struct GeminiResponse {
    #[serde(default)]
    candidates: Vec<GeminiCandidate>,
    #[serde(rename = "usageMetadata")]
    usage_metadata: Option<GeminiUsageMetadata>,
    /// Gemini may return an error object inside a 200 response.
    error: Option<GeminiErrorBody>,
}

#[derive(Deserialize)]
struct GeminiErrorBody {
    message: String,
    #[serde(default)]
    code: u16,
}

#[derive(Deserialize)]
struct GeminiCandidate {
    content: Option<GeminiContent>,
    #[serde(rename = "finishReason")]
    finish_reason: Option<String>,
}

#[derive(Deserialize)]
struct GeminiUsageMetadata {
    #[serde(rename = "promptTokenCount")]
    prompt_token_count: Option<u64>,
    #[serde(rename = "candidatesTokenCount")]
    candidates_token_count: Option<u64>,
}

// Schema normalization moved to crate::schema (shared between providers).

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn function_declaration_serializes_with_json_schema() {
        let decl = GeminiFunctionDeclaration {
            name: "test".to_string(),
            description: "A test tool".to_string(),
            parameters_json_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string" }
                }
            }),
        };

        let json = serde_json::to_value(&decl).unwrap();
        assert!(json.get("parametersJsonSchema").is_some());
        assert!(json.get("parameters").is_none());
    }

    #[test]
    fn normalize_converts_nullable_union_to_nullable_field() {
        use schemars::schema::*;

        let mut root = RootSchema {
            schema: SchemaObject {
                instance_type: Some(SingleOrVec::Single(Box::new(InstanceType::Object))),
                object: Some(Box::new(ObjectValidation {
                    properties: {
                        let mut m = schemars::Map::new();
                        m.insert(
                            "name".to_string(),
                            Schema::Object(SchemaObject {
                                instance_type: Some(SingleOrVec::Vec(vec![
                                    InstanceType::String,
                                    InstanceType::Null,
                                ])),
                                ..Default::default()
                            }),
                        );
                        m.insert(
                            "count".to_string(),
                            Schema::Object(SchemaObject {
                                instance_type: Some(SingleOrVec::Single(Box::new(
                                    InstanceType::Integer,
                                ))),
                                ..Default::default()
                            }),
                        );
                        m
                    },
                    ..Default::default()
                })),
                ..Default::default()
            },
            ..Default::default()
        };

        crate::schema::normalize_schema(&mut root, &crate::schema::NormalizeOptions::gemini());

        let v = serde_json::to_value(&root).unwrap();
        assert_eq!(v["properties"]["name"]["type"], "string");
        assert_eq!(v["properties"]["name"]["nullable"], true);
        assert_eq!(v["properties"]["count"]["type"], "integer");
        assert!(v["properties"]["count"].get("nullable").is_none());
    }

    #[test]
    fn normalize_strips_format_and_constraints() {
        use schemars::schema::*;

        let mut root = RootSchema {
            schema: SchemaObject {
                instance_type: Some(SingleOrVec::Single(Box::new(InstanceType::Object))),
                object: Some(Box::new(ObjectValidation {
                    properties: {
                        let mut m = schemars::Map::new();
                        m.insert(
                            "ts".to_string(),
                            Schema::Object(SchemaObject {
                                instance_type: Some(SingleOrVec::Single(Box::new(
                                    InstanceType::String,
                                ))),
                                format: Some("date-time".to_string()),
                                ..Default::default()
                            }),
                        );
                        m.insert(
                            "count".to_string(),
                            Schema::Object(SchemaObject {
                                instance_type: Some(SingleOrVec::Single(Box::new(
                                    InstanceType::Integer,
                                ))),
                                format: Some("uint".to_string()),
                                number: Some(Box::new(NumberValidation {
                                    minimum: Some(0.0),
                                    ..Default::default()
                                })),
                                ..Default::default()
                            }),
                        );
                        m
                    },
                    ..Default::default()
                })),
                ..Default::default()
            },
            ..Default::default()
        };

        crate::schema::normalize_schema(&mut root, &crate::schema::NormalizeOptions::gemini());

        let v = serde_json::to_value(&root).unwrap();
        assert!(v["properties"]["ts"].get("format").is_none());
        assert!(v["properties"]["count"].get("format").is_none());
        assert!(v["properties"]["count"].get("minimum").is_none());
    }

    #[test]
    fn normalize_recurses_into_array_items() {
        use schemars::schema::*;

        let mut root = RootSchema {
            schema: SchemaObject {
                instance_type: Some(SingleOrVec::Single(Box::new(InstanceType::Object))),
                object: Some(Box::new(ObjectValidation {
                    properties: {
                        let mut m = schemars::Map::new();
                        m.insert(
                            "tags".to_string(),
                            Schema::Object(SchemaObject {
                                instance_type: Some(SingleOrVec::Single(Box::new(
                                    InstanceType::Array,
                                ))),
                                array: Some(Box::new(ArrayValidation {
                                    items: Some(SingleOrVec::Single(Box::new(
                                        Schema::Object(SchemaObject {
                                            instance_type: Some(SingleOrVec::Vec(vec![
                                                InstanceType::String,
                                                InstanceType::Null,
                                            ])),
                                            ..Default::default()
                                        }),
                                    ))),
                                    ..Default::default()
                                })),
                                ..Default::default()
                            }),
                        );
                        m
                    },
                    ..Default::default()
                })),
                ..Default::default()
            },
            ..Default::default()
        };

        crate::schema::normalize_schema(&mut root, &crate::schema::NormalizeOptions::gemini());

        let v = serde_json::to_value(&root).unwrap();
        let items = &v["properties"]["tags"]["items"];
        assert_eq!(items["type"], "string");
        assert_eq!(items["nullable"], true);
    }
}
