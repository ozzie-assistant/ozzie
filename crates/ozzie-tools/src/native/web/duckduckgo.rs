use ozzie_core::domain::ToolError;

use super::html::{decode_html_entities, strip_html_tags};
use super::search::SearchProvider;
use super::SearchResult;

/// DuckDuckGo HTML scraper search provider.
pub struct DuckDuckGoProvider {
    client: reqwest::Client,
}

impl DuckDuckGoProvider {
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::limited(5))
            .build()
            .unwrap_or_default();
        Self { client }
    }
}

#[async_trait::async_trait]
impl SearchProvider for DuckDuckGoProvider {
    async fn search(
        &self,
        query: &str,
        max_results: usize,
    ) -> Result<Vec<SearchResult>, ToolError> {
        let resp = self
            .client
            .post("https://html.duckduckgo.com/html/")
            .header("User-Agent", "Ozzie/1.0 (web_search)")
            .header("Accept", "text/html")
            .form(&[("q", query)])
            .send()
            .await
            .map_err(|e| ToolError::Execution(format!("web_search request failed: {e}")))?;

        if !resp.status().is_success() {
            return Err(ToolError::Execution(format!(
                "web_search: DuckDuckGo returned status {}",
                resp.status()
            )));
        }

        let html = resp
            .text()
            .await
            .map_err(|e| ToolError::Execution(format!("web_search: read response: {e}")))?;

        Ok(parse_results(&html, max_results))
    }
}

/// Parses DuckDuckGo HTML search results page.
fn parse_results(html: &str, max_results: usize) -> Vec<SearchResult> {
    let mut results = Vec::new();

    let parts: Vec<&str> = html.split("class=\"result__a\"").collect();

    for part in parts.iter().skip(1) {
        if results.len() >= max_results {
            break;
        }

        let url = extract_attr_value(part, "href=\"");
        let title = extract_tag_text(part);

        let snippet = if let Some(snippet_start) = part.find("class=\"result__snippet\"") {
            let snippet_part = &part[snippet_start..];
            let text = extract_tag_text(snippet_part);
            decode_html_entities(&strip_html_tags(&text))
        } else {
            String::new()
        };

        let url = decode_ddg_url(&url);
        if url.is_empty() || !url.starts_with("http") {
            continue;
        }

        let title = decode_html_entities(&strip_html_tags(&title));

        results.push(SearchResult {
            title,
            url,
            snippet,
        });
    }

    results
}

fn extract_attr_value(s: &str, prefix: &str) -> String {
    if let Some(start) = s.find(prefix) {
        let after = &s[start + prefix.len()..];
        if let Some(end) = after.find('"') {
            return after[..end].to_string();
        }
    }
    String::new()
}

fn extract_tag_text(s: &str) -> String {
    if let Some(start) = s.find('>') {
        let after = &s[start + 1..];
        if let Some(end) = after.find("</") {
            return after[..end].to_string();
        }
    }
    String::new()
}

/// Decodes DuckDuckGo redirect URLs (//duckduckgo.com/l/?uddg=...).
fn decode_ddg_url(url: &str) -> String {
    if url.contains("duckduckgo.com/l/?")
        && let Some(start) = url.find("uddg=") {
            let encoded = &url[start + 5..];
            let encoded = if let Some(end) = encoded.find('&') {
                &encoded[..end]
            } else {
                encoded
            };
            return url_decode(encoded);
        }
    url.to_string()
}

fn url_decode(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.bytes();
    while let Some(b) = chars.next() {
        if b == b'%' {
            let hi = chars.next().unwrap_or(b'0');
            let lo = chars.next().unwrap_or(b'0');
            let hex = [hi, lo];
            if let Ok(hex_str) = std::str::from_utf8(&hex)
                && let Ok(byte) = u8::from_str_radix(hex_str, 16)
            {
                result.push(byte as char);
                continue;
            }
            result.push('%');
            result.push(hi as char);
            result.push(lo as char);
        } else if b == b'+' {
            result.push(' ');
        } else {
            result.push(b as char);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_ddg_empty_html() {
        assert!(parse_results("", 10).is_empty());
    }

    #[test]
    fn parse_ddg_no_results() {
        let html = "<html><body><p>No results found</p></body></html>";
        assert!(parse_results(html, 10).is_empty());
    }

    #[test]
    fn parse_ddg_single_result() {
        let html = r#"
        <div class="result">
            <h2 class="result__title">
                <a rel="nofollow" class="result__a" href="https://example.com">Example Title</a>
            </h2>
            <a class="result__snippet" href="https://example.com">This is the snippet text.</a>
        </div>
        "#;
        let results = parse_results(html, 10);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Example Title");
        assert_eq!(results[0].url, "https://example.com");
        assert_eq!(results[0].snippet, "This is the snippet text.");
    }

    #[test]
    fn parse_ddg_multiple_results() {
        let html = r#"
        <a class="result__a" href="https://one.com">First</a>
        <a class="result__snippet" href="">Snippet one</a>
        <a class="result__a" href="https://two.com">Second</a>
        <a class="result__snippet" href="">Snippet two</a>
        <a class="result__a" href="https://three.com">Third</a>
        <a class="result__snippet" href="">Snippet three</a>
        "#;
        let results = parse_results(html, 2);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].title, "First");
        assert_eq!(results[1].title, "Second");
    }

    #[test]
    fn parse_ddg_redirect_url() {
        let html = r#"
        <a class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fexample.com%2Fpage&amp;rut=abc">Title</a>
        <a class="result__snippet" href="">Snippet</a>
        "#;
        let results = parse_results(html, 10);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].url, "https://example.com/page");
    }

    #[test]
    fn parse_ddg_html_entities() {
        let html = r#"
        <a class="result__a" href="https://example.com">Rust &amp; Go</a>
        <a class="result__snippet" href="">A &lt;great&gt; comparison</a>
        "#;
        let results = parse_results(html, 10);
        assert_eq!(results[0].title, "Rust & Go");
        assert_eq!(results[0].snippet, "A <great> comparison");
    }

    #[test]
    fn parse_ddg_skips_non_http_urls() {
        let html = r#"
        <a class="result__a" href="javascript:void(0)">Bad Link</a>
        <a class="result__snippet" href="">Bad snippet</a>
        <a class="result__a" href="https://good.com">Good Link</a>
        <a class="result__snippet" href="">Good snippet</a>
        "#;
        let results = parse_results(html, 10);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].url, "https://good.com");
    }

    #[test]
    fn url_decode_basic() {
        assert_eq!(
            url_decode("https%3A%2F%2Fexample.com"),
            "https://example.com"
        );
    }

    #[test]
    fn url_decode_plus_as_space() {
        assert_eq!(url_decode("hello+world"), "hello world");
    }

    #[test]
    fn url_decode_passthrough() {
        assert_eq!(url_decode("noencode"), "noencode");
    }

    #[test]
    fn decode_ddg_url_direct() {
        assert_eq!(decode_ddg_url("https://example.com"), "https://example.com");
    }

    #[test]
    fn decode_ddg_url_redirect() {
        let url = "//duckduckgo.com/l/?uddg=https%3A%2F%2Fexample.com&rut=abc";
        assert_eq!(decode_ddg_url(url), "https://example.com");
    }
}
