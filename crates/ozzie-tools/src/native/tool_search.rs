use std::sync::Arc;

use ozzie_core::domain::{Tool, ToolError, ToolInfo};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::registry::{schema_for, ToolRegistry, ToolSpec};

/// Searches registered tools by keyword or exact name.
///
/// Use `select:name1,name2` for exact picks (no fuzzy — immune to LLM
/// hallucination). Use free-text keywords for discovery. Prefix a term
/// with `+` to make it required.
///
/// This tool only returns information — use `activate` to make a tool
/// available for use.
pub struct ToolSearchTool {
    registry: Arc<ToolRegistry>,
    /// Names of tools that are always sent to the LLM (not deferred).
    core_names: Vec<String>,
}

impl ToolSearchTool {
    pub fn new(registry: Arc<ToolRegistry>, core_names: Vec<String>) -> Self {
        Self {
            registry,
            core_names,
        }
    }

    pub fn spec() -> ToolSpec {
        ToolSpec {
            name: "tool_search".to_string(),
            description: "Search for available tools by keyword or exact name. \
                Use \"select:name1,name2\" for exact picks, or free-text keywords \
                for discovery. Returns matching tool names and descriptions. \
                Use `activate` to make a found tool available."
                .to_string(),
            parameters: schema_for::<ToolSearchInput>(),
            dangerous: false,
        }
    }

    fn deferred_specs(&self) -> Vec<ToolSpec> {
        self.registry
            .all_specs()
            .into_iter()
            .filter(|s| !self.core_names.contains(&s.name))
            .collect()
    }
}

#[derive(Deserialize, JsonSchema)]
struct ToolSearchInput {
    /// Search query. Use "select:name1,name2" for exact selection,
    /// or keywords for fuzzy search. Prefix a term with "+" to require it.
    query: String,
    /// Maximum results to return (default: 5).
    #[serde(default)]
    max_results: Option<usize>,
}

#[derive(Serialize)]
struct ToolSearchOutput {
    matches: Vec<ToolMatch>,
    query: String,
    total_searchable: usize,
}

#[derive(Serialize)]
struct ToolMatch {
    name: String,
    description: String,
}

#[async_trait::async_trait]
impl Tool for ToolSearchTool {
    fn info(&self) -> ToolInfo {
        ToolInfo::with_parameters(
            "tool_search",
            "Search for available tools by keyword or exact name",
            ToolSearchTool::spec().parameters,
        )
    }

    async fn run(&self, arguments_json: &str) -> Result<String, ToolError> {
        let input: ToolSearchInput = serde_json::from_str(arguments_json)
            .map_err(|e| ToolError::Execution(format!("tool_search: parse input: {e}")))?;

        let max_results = input.max_results.unwrap_or(5).clamp(1, 20);
        let specs = self.deferred_specs();
        let matches = search(&input.query, max_results, &specs);

        let output = ToolSearchOutput {
            query: input.query,
            total_searchable: specs.len(),
            matches,
        };

        serde_json::to_string(&output)
            .map_err(|e| ToolError::Execution(format!("tool_search: serialize: {e}")))
    }
}

