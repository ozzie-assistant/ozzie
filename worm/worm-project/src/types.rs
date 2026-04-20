use serde::{Deserialize, Serialize};

/// Loaded project manifest (from `.ozzie/project.yaml` + `.ozzie/ozzie.md`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProjectManifest {
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// Skills to activate when this project is opened (global or local names).
    #[serde(default)]
    pub skills: Vec<String>,
    /// Tags for categorization and memory scoping.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Auto-commit file writes in this project workspace.
    #[serde(default)]
    pub git_auto_commit: bool,
    /// Memory pipeline configuration for workspace scanning.
    #[serde(default)]
    pub memory: Option<ProjectMemoryConfig>,
    /// Instructions loaded from `.ozzie/ozzie.md` — injected into LLM prompts.
    /// Not part of project.yaml, populated at load time.
    #[serde(skip)]
    pub instructions: String,
    /// Absolute path to the project root directory.
    /// Not part of project.yaml, populated at load time.
    #[serde(skip)]
    pub path: String,
}

/// Memory pipeline configuration — controls how the dream scanner
/// extracts knowledge from workspace files.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectMemoryConfig {
    /// Cron expression for workspace → memory consolidation.
    #[serde(default)]
    pub scan_cron: Option<String>,
    /// File glob patterns to include. Default: `["**/*.md", "**/*.txt", "**/*.json"]`.
    #[serde(default = "default_patterns")]
    pub patterns: Vec<String>,
    /// Max characters per file before truncation.
    #[serde(default = "default_max_file_chars")]
    pub max_file_chars: usize,
    /// Hints for the LLM classifier — what types of knowledge to extract.
    #[serde(default)]
    pub extract: Vec<ExtractionHint>,
}

/// Guides the LLM classifier on what knowledge to look for in workspace files.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractionHint {
    /// Memory type: fact, procedure, preference, context.
    #[serde(rename = "type")]
    pub memory_type: String,
    /// What to focus on for this type.
    pub focus: String,
}

fn default_patterns() -> Vec<String> {
    vec![
        "**/*.md".to_string(),
        "**/*.txt".to_string(),
        "**/*.json".to_string(),
    ]
}

fn default_max_file_chars() -> usize {
    10_000
}

impl Default for ProjectMemoryConfig {
    fn default() -> Self {
        Self {
            scan_cron: None,
            patterns: default_patterns(),
            max_file_chars: default_max_file_chars(),
            extract: Vec::new(),
        }
    }
}
