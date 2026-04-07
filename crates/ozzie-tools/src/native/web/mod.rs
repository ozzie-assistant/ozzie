mod duckduckgo;
mod fetch;
mod html;
mod search;

pub use fetch::WebFetchTool;
pub use search::WebSearchTool;

/// A single search result.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SearchResult {
    pub title: String,
    pub url: String,
    pub snippet: String,
}
