use std::path::Path;

use ozzie_core::domain::DreamExtraction;
use ozzie_core::project::{ProjectManifest, ProjectMemoryConfig};
use ozzie_llm::{ChatMessage, ChatRole, Provider};
use tracing::{debug, warn};

use super::workspace_record::WorkspaceRecord;

/// Maximum total chars sent to the LLM per project scan.
const MAX_BATCH_CHARS: usize = 30_000;

/// Scans a project workspace for changed files and classifies them via LLM.
///
/// Returns `None` if nothing changed since last scan.
pub async fn scan_workspace(
    provider: &dyn Provider,
    manifest: &ProjectManifest,
    previous: Option<&WorkspaceRecord>,
) -> anyhow::Result<Option<WorkspaceScanResult>> {
    let project_path = Path::new(&manifest.path);
    let memory_config = manifest.memory.as_ref();

    // Get current HEAD
    let current_head = git_head(project_path)?;
    if current_head.is_empty() {
        debug!(project = %manifest.name, "no git HEAD, skipping");
        return Ok(None);
    }

    // Skip if nothing changed
    if let Some(prev) = previous
        && prev.last_commit == current_head
    {
        debug!(project = %manifest.name, "no changes since last scan");
        return Ok(None);
    }

    // Get changed files
    let base_commit = previous.map(|r| r.last_commit.as_str());
    let changed_files = git_changed_files(project_path, base_commit)?;

    if changed_files.is_empty() {
        debug!(project = %manifest.name, "no changed files");
        return Ok(Some(WorkspaceScanResult {
            head_commit: current_head,
            extraction: DreamExtraction::default(),
        }));
    }

    // Filter by patterns
    let patterns = memory_config.map(|c| c.patterns.as_slice());
    let max_file_chars = memory_config
        .map(|c| c.max_file_chars)
        .unwrap_or(10_000);

    let relevant_files: Vec<&str> = changed_files
        .iter()
        .map(|s| s.as_str())
        .filter(|f| matches_patterns(f, patterns))
        .collect();

    if relevant_files.is_empty() {
        debug!(project = %manifest.name, "no relevant files after pattern filter");
        return Ok(Some(WorkspaceScanResult {
            head_commit: current_head,
            extraction: DreamExtraction::default(),
        }));
    }

    // Read file contents (with truncation and batch limit)
    let mut file_contents = Vec::new();
    let mut total_chars = 0;

    for file_path in &relevant_files {
        let full_path = project_path.join(file_path);
        let content = match std::fs::read_to_string(&full_path) {
            Ok(c) => c,
            Err(_) => continue, // deleted or binary
        };

        let truncated = if content.len() > max_file_chars {
            format!("{}...\n[truncated]", &content[..max_file_chars])
        } else {
            content
        };

        if total_chars + truncated.len() > MAX_BATCH_CHARS {
            debug!(
                project = %manifest.name,
                files_so_far = file_contents.len(),
                "batch limit reached, stopping file collection"
            );
            break;
        }

        total_chars += truncated.len();
        file_contents.push((*file_path, truncated));
    }

    if file_contents.is_empty() {
        return Ok(Some(WorkspaceScanResult {
            head_commit: current_head,
            extraction: DreamExtraction::default(),
        }));
    }

    debug!(
        project = %manifest.name,
        files = file_contents.len(),
        total_chars,
        "classifying workspace files"
    );

    // Classify via LLM
    let extraction = classify_workspace(
        provider,
        manifest,
        memory_config,
        &file_contents,
        previous,
    )
    .await?;

    Ok(Some(WorkspaceScanResult {
        head_commit: current_head,
        extraction,
    }))
}

pub struct WorkspaceScanResult {
    pub head_commit: String,
    pub extraction: DreamExtraction,
}

// ---- Classifier ----

const SYSTEM_PROMPT: &str = r#"You are a knowledge extractor for a project workspace. Analyze the following files and extract ONLY lasting, reusable knowledge.

Classify each piece into exactly one category:
- **profile**: Identity-level facts about the user (preferences, goals, constraints). Rarely changes.
- **memory**: Contextual knowledge (data points, decisions, procedures, status). Useful for future reference.

Rules:
- Do NOT repeat previously extracted knowledge (listed below if any).
- Only extract NEW information from the changed files.
- If nothing worth extracting, respond with empty arrays.
- Respond with JSON only, no markdown fences.

