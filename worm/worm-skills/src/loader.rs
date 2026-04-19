use std::collections::HashMap;
use std::path::Path;

use tracing::{debug, warn};

use crate::types::{SkillMD, TriggersDef};

/// Loads all skills from `$OZZIE_PATH/skills/*/SKILL.md`.
/// Returns an empty vec if the directory doesn't exist.
pub fn load_skills_dir(skills_dir: &Path) -> Vec<SkillMD> {
    if !skills_dir.exists() {
        debug!(path = %skills_dir.display(), "skills directory not found");
        return Vec::new();
    }

    let entries = match std::fs::read_dir(skills_dir) {
        Ok(e) => e,
        Err(e) => {
            warn!(error = %e, "failed to read skills directory");
            return Vec::new();
        }
    };

    let mut skills = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let skill_file = path.join("SKILL.md");
        if !skill_file.exists() {
            continue;
        }
        match parse_skill_md(&skill_file) {
            Ok(skill) => {
                debug!(name = %skill.name, "loaded skill");
                skills.push(skill);
            }
            Err(e) => {
                warn!(path = %skill_file.display(), error = %e, "failed to parse skill");
            }
        }
    }

    skills.sort_by(|a, b| a.name.cmp(&b.name));
    debug!(count = skills.len(), "skills loaded");
    skills
}

/// Builds a name→description map from loaded skills (for prompt injection).
pub fn skill_descriptions(skills: &[SkillMD]) -> HashMap<String, String> {
    skills
        .iter()
        .map(|s| (s.name.clone(), s.description.clone()))
        .collect()
}

/// Parses a SKILL.md file with YAML front-matter.
pub fn parse_skill_md(path: &Path) -> Result<SkillMD, SkillLoadError> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| SkillLoadError::Io(e.to_string()))?;
    let dir = path
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_string();

    // Try to extract YAML front-matter between --- delimiters
    if let Some(rest) = content.strip_prefix("---")
        && let Some(end) = rest.find("---")
    {
        let yaml_str = &rest[..end];
        let body = rest[end + 3..].trim().to_string();

        // Parse front-matter as SkillFrontMatter
        let fm: SkillFrontMatter = serde_json::from_str(
            &yaml_like_to_json(yaml_str),
        )
        .unwrap_or_default();

        return Ok(SkillMD {
            name: fm.name.unwrap_or_else(|| dir.clone()),
            description: fm.description.unwrap_or_default(),
            license: fm.license,
            compatibility: fm.compatibility,
            metadata: fm.metadata.unwrap_or_default(),
            allowed_tools: fm.allowed_tools.unwrap_or_default(),
            body,
            dir,
            workflow: None,
            triggers: fm.triggers,
            source: Default::default(),
        });
    }

    // No front-matter — use directory name as skill name, first heading as description
    let name = dir.clone();
    let first_line = content.lines().next().unwrap_or("");
    let description = first_line
        .trim_start_matches('#')
        .trim()
        .to_string();

    Ok(SkillMD {
        name,
        description,
        license: None,
        compatibility: None,
        metadata: HashMap::new(),
        allowed_tools: Vec::new(),
        body: content,
        dir,
        workflow: None,
        triggers: None,
        source: Default::default(),
    })
}

#[derive(Debug, thiserror::Error)]
pub enum SkillLoadError {
    #[error("IO error: {0}")]
    Io(String),
    #[error("parse error: {0}")]
    Parse(String),
}

#[derive(Debug, Default, serde::Deserialize)]
struct SkillFrontMatter {
    name: Option<String>,
    description: Option<String>,
    license: Option<String>,
    compatibility: Option<String>,
    metadata: Option<HashMap<String, String>>,
    allowed_tools: Option<Vec<String>>,
    triggers: Option<TriggersDef>,
}

