use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use ozzie_core::domain::{Tool, ToolInfo, ToolLookup};
use schemars::schema::RootSchema;

/// Tool specification describing name, description, and parameter JSON Schema.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    /// Full JSON Schema for the tool's input parameters.
    /// Generated via `schema_for::<ArgsStruct>()`.
    pub parameters: RootSchema,
    pub dangerous: bool,
}

/// Generates a typed JSON Schema from a `schemars::JsonSchema` type.
pub fn schema_for<T: schemars::JsonSchema>() -> RootSchema {
    let settings = schemars::r#gen::SchemaSettings::draft07().with(|s| {
        s.inline_subschemas = true;
    });
    let generator = settings.into_generator();
    generator.into_root_schema_for::<T>()
}

// ---- Tool Registry ----

/// Thread-safe registry managing tools by name.
///
/// Tool lookups normalize names (lowercase, dashes to underscores) and support
/// short aliases (e.g. `"shell"` → `"shell_exec"`).
pub struct ToolRegistry {
    tools: RwLock<HashMap<String, RegistryEntry>>,
    /// Short alias → canonical name (e.g. `"shell"` → `"shell_exec"`).
    aliases: RwLock<HashMap<String, String>>,
}

struct RegistryEntry {
    tool: Arc<dyn Tool>,
    spec: ToolSpec,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: RwLock::new(HashMap::new()),
            aliases: RwLock::new(HashMap::new()),
        }
    }

    /// Registers a short alias that maps to a canonical tool name.
    pub fn add_alias(&self, alias: &str, canonical: &str) {
        let mut aliases = self.aliases.write().unwrap();
        aliases.insert(normalize_tool_name(alias), canonical.to_string());
    }

    /// Resolves a tool name through normalization and alias lookup.
    ///
    /// 1. Try exact match
    /// 2. Normalize (lowercase + dashes→underscores) and try again
    /// 3. Check alias table
    fn resolve_name(&self, name: &str, tools: &HashMap<String, RegistryEntry>) -> Option<String> {
        if tools.contains_key(name) {
            return Some(name.to_string());
        }
        let normalized = normalize_tool_name(name);
        if tools.contains_key(&normalized) {
            return Some(normalized);
        }
        let aliases = self.aliases.read().unwrap();
        aliases.get(&normalized).cloned()
    }

    /// Registers a native tool with an explicit spec.
    pub fn register(&self, tool: Box<dyn Tool>, spec: ToolSpec) {
        let name = spec.name.clone();
        let mut tools = self.tools.write().unwrap();
        tools.insert(
            name,
            RegistryEntry {
                tool: Arc::from(tool),
                spec,
            },
        );
    }

    /// Returns all registered tools as Arc<dyn Tool> (for ReactLoop integration).
    pub fn all_tools(&self) -> Vec<Arc<dyn Tool>> {
        let tools = self.tools.read().unwrap();
        tools.values().map(|e| e.tool.clone()).collect()
    }

    /// Returns a tool by name (supports normalized names and aliases).
    pub fn get(&self, name: &str) -> Option<ToolInfo> {
        let tools = self.tools.read().unwrap();
        let resolved = self.resolve_name(name, &tools)?;
        tools.get(&resolved).map(|entry| entry.tool.info())
    }

    /// Returns the spec for a tool (supports normalized names and aliases).
    pub fn spec(&self, name: &str) -> Option<ToolSpec> {
        let tools = self.tools.read().unwrap();
        let resolved = self.resolve_name(name, &tools)?;
        tools.get(&resolved).map(|entry| entry.spec.clone())
    }

    /// Returns all registered specs.
    pub fn all_specs(&self) -> Vec<ToolSpec> {
        let tools = self.tools.read().unwrap();
        tools.values().map(|e| e.spec.clone()).collect()
    }

    /// Returns all registered tool names.
    pub fn names(&self) -> Vec<String> {
        let tools = self.tools.read().unwrap();
        tools.keys().cloned().collect()
    }

    /// Returns descriptions for all registered tools.
    pub fn all_descriptions(&self) -> HashMap<String, String> {
        let tools = self.tools.read().unwrap();
        tools
            .iter()
            .map(|(name, entry)| (name.clone(), entry.spec.description.clone()))
            .collect()
    }

    /// Returns whether a tool is marked as dangerous (supports normalized names and aliases).
    pub fn is_dangerous(&self, name: &str) -> bool {
        let tools = self.tools.read().unwrap();
        let Some(resolved) = self.resolve_name(name, &tools) else {
            return false;
        };
        tools.get(&resolved).is_some_and(|e| e.spec.dangerous)
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Normalizes a tool name: lowercase, dashes → underscores.
fn normalize_tool_name(name: &str) -> String {
    name.trim().replace('-', "_").to_ascii_lowercase()
}

impl ToolLookup for ToolRegistry {
    fn tools_by_names(&self, names: &[String]) -> Vec<Box<dyn Tool>> {
        let tools = self.tools.read().unwrap();
        names
            .iter()
            .filter_map(|name| {
                let resolved = self.resolve_name(name, &tools)?;
                tools.get(&resolved).map(|entry| {
                    let info = entry.tool.info();
                    Box::new(ToolProxy {
                        name: info.name.clone(),
                        description: info.description.clone(),
                    }) as Box<dyn Tool>
                })
            })
            .collect()
    }

    fn tool_names(&self) -> Vec<String> {
        self.names()
    }
}

/// Lightweight proxy returned from ToolLookup for tool metadata.
struct ToolProxy {
    name: String,
    description: String,
}

#[async_trait::async_trait]
impl Tool for ToolProxy {
    fn info(&self) -> ToolInfo {
        ToolInfo::new(self.name.clone(), self.description.clone())
    }

    async fn run(&self, _arguments_json: &str) -> Result<String, ozzie_core::domain::ToolError> {
        Err(ozzie_core::domain::ToolError::Execution(
            "ToolProxy::run should not be called directly; use ToolRegistry".to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ozzie_core::domain::ToolError;

    struct DummyTool;

    #[async_trait::async_trait]
    impl Tool for DummyTool {
        fn info(&self) -> ToolInfo {
            ToolInfo::new("dummy", "A dummy tool")
        }

        async fn run(&self, args: &str) -> Result<String, ToolError> {
            Ok(format!("executed with: {args}"))
        }
    }

    fn dummy_spec(name: &str, dangerous: bool) -> ToolSpec {
        use schemars::schema::{InstanceType, RootSchema, SchemaObject, SingleOrVec};
        ToolSpec {
            name: name.to_string(),
            description: name.to_string(),
            parameters: RootSchema {
                schema: SchemaObject {
                    instance_type: Some(SingleOrVec::Single(Box::new(InstanceType::Object))),
                    ..Default::default()
                },
                ..Default::default()
            },
            dangerous,
        }
    }

    #[test]
    fn register_and_lookup() {
        let registry = ToolRegistry::new();
        registry.register(Box::new(DummyTool), dummy_spec("dummy", false));

        assert!(registry.get("dummy").is_some());
        assert!(registry.get("unknown").is_none());
        assert_eq!(registry.names().len(), 1);
    }

    #[test]
    fn dangerous_flag() {
        let registry = ToolRegistry::new();
        registry.register(Box::new(DummyTool), dummy_spec("safe", false));
        registry.register(Box::new(DummyTool), dummy_spec("risky", true));

        assert!(!registry.is_dangerous("safe"));
        assert!(registry.is_dangerous("risky"));
    }

    #[test]
    fn tools_by_names_lookup() {
        let registry = ToolRegistry::new();
        registry.register(Box::new(DummyTool), dummy_spec("tool_a", false));
        registry.register(Box::new(DummyTool), dummy_spec("tool_b", false));

        let found = registry.tools_by_names(&[
            "tool_a".to_string(),
            "tool_b".to_string(),
            "missing".to_string(),
        ]);
        assert_eq!(found.len(), 2);
    }

    #[test]
    fn schema_for_derives_json_schema() {
        #[derive(serde::Deserialize, schemars::JsonSchema)]
        #[allow(dead_code)]
        struct TestArgs {
            /// A required name.
            name: String,
            /// An optional count.
            #[serde(default)]
            count: Option<u32>,
        }

        let schema = schema_for::<TestArgs>();
        // Serialize to Value for assertion convenience.
        let v = serde_json::to_value(&schema).unwrap();
        assert_eq!(v["type"], "object");
        assert!(v["properties"]["name"].is_object());
        assert!(v["properties"]["count"].is_object());
        // "name" should be required (non-Option), "count" should not
        let required = v["required"].as_array().unwrap();
        assert!(required.contains(&serde_json::json!("name")));
    }

    #[test]
    fn schema_for_has_expected_fields() {
        #[derive(serde::Deserialize, schemars::JsonSchema)]
        #[allow(dead_code)]
        struct TestToolArgs {
            /// A required path.
            path: String,
            #[serde(default)]
            count: Option<usize>,
        }

        let schema = schema_for::<TestToolArgs>();
        let value = serde_json::to_value(&schema).unwrap();

        // schemars adds $schema and title — these must be stripped by LLM drivers
        // for OpenAI-compatible APIs (llama.cpp, vLLM) that don't expect them.
        assert!(value.get("$schema").is_some(), "schemars produces $schema");
        assert!(value.get("title").is_some(), "schemars produces title");
        assert_eq!(value["type"], "object");
        assert!(value["required"].as_array().unwrap().contains(&serde_json::json!("path")));
    }

    #[test]
    fn normalized_lookup() {
        let registry = ToolRegistry::new();
        registry.register(Box::new(DummyTool), dummy_spec("shell_exec", false));

        // Exact match
        assert!(registry.get("shell_exec").is_some());
        // Normalized: uppercase + dashes
        assert!(registry.get("Shell-Exec").is_some());
        // Normalized: all caps
        assert!(registry.get("SHELL_EXEC").is_some());
        // Still fails for genuinely unknown tools
        assert!(registry.get("unknown").is_none());
    }

    #[test]
    fn alias_lookup() {
        let registry = ToolRegistry::new();
        registry.register(Box::new(DummyTool), dummy_spec("shell_exec", false));
        registry.add_alias("shell", "shell_exec");
        registry.add_alias("sh", "shell_exec");

        assert!(registry.get("shell").is_some());
        assert!(registry.get("sh").is_some());
        assert!(registry.get("Shell").is_some()); // normalized alias
        assert!(registry.spec("shell").is_some());
        assert!(registry.is_dangerous("shell") == registry.is_dangerous("shell_exec"));
    }
}