Response format:
{"profile": ["sentence 1", ...], "memory": [{"title": "...", "content": "...", "type": "preference|fact|procedure|context", "tags": ["..."]}]}"#;

async fn classify_workspace(
    provider: &dyn Provider,
    manifest: &ProjectManifest,
    memory_config: Option<&ProjectMemoryConfig>,
    files: &[(&str, String)],
    previous: Option<&WorkspaceRecord>,
) -> anyhow::Result<DreamExtraction> {
    let system = build_workspace_prompt(manifest, memory_config, previous);
    let user_content = format_files(files);

    let chat_messages = vec![
        ChatMessage::text(ChatRole::System, system),
        ChatMessage::text(ChatRole::User, user_content),
    ];

    let response = provider.chat(&chat_messages, &[]).await?;
    parse_extraction(&response.content)
}

fn build_workspace_prompt(
    manifest: &ProjectManifest,
    memory_config: Option<&ProjectMemoryConfig>,
    previous: Option<&WorkspaceRecord>,
) -> String {
    let mut prompt = SYSTEM_PROMPT.to_string();

    // Project context from ozzie.md
    prompt.push_str(&format!(
        "\n\nProject: {} ({})\n",
        manifest.name, manifest.description
    ));

    if !manifest.instructions.is_empty() {
        prompt.push_str("\n### Project Context\n");
        prompt.push_str(&manifest.instructions);
        prompt.push('\n');
    }

    // Extraction hints from memory config
    if let Some(config) = memory_config
        && !config.extract.is_empty()
    {
        prompt.push_str("\n### What to Extract\n");
        for hint in &config.extract {
            prompt.push_str(&format!("- **{}**: {}\n", hint.memory_type, hint.focus));
        }
    }

    // Tag hint
    if !manifest.tags.is_empty() {
        prompt.push_str(&format!(
            "\nAlways include these tags: {}\n",
            manifest.tags.join(", ")
        ));
    }

    // Previous extractions
    if let Some(record) = previous
        && !record.memory_ids.is_empty()
    {
        prompt.push_str("\nPreviously extracted memory IDs (do NOT re-extract): ");
        prompt.push_str(&record.memory_ids.join(", "));
        prompt.push('\n');
    }

    prompt
}

fn format_files(files: &[(&str, String)]) -> String {
    let mut out = String::with_capacity(files.iter().map(|(_, c)| c.len() + 50).sum());
    for (path, content) in files {
        out.push_str(&format!("=== FILE: {path} ===\n"));
        out.push_str(content);
        out.push_str("\n\n");
    }
    out
}

fn parse_extraction(raw: &str) -> anyhow::Result<DreamExtraction> {
    if let Ok(extraction) = serde_json::from_str::<DreamExtraction>(raw) {
        return Ok(extraction);
    }

    let stripped = extract_json(raw);
    if let Ok(extraction) = serde_json::from_str::<DreamExtraction>(stripped) {
        return Ok(extraction);
    }

    warn!(
        raw_len = raw.len(),
        "failed to parse workspace extraction, returning empty"
    );
    Ok(DreamExtraction::default())
}

fn extract_json(s: &str) -> &str {
    let s = s.trim();
    if let Some(start) = s.find("```json") {
        let after = &s[start + 7..];
        if let Some(end) = after.find("```") {
            return after[..end].trim();
        }
    }
    if let Some(start) = s.find("```") {
        let after = &s[start + 3..];
        if let Some(end) = after.find("```") {
            return after[..end].trim();
        }
    }
    if let (Some(start), Some(end)) = (s.find('{'), s.rfind('}')) {
        return &s[start..=end];
    }
    s
}

// ---- Git helpers ----

fn git_head(project_path: &Path) -> anyhow::Result<String> {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(project_path)
        .output()?;

    if !output.status.success() {
        return Ok(String::new());
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn git_changed_files(project_path: &Path, base_commit: Option<&str>) -> anyhow::Result<Vec<String>> {
    let args: Vec<&str> = match base_commit {
        Some(base) => vec!["diff", "--name-only", base, "HEAD"],
        None => vec!["ls-files"],
    };

    let output = std::process::Command::new("git")
        .args(&args)
        .current_dir(project_path)
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // If the base commit doesn't exist anymore, fall back to listing all files
        if base_commit.is_some() {
            debug!(error = %stderr, "git diff failed, falling back to ls-files");
            return git_changed_files(project_path, None);
        }
        anyhow::bail!("git command failed: {stderr}");
    }

    let files: Vec<String> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|l| !l.is_empty())
        .map(|l| l.to_string())
        .collect();

    Ok(files)
}

