use ozzie_core::domain::{Tool, ToolError, ToolInfo};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::registry::{schema_for, ToolSpec};

use super::html::strip_html_tags;

const MAX_BODY_SIZE: usize = 512 * 1024; // 512 KB

/// Extracts readable text from HTML content.
///
/// Implementations can range from simple tag stripping to full DOM-based
/// extraction (e.g. readability algorithm).
pub trait HtmlReader: Send + Sync {
    fn extract_text(&self, html: &str) -> String;
}

/// Default reader: strips HTML tags and collapses whitespace.
pub struct StripTagsReader;

impl HtmlReader for StripTagsReader {
    fn extract_text(&self, html: &str) -> String {
        strip_html_tags(html)
    }
}

/// Fetches web pages and extracts text content.
pub struct WebFetchTool {
    client: reqwest::Client,
    reader: Box<dyn HtmlReader>,
}

impl WebFetchTool {
    pub fn new() -> Self {
        Self::with_reader(Box::new(StripTagsReader))
    }

    pub fn with_reader(reader: Box<dyn HtmlReader>) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::limited(5))
            .build()
            .unwrap_or_default();
        Self { client, reader }
    }

    pub fn spec() -> ToolSpec {
        ToolSpec {
            name: "web_fetch".to_string(),
            description: "Fetch a web page and extract its text content".to_string(),
            parameters: schema_for::<WebFetchArgs>(),
            dangerous: false,
        }
    }
}

impl Default for WebFetchTool {
    fn default() -> Self {
        Self::new()
    }
}

/// Arguments for web_fetch.
#[derive(Deserialize, JsonSchema)]
struct WebFetchArgs {
    /// URL to fetch.
    url: String,
    /// Maximum response size in bytes (default: 512KB).
    #[serde(default)]
    max_size: Option<usize>,
}

#[derive(Serialize)]
struct WebFetchResult {
    url: String,
    status: u16,
    content: String,
}

#[async_trait::async_trait]
impl Tool for WebFetchTool {
    fn info(&self) -> ToolInfo {
        ToolInfo::with_parameters(
            "web_fetch",
            "Fetch a web page and extract text",
            WebFetchTool::spec().parameters,
        )
    }

    async fn run(&self, arguments_json: &str) -> Result<String, ToolError> {
        let args: WebFetchArgs = serde_json::from_str(arguments_json)
            .map_err(|e| ToolError::Execution(format!("invalid arguments: {e}")))?;

        let max_size = args.max_size.unwrap_or(MAX_BODY_SIZE);

        ozzie_core::domain::emit_progress("", "web_fetch", &format!("fetching {}", args.url));

        let resp = self
            .client
            .get(&args.url)
            .send()
            .await
            .map_err(|e| ToolError::Execution(format!("fetch '{}': {e}", args.url)))?;

        let status = resp.status().as_u16();
        let content_type = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_lowercase();

        let body = resp
            .text()
            .await
            .map_err(|e| ToolError::Execution(format!("read body: {e}")))?;

        let truncated = if body.len() > max_size {
            format!("{}...(truncated)", &body[..max_size])
        } else {
            body
        };

        // Only strip HTML if the response is actually HTML.
        // JSON, plain text, XML, etc. are passed through as-is.
        let is_html = content_type.contains("text/html")
            || (content_type.is_empty() && looks_like_html(&truncated));

        let content = if is_html {
            self.reader.extract_text(&truncated)
        } else {
            truncated
        };

        let result = WebFetchResult {
            url: args.url,
            status,
            content,
        };

        serde_json::to_string(&result)
            .map_err(|e| ToolError::Execution(format!("serialize result: {e}")))
    }
}

/// Heuristic: content looks like HTML if it starts with `<`.
fn looks_like_html(s: &str) -> bool {
    let trimmed = s.trim_start();
    trimmed.starts_with('<')
}
