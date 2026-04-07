//! Shared helpers for LLM provider implementations.
//!
//! Extracts the common HTTP request/response flow and streaming SSE/NdJson
//! parsing so that each provider only implements the provider-specific parts.

use std::collections::VecDeque;
use std::pin::Pin;

use futures_core::Stream;
use futures_util::StreamExt;

use crate::{ChatDelta, ChatResponse, LlmError};

/// Line protocol for streaming responses.
#[derive(Debug, Clone, Copy)]
pub enum LineProtocol {
    /// SSE: lines prefixed with `data: ` (OpenAI, Anthropic, Gemini).
    Sse,
    /// Newline-delimited JSON: each line is a complete JSON object (Ollama).
    NdJson,
}

/// Sends a JSON POST request, checks HTTP status, reads body, parses JSON,
/// and maps the response using a provider-supplied closure.
pub async fn post_and_parse<Req, Resp, F>(
    client: &reqwest::Client,
    url: &str,
    headers: reqwest::header::HeaderMap,
    body: &Req,
    provider_name: &str,
    map_response: F,
) -> Result<ChatResponse, LlmError>
where
    Req: serde::Serialize,
    Resp: serde::de::DeserializeOwned,
    F: FnOnce(Resp) -> Result<ChatResponse, LlmError>,
{
    if let Ok(json) = serde_json::to_string_pretty(body) {
        tracing::debug!(
            target: "ozzie_llm",
            provider = provider_name,
            url,
            bytes = json.len(),
            "request body:\n{json}"
        );
    }

    let resp = client
        .post(url)
        .headers(headers)
        .json(body)
        .send()
        .await
        .map_err(|e| LlmError::Connection(e.to_string()))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(LlmError::classify(&format!("{status}: {text}")));
    }

    let text = resp
        .text()
        .await
        .map_err(|e| LlmError::Other(format!("read response body: {e}")))?;

    let api_resp: Resp = serde_json::from_str(&text).map_err(|e| {
        tracing::error!(provider = provider_name, body = %text, "response parse failed");
        LlmError::Other(format!("parse response: {e}"))
    })?;

    map_response(api_resp)
}

/// Sends a streaming JSON POST request and returns a `ChatDelta` stream.
///
/// `parse_line` receives the payload of each line (after SSE prefix stripping
/// or raw for NdJson). Return `None` to signal end-of-stream, `Some(deltas)`
/// to emit zero or more deltas.
///
/// `on_finish` is called when the byte stream ends, allowing providers to flush
/// pending state (e.g. emit a final `Done` delta if the stream was cut before
/// the usage event arrived).
pub async fn send_and_stream<Req, F, Fin>(
    client: &reqwest::Client,
    url: &str,
    headers: reqwest::header::HeaderMap,
    body: &Req,
    protocol: LineProtocol,
    parse_line: F,
    on_finish: Fin,
) -> Result<Pin<Box<dyn Stream<Item = Result<ChatDelta, LlmError>> + Send>>, LlmError>
where
    Req: serde::Serialize,
    F: FnMut(&str) -> Option<Vec<ChatDelta>> + Send + 'static,
    Fin: FnOnce() -> Vec<ChatDelta> + Send + 'static,
{
    let resp = client
        .post(url)
        .headers(headers)
        .json(body)
        .send()
        .await
        .map_err(|e| LlmError::Connection(e.to_string()))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(LlmError::classify(&format!("{status}: {text}")));
    }

    Ok(stream_from_bytes(
        box_byte_stream(resp),
        protocol,
        parse_line,
        on_finish,
    ))
}

/// Type alias for a boxed byte stream (erases the concrete `bytes_stream()` type).
type ByteStream = Pin<Box<dyn Stream<Item = Result<Vec<u8>, String>> + Send>>;

/// Boxes a `reqwest::Response::bytes_stream()` into our type-erased `ByteStream`.
fn box_byte_stream(resp: reqwest::Response) -> ByteStream {
    use futures_util::TryStreamExt;
    Box::pin(
        resp.bytes_stream()
            .map_ok(|b| b.to_vec())
            .map_err(|e| e.to_string()),
    )
}

/// Creates a `ChatDelta` stream from a raw byte stream using the
/// unfold+buffer pattern with pending delta queue.
fn stream_from_bytes<F, Fin>(
    byte_stream: ByteStream,
    protocol: LineProtocol,
    parse_line: F,
    on_finish: Fin,
) -> Pin<Box<dyn Stream<Item = Result<ChatDelta, LlmError>> + Send>>
where
    F: FnMut(&str) -> Option<Vec<ChatDelta>> + Send + 'static,
    Fin: FnOnce() -> Vec<ChatDelta> + Send + 'static,
{
    let stream = futures_util::stream::unfold(
        (byte_stream, String::new(), VecDeque::<ChatDelta>::new(), parse_line, Some(on_finish)),
        move |(mut byte_stream, mut buffer, mut pending, mut parse, mut finish)| async move {
            loop {
                // Drain pending deltas first.
                if let Some(delta) = pending.pop_front() {
                    return Some((Ok(delta), (byte_stream, buffer, pending, parse, finish)));
                }

                // Process buffered lines.
                while let Some(line_end) = buffer.find('\n') {
                    let line = buffer[..line_end].trim().to_string();
                    buffer = buffer[line_end + 1..].to_string();

                    let data = match protocol {
                        LineProtocol::Sse => match line.strip_prefix("data: ") {
                            Some(d) => d.trim().to_string(),
                            None => continue,
                        },
                        LineProtocol::NdJson => {
                            if line.is_empty() {
                                continue;
                            }
                            line
                        }
                    };

                    match parse(&data) {
                        None => {
                            // parse_line signaled logical end — flush pending state.
                            if let Some(f) = finish.take() {
                                pending.extend(f());
                            }
                            if let Some(delta) = pending.pop_front() {
                                return Some((Ok(delta), (byte_stream, buffer, pending, parse, finish)));
                            }
                            return None;
                        }
                        Some(deltas) => pending.extend(deltas),
                    }

                    if let Some(delta) = pending.pop_front() {
                        return Some((Ok(delta), (byte_stream, buffer, pending, parse, finish)));
                    }
                }

                // Read more bytes.
                match byte_stream.next().await {
                    Some(Ok(chunk)) => {
                        buffer.push_str(&String::from_utf8_lossy(&chunk));
                    }
                    Some(Err(e)) => {
                        return Some((
                            Err(LlmError::Connection(e)),
                            (byte_stream, buffer, pending, parse, finish),
                        ));
                    }
                    None => {
                        // Byte stream ended — flush pending state.
                        if let Some(f) = finish.take() {
                            pending.extend(f());
                        }
                        if let Some(delta) = pending.pop_front() {
                            return Some((Ok(delta), (byte_stream, buffer, pending, parse, finish)));
                        }
                        return None;
                    }
                }
            }
        },
    );

    Box::pin(stream)
}