fn search(query: &str, max_results: usize, specs: &[ToolSpec]) -> Vec<ToolMatch> {
    let query = query.trim();
    let lowered = query.to_ascii_lowercase();

    // Exact selection: "select:name1,name2"
    if let Some(selection) = lowered.strip_prefix("select:") {
        return selection
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .filter_map(|wanted| {
                let canonical = normalize(wanted);
                specs
                    .iter()
                    .find(|s| normalize(&s.name) == canonical)
                    .map(|s| ToolMatch {
                        name: s.name.clone(),
                        description: s.description.clone(),
                    })
            })
            .take(max_results)
            .collect();
    }

    // Keyword search with optional required terms (+term).
    let mut required = Vec::new();
    let mut optional = Vec::new();
    for term in lowered.split_whitespace() {
        if let Some(rest) = term.strip_prefix('+') {
            if !rest.is_empty() {
                required.push(rest.to_string());
            }
        } else {
            optional.push(term.to_string());
        }
    }
    let all_terms: Vec<&str> = required
        .iter()
        .chain(optional.iter())
        .map(|s| s.as_str())
        .collect();

    if all_terms.is_empty() {
        return Vec::new();
    }

    let mut scored: Vec<(i32, &ToolSpec)> = specs
        .iter()
        .filter_map(|spec| {
            let name_lower = spec.name.to_ascii_lowercase();
            let name_norm = normalize(&spec.name);
            let desc_lower = spec.description.to_ascii_lowercase();
            let haystack = format!("{name_lower} {desc_lower}");

            // Required terms must all appear.
            if required.iter().any(|t| !haystack.contains(t.as_str())) {
                return None;
            }

            let mut score = 0i32;
            for term in &all_terms {
                let term_norm = normalize(term);
                if name_lower == *term {
                    score += 10;
                } else if name_lower.contains(term) {
                    score += 5;
                }
                if name_norm == term_norm {
                    score += 12;
                }
                if desc_lower.contains(term) {
                    score += 2;
                }
            }

            (score > 0).then_some((score, spec))
        })
        .collect();

    scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.name.cmp(&b.1.name)));
    scored
        .into_iter()
        .take(max_results)
        .map(|(_, spec)| ToolMatch {
            name: spec.name.clone(),
            description: spec.description.clone(),
        })
        .collect()
}

/// Strips non-alphanumeric, lowercases.
fn normalize(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use schemars::schema::{InstanceType, RootSchema, SchemaObject, SingleOrVec};

    fn empty_schema() -> RootSchema {
        RootSchema {
            schema: SchemaObject {
                instance_type: Some(SingleOrVec::Single(Box::new(InstanceType::Object))),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    fn specs() -> Vec<ToolSpec> {
        vec![
            ToolSpec {
                name: "docker_build".into(),
                description: "Build Docker images from Dockerfile".into(),
                parameters: empty_schema(),
                dangerous: false,
            },
            ToolSpec {
                name: "web_fetch".into(),
                description: "Fetch content from a URL".into(),
                parameters: empty_schema(),
                dangerous: false,
            },
            ToolSpec {
                name: "web_search".into(),
                description: "Search the web using DuckDuckGo".into(),
                parameters: empty_schema(),
                dangerous: false,
            },
            ToolSpec {
                name: "schedule_task".into(),
                description: "Schedule a recurring task".into(),
                parameters: empty_schema(),
                dangerous: false,
            },
        ]
    }

    #[test]
    fn select_exact_names() {
        let results = search("select:docker_build,web_fetch", 5, &specs());
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].name, "docker_build");
        assert_eq!(results[1].name, "web_fetch");
    }

    #[test]
    fn select_case_insensitive() {
        let results = search("select:Docker_Build", 5, &specs());
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "docker_build");
    }

    #[test]
    fn select_unknown_name_skipped() {
        let results = search("select:nonexistent,web_fetch", 5, &specs());
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "web_fetch");
    }

    #[test]
    fn keyword_search_by_name() {
        let results = search("docker", 5, &specs());
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "docker_build");
    }

    #[test]
    fn keyword_search_by_description() {
        let results = search("recurring", 5, &specs());
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "schedule_task");
    }

    #[test]
    fn keyword_search_web_returns_both() {
        let results = search("web", 5, &specs());
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn required_term_filters() {
        let results = search("+web fetch", 5, &specs());
        // Both web tools match "+web", but only web_fetch has "fetch" in desc
        assert_eq!(results.len(), 2);
        // web_fetch scores higher (name contains "fetch" + desc contains "fetch")
        assert_eq!(results[0].name, "web_fetch");
    }

    #[test]
    fn empty_query_returns_nothing() {
        let results = search("", 5, &specs());
        assert!(results.is_empty());
    }

    #[test]
    fn max_results_respected() {
        let results = search("web", 1, &specs());
        assert_eq!(results.len(), 1);
    }
}