/// Minimal YAML-like parser that converts simple key: value YAML to JSON.
/// Handles nested objects for triggers, but not full YAML spec.
fn yaml_like_to_json(yaml: &str) -> String {
    let mut json_parts = Vec::new();
    let mut in_nested = false;
    let mut nested_key = String::new();
    let mut nested_parts = Vec::new();

    for line in yaml.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        let indent = line.len() - line.trim_start().len();

        if indent >= 2 && in_nested {
            if let Some((k, v)) = trimmed.split_once(':') {
                let k = k.trim().trim_matches('"');
                let v = v.trim().trim_matches('"');
                if v.is_empty() {
                    continue;
                }
                nested_parts.push(format!("\"{k}\": {}", json_value(v)));
            }
            continue;
        }

        if in_nested {
            json_parts.push(format!(
                "\"{nested_key}\": {{{}}}",
                nested_parts.join(", ")
            ));
            nested_parts.clear();
            in_nested = false;
        }

        if let Some((k, v)) = trimmed.split_once(':') {
            let k = k.trim().trim_matches('"');
            let v = v.trim();
            if v.is_empty() {
                in_nested = true;
                nested_key = k.to_string();
                continue;
            }
            json_parts.push(format!("\"{k}\": {}", json_value(v)));
        }
    }

    if in_nested {
        json_parts.push(format!(
            "\"{nested_key}\": {{{}}}",
            nested_parts.join(", ")
        ));
    }

    format!("{{{}}}", json_parts.join(", "))
}

fn json_value(v: &str) -> String {
    let v = v.trim_matches('"').trim_matches('\'');
    if v.parse::<f64>().is_ok() {
        return v.to_string();
    }
    if v == "true" || v == "false" {
        return v.to_string();
    }
    if v.starts_with('[') && v.ends_with(']') {
        return v.to_string();
    }
    format!("\"{v}\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_empty_dir() {
        let dir = tempfile::tempdir().unwrap();
        let skills = load_skills_dir(dir.path());
        assert!(skills.is_empty());
    }

    #[test]
    fn load_nonexistent_dir() {
        let skills = load_skills_dir(Path::new("/nonexistent"));
        assert!(skills.is_empty());
    }

    #[test]
    fn load_skill_no_frontmatter() {
        let dir = tempfile::tempdir().unwrap();
        let skill_dir = dir.path().join("my-skill");
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(skill_dir.join("SKILL.md"), "# My Skill\nDoes things.").unwrap();

        let skills = load_skills_dir(dir.path());
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].name, "my-skill");
        assert_eq!(skills[0].description, "My Skill");
    }

    #[test]
    fn load_skill_with_frontmatter() {
        let dir = tempfile::tempdir().unwrap();
        let skill_dir = dir.path().join("deploy");
        std::fs::create_dir_all(&skill_dir).unwrap();
        let content = "---\nname: deploy\ndescription: Deploy to production\n---\n# Deploy\n\nSteps here.";
        std::fs::write(skill_dir.join("SKILL.md"), content).unwrap();

        let skills = load_skills_dir(dir.path());
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].name, "deploy");
        assert_eq!(skills[0].description, "Deploy to production");
        assert_eq!(skills[0].body, "# Deploy\n\nSteps here.");
    }

    #[test]
    fn skill_descriptions_map() {
        let skills = vec![
            SkillMD {
                name: "a".to_string(),
                description: "desc_a".to_string(),
                license: None,
                compatibility: None,
                metadata: HashMap::new(),
                allowed_tools: Vec::new(),
                body: String::new(),
                dir: String::new(),
                workflow: None,
                triggers: None,
                source: Default::default(),
            },
        ];
        let descs = skill_descriptions(&skills);
        assert_eq!(descs.get("a").unwrap(), "desc_a");
    }

    #[test]
    fn yaml_like_parse_simple() {
        let yaml = "\nname: test\ndescription: A test skill\n";
        let json = yaml_like_to_json(yaml);
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["name"], "test");
        assert_eq!(v["description"], "A test skill");
    }
}
