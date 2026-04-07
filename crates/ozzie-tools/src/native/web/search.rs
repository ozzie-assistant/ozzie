use ozzie_core::domain::{Tool, ToolError, ToolInfo};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::registry::{schema_for, ToolSpec};

use super::SearchResult;

/// Provides web search results from an external engine.
///
/// Implementations: DuckDuckGo (default), Google, Bing, etc.
#[async_trait::async_trait]
pub trait SearchProvider: Send + Sync {
    async fn search(&self, query: &str, max_results: usize) -> Result<Vec<SearchResult>, ToolError>;
}

const DEFAULT_MAX_RESULTS: usize = 10;

/// Searches the web and returns structured results.
pub struct WebSearchTool {
    provider: Box<dyn SearchProvider>,
    max_results: usize,
}

impl WebSearchTool {
    pub fn new() -> Self {
        Self::with_provider(
            Box::new(super::duckduckgo::DuckDuckGoProvider::new()),
            DEFAULT_MAX_RESULTS,
        )
    }

    pub fn with_provider(provider: Box<dyn SearchProvider>, max_results: usize) -> Self {
        Self {
            provider,
            max_results,
        }
    }

    pub fn spec() -> ToolSpec {
        ToolSpec {
            name: "web_search".to_string(),
            description:
                "Search the web for current information. Returns titles, URLs, and snippets."
                    .to_string(),
            parameters: schema_for::<WebSearchArgs>(),
            dangerous: false,
        }
    }
}

impl Default for WebSearchTool {
    fn default() -> Self {
        Self::new()
    }
}

/// Arguments for web_search.
#[derive(Deserialize, JsonSchema)]
struct WebSearchArgs {
    /// The search query.
    query: String,
}

#[derive(Serialize)]
struct WebSearchOutput {
    query: String,
    results: Vec<SearchResult>,
}

#[async_trait::async_trait]
impl Tool for WebSearchTool {
    fn info(&self) -> ToolInfo {
        ToolInfo::with_parameters(
            "web_search",
            "Search the web for current information",
            WebSearchTool::spec().parameters,
        )
    }

    async fn run(&self, arguments_json: &str) -> Result<String, ToolError> {
        let args: WebSearchArgs = serde_json::from_str(arguments_json)
            .map_err(|e| ToolError::Execution(format!("invalid arguments: {e}")))?;

        if args.query.trim().is_empty() {
            return Err(ToolError::Execution(
                "web_search: query is required".to_string(),
            ));
        }

        let results = self.provider.search(&args.query, self.max_results).await?;

        let output = WebSearchOutput {
            query: args.query,
            results,
        };

        serde_json::to_string(&output)
            .map_err(|e| ToolError::Execution(format!("serialize result: {e}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn web_search_spec_name() {
        let spec = WebSearchTool::spec();
        assert_eq!(spec.name, "web_search");
        assert!(!spec.dangerous);
    }

    #[test]
    fn web_search_spec_has_query_param() {
        let spec = WebSearchTool::spec();
        let v = serde_json::to_value(&spec.parameters).unwrap();
        let props = &v["properties"];
        assert!(props.get("query").is_some());
        let required = v["required"].as_array().unwrap();
        assert!(required.contains(&serde_json::json!("query")));
    }

    #[test]
    fn web_search_info() {
        let tool = WebSearchTool::new();
        let info = tool.info();
        assert_eq!(info.name, "web_search");
    }

    #[tokio::test]
    async fn web_search_missing_query() {
        let tool = WebSearchTool::new();
        let result = tool.run(r#"{"query": ""}"#).await;
        assert!(result.unwrap_err().to_string().contains("query is required"));
    }

    #[tokio::test]
    async fn web_search_invalid_json() {
        let tool = WebSearchTool::new();
        let result = tool.run("not json").await;
        assert!(result.unwrap_err().to_string().contains("invalid arguments"));
    }

    #[tokio::test]
    async fn web_search_missing_field() {
        let tool = WebSearchTool::new();
        let result = tool.run(r#"{}"#).await;
        assert!(result.is_err());
    }
}
