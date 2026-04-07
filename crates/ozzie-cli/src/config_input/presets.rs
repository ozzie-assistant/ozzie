//! Wizard presets — loaded from `model_catalog.yaml` at compile time.
//!
//! The YAML catalog is the single source of truth for model names,
//! context windows, and default capabilities.

use std::collections::HashMap;
use std::sync::LazyLock;

use ozzie_core::config::Driver;
use ozzie_core::domain::ModelCapability;
use serde::Deserialize;

// ── Catalog types ─────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct ModelEntry {
    name: String,
    context_window: usize,
    #[serde(default)]
    capabilities: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct DriverEntry {
    default: String,
    models: Vec<ModelEntry>,
}

type RawCatalog = HashMap<String, DriverEntry>;

struct Catalog {
    entries: HashMap<String, DriverEntry>,
}

impl Catalog {
    fn load() -> Self {
        let raw: RawCatalog =
            serde_yaml::from_str(include_str!("model_catalog.yaml")).expect("embedded model_catalog.yaml must be valid YAML");
        Self { entries: raw }
    }

    fn driver_entry(&self, driver: Driver) -> Option<&DriverEntry> {
        self.entries.get(driver.as_str())
    }

    fn model_entry(&self, driver: Driver, model: &str) -> Option<&ModelEntry> {
        self.driver_entry(driver)?
            .models
            .iter()
            .find(|m| m.name == model)
    }
}

static CATALOG: LazyLock<Catalog> = LazyLock::new(Catalog::load);

// ── Public API (same signatures as before) ────────────────────────────────

/// Returns default capabilities for known provider models.
pub fn default_capabilities(driver: Driver, model: &str) -> &'static [ModelCapability] {
    // Try exact match first
    if let Some(entry) = CATALOG.model_entry(driver, model) {
        return parse_capabilities_static(&entry.capabilities);
    }
    // Try substring match within the driver's models
    if let Some(driver_entry) = CATALOG.driver_entry(driver) {
        for m in &driver_entry.models {
            if model.contains(&m.name) || m.name.contains(model) {
                return parse_capabilities_static(&m.capabilities);
            }
        }
    }
    &[]
}

/// LLM model presets per driver.
pub fn model_presets(driver: Driver) -> Vec<&'static str> {
    CATALOG
        .driver_entry(driver)
        .map(|e| e.models.iter().map(|m| m.name.as_str()).collect())
        .unwrap_or_default()
}

/// Default model for a driver (used when custom model field is left empty).
pub fn default_model_for(driver: Driver) -> &'static str {
    CATALOG
        .driver_entry(driver)
        .map(|e| e.default.as_str())
        .unwrap_or("")
}

/// Context window for a known model (from catalog). Returns `None` for custom models.
pub fn default_context_window(driver: Driver, model: &str) -> Option<usize> {
    // Exact match
    if let Some(entry) = CATALOG.model_entry(driver, model) {
        return Some(entry.context_window);
    }
    // Substring match
    if let Some(driver_entry) = CATALOG.driver_entry(driver) {
        for m in &driver_entry.models {
            if model.contains(&m.name) || m.name.contains(model) {
                return Some(m.context_window);
            }
        }
    }
    None
}

/// Embedding model presets: (model_name, dimensions).
pub fn embedding_presets(driver: Driver) -> &'static [(&'static str, usize)] {
    match driver {
        Driver::OpenAi => &[
            ("text-embedding-3-small", 1536),
            ("text-embedding-3-large", 3072),
            ("text-embedding-ada-002", 1536),
        ],
        Driver::Mistral => &[("mistral-embed", 1024)],
        Driver::Gemini => &[("text-embedding-004", 768), ("embedding-001", 768)],
        Driver::Ollama => &[
            ("nomic-embed-text", 768),
            ("mxbai-embed-large", 1024),
            ("all-minilm", 384),
        ],
        _ => &[],
    }
}

// ── Internals ─────────────────────────────────────────────────────────────

/// Converts capability strings to a static slice.
///
/// Uses leaked allocations — acceptable since the catalog is loaded once at startup.
fn parse_capabilities_static(caps: &[String]) -> &'static [ModelCapability] {
    static CACHE: LazyLock<std::sync::Mutex<HashMap<Vec<String>, &'static [ModelCapability]>>> =
        LazyLock::new(|| std::sync::Mutex::new(HashMap::new()));

    let key: Vec<String> = caps.to_vec();
    let mut cache = CACHE.lock().unwrap();
    if let Some(&cached) = cache.get(&key) {
        return cached;
    }

    let parsed: Vec<ModelCapability> = caps
        .iter()
        .filter_map(|s| s.parse::<ModelCapability>().ok())
        .collect();
    let leaked: &'static [ModelCapability] = Box::leak(parsed.into_boxed_slice());
    cache.insert(key, leaked);
    leaked
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_loads() {
        // Force load
        let _ = &*CATALOG;
        assert!(CATALOG.driver_entry(Driver::Anthropic).is_some());
    }

    #[test]
    fn default_caps_anthropic_sonnet() {
        let caps = default_capabilities(Driver::Anthropic, "claude-sonnet-4-20250514");
        assert!(caps.contains(&ModelCapability::Thinking));
        assert!(caps.contains(&ModelCapability::Vision));
        assert!(caps.contains(&ModelCapability::ToolUse));
    }

    #[test]
    fn default_caps_anthropic_haiku() {
        let caps = default_capabilities(Driver::Anthropic, "claude-haiku-4-20250506");
        assert!(caps.contains(&ModelCapability::Fast));
        assert!(caps.contains(&ModelCapability::Cheap));
        assert!(!caps.contains(&ModelCapability::Thinking));
    }

    #[test]
    fn default_caps_unknown_model() {
        let caps = default_capabilities(Driver::Vllm, "some-model");
        assert!(caps.is_empty());
    }

    #[test]
    fn default_caps_ollama_deepseek() {
        let caps = default_capabilities(Driver::Ollama, "deepseek-r1:8b");
        assert!(caps.contains(&ModelCapability::Thinking));
        assert!(caps.contains(&ModelCapability::Coding));
    }

    #[test]
    fn model_presets_anthropic() {
        let presets = model_presets(Driver::Anthropic);
        assert!(!presets.is_empty());
        assert!(presets[0].contains("sonnet"));
    }

    #[test]
    fn embedding_presets_openai() {
        let presets = embedding_presets(Driver::OpenAi);
        assert!(!presets.is_empty());
        assert_eq!(presets[0].0, "text-embedding-3-small");
    }

    #[test]
    fn context_window_opus() {
        let cw = default_context_window(Driver::Anthropic, "claude-opus-4-20250514");
        assert_eq!(cw, Some(1_000_000));
    }

    #[test]
    fn context_window_qwen_groq() {
        let cw = default_context_window(Driver::Groq, "qwen/qwen3-32b");
        assert_eq!(cw, Some(131072));
    }

    #[test]
    fn context_window_unknown() {
        let cw = default_context_window(Driver::Vllm, "my-custom-model");
        assert_eq!(cw, None);
    }
}