// ---- Pattern matching ----

/// Checks if a file path matches the configured patterns.
/// Patterns starting with `!` are exclusions.
/// No patterns = match all.
fn matches_patterns(file_path: &str, patterns: Option<&[String]>) -> bool {
    let patterns = match patterns {
        Some(p) if !p.is_empty() => p,
        _ => return true,
    };

    // Check exclusions first
    for pattern in patterns {
        if let Some(exclude) = pattern.strip_prefix('!')
            && simple_glob_match(exclude, file_path)
        {
            return false;
        }
    }

    // Check inclusions
    for pattern in patterns {
        if !pattern.starts_with('!') && simple_glob_match(pattern, file_path) {
            return true;
        }
    }

    false
}

/// Minimal glob matching — supports `*` (single segment) and `**` (any depth).
fn simple_glob_match(pattern: &str, path: &str) -> bool {
    // Extension-only patterns like "**/*.md"
    if let Some(ext) = pattern.strip_prefix("**/") {
        if ext.starts_with("*.") {
            let suffix = &ext[1..]; // ".md"
            return path.ends_with(suffix);
        }
        return path.contains(ext) || path.ends_with(ext);
    }

    // Exact match or simple wildcard
    if let Some(ext) = pattern.strip_prefix("*.") {
        return path.ends_with(&format!(".{ext}"));
    }

    // Prefix match for directory patterns
    if let Some(prefix) = pattern.strip_suffix("/**") {
        return path.starts_with(prefix);
    }

    // Exact match
    path == pattern
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pattern_match_extension() {
        assert!(matches_patterns("docs/readme.md", Some(&["**/*.md".to_string()])));
        assert!(!matches_patterns("docs/readme.txt", Some(&["**/*.md".to_string()])));
    }

    #[test]
    fn pattern_match_exclusion() {
        let patterns = vec!["**/*.md".to_string(), "!_index.md".to_string()];
        assert!(matches_patterns("notes.md", Some(&patterns)));
        assert!(!matches_patterns("_index.md", Some(&patterns)));
    }

    #[test]
    fn pattern_match_no_patterns() {
        assert!(matches_patterns("anything.rs", None));
        assert!(matches_patterns("anything.rs", Some(&[])));
    }

    #[test]
    fn pattern_match_directory_exclude() {
        let patterns = vec![
            "**/*.md".to_string(),
            "!.ozzie/**".to_string(),
        ];
        assert!(matches_patterns("notes.md", Some(&patterns)));
        assert!(!matches_patterns(".ozzie/project.yaml", Some(&patterns)));
    }

    #[test]
    fn format_files_output() {
        let files = vec![
            ("a.md", "hello".to_string()),
            ("b.md", "world".to_string()),
        ];
        let formatted = format_files(&files);
        assert!(formatted.contains("=== FILE: a.md ==="));
        assert!(formatted.contains("hello"));
        assert!(formatted.contains("=== FILE: b.md ==="));
    }

    #[test]
    fn parse_clean_json() {
        let json = r#"{"profile": [], "memory": [{"title": "Test", "content": "Data", "type": "fact", "tags": ["test"]}]}"#;
        let extraction = parse_extraction(json).unwrap();
        assert_eq!(extraction.memory.len(), 1);
    }

    #[test]
    fn parse_garbage_returns_empty() {
        let extraction = parse_extraction("not json").unwrap();
        assert!(extraction.memory.is_empty());
    }

    #[test]
    fn glob_match_double_star_ext() {
        assert!(simple_glob_match("**/*.md", "foo/bar/baz.md"));
        assert!(simple_glob_match("**/*.md", "readme.md"));
        assert!(!simple_glob_match("**/*.md", "readme.txt"));
    }

    #[test]
    fn glob_match_dir_prefix() {
        assert!(simple_glob_match(".ozzie/**", ".ozzie/project.yaml"));
        assert!(!simple_glob_match(".ozzie/**", "src/main.rs"));
    }
}
