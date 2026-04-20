use std::path::Path;

use clap::{Args, Subcommand};
use ozzie_utils::config::{logs_path, skills_path};
use ozzie_core::skills::{FsSkillRepository, SkillMD, SkillRepository, TriggersDef};

use crate::output;

/// Schedule management commands.
#[derive(Args)]
pub struct ScheduleArgs {
    #[command(subcommand)]
    command: ScheduleCommand,

    /// Output as JSON.
    #[arg(long, global = true)]
    json: bool,
}

#[derive(Subcommand)]
enum ScheduleCommand {
    /// List all scheduled triggers.
    List,
    /// Show recent trigger history.
    History {
        /// Maximum number of entries.
        #[arg(long, default_value = "20")]
        limit: usize,
    },
}

pub async fn run(args: ScheduleArgs) -> anyhow::Result<()> {
    match args.command {
        ScheduleCommand::List => list_triggers(args.json).await,
        ScheduleCommand::History { limit } => show_history(limit, args.json).await,
    }
}

async fn list_triggers(json: bool) -> anyhow::Result<()> {
    let skills_dir = skills_path();
    let skills = load_skills_with_triggers(&skills_dir).await?;

    if json {
        return output::print_json(&skills);
    }

    if skills.is_empty() {
        println!("No scheduled skills found.");
        return Ok(());
    }

    let rows: Vec<Vec<String>> = skills
        .iter()
        .map(|(skill, triggers)| {
            let source = if skill.dir.is_empty() {
                "-".to_string()
            } else {
                skill.dir.clone()
            };
            let cron = triggers
                .cron
                .as_deref()
                .unwrap_or("-")
                .to_string();
            let event = triggers
                .on_event
                .as_ref()
                .map(|e| e.event.clone())
                .unwrap_or_else(|| "-".to_string());
            let enabled = if triggers.max_runs == Some(0) {
                "no"
            } else {
                "yes"
            };
            vec![
                source,
                skill.name.clone(),
                cron,
                event,
                enabled.to_string(),
            ]
        })
        .collect();

    output::print_table(&["SOURCE", "NAME", "CRON", "EVENT", "ENABLED"], rows);
    Ok(())
}

async fn show_history(limit: usize, json: bool) -> anyhow::Result<()> {
    let logs_dir = logs_path();
    let entries = read_scheduler_logs(&logs_dir, limit)?;

    if json {
        return output::print_json(&entries);
    }

    if entries.is_empty() {
        println!("No scheduler history found.");
        return Ok(());
    }

    let rows: Vec<Vec<String>> = entries
        .iter()
        .map(|e| {
            vec![
                e.get("ts")
                    .and_then(|v| v.as_str())
                    .unwrap_or("-")
                    .to_string(),
                e.get("skill")
                    .and_then(|v| v.as_str())
                    .unwrap_or("-")
                    .to_string(),
                e.get("trigger")
                    .and_then(|v| v.as_str())
                    .unwrap_or("-")
                    .to_string(),
                e.get("status")
                    .and_then(|v| v.as_str())
                    .unwrap_or("-")
                    .to_string(),
            ]
        })
        .collect();

    output::print_table(&["TIMESTAMP", "SKILL", "TRIGGER", "STATUS"], rows);
    Ok(())
}

/// Loads all SKILL.md files that have triggers defined.
async fn load_skills_with_triggers(skills_dir: &Path) -> anyhow::Result<Vec<(SkillMD, TriggersDef)>> {
    let all_skills = FsSkillRepository::new(skills_dir).load_all().await;
    let results: Vec<(SkillMD, TriggersDef)> = all_skills
        .into_iter()
        .filter_map(|skill| {
            let triggers = skill.triggers.clone()?;
            Some((skill, triggers))
        })
        .collect();
    Ok(results)
}

/// Reads scheduler log entries from the logs directory.
fn read_scheduler_logs(
    logs_dir: &Path,
    limit: usize,
) -> anyhow::Result<Vec<serde_json::Value>> {
    let log_path = logs_dir.join("scheduler.jsonl");
    if !log_path.exists() {
        return Ok(Vec::new());
    }

    let content = std::fs::read_to_string(&log_path)?;
    let mut entries: Vec<serde_json::Value> = content
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();

    // Most recent first
    entries.reverse();
    entries.truncate(limit);
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn parse_empty_skills_dir() {
        let dir = tempfile::tempdir().unwrap();
        let result = load_skills_with_triggers(dir.path()).await.unwrap();
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn parse_skill_md_no_frontmatter() {
        let dir = tempfile::tempdir().unwrap();
        let skill_dir = dir.path().join("my-skill");
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(skill_dir.join("SKILL.md"), "# My Skill\nDoes things.").unwrap();

        let repo = FsSkillRepository::new(&skill_dir);
        let skill = repo.load_one(&skill_dir.join("SKILL.md")).await.unwrap();
        assert_eq!(skill.name, "my-skill");
        assert_eq!(skill.description, "My Skill");
        assert!(skill.triggers.is_none());
    }

    #[tokio::test]
    async fn parse_skill_md_with_frontmatter() {
        let dir = tempfile::tempdir().unwrap();
        let skill_dir = dir.path().join("deploy");
        std::fs::create_dir_all(&skill_dir).unwrap();
        let content = r#"---
name: deploy
description: Deploy to production
---
# Deploy

Steps here."#;
        std::fs::write(skill_dir.join("SKILL.md"), content).unwrap();

        let repo = FsSkillRepository::new(&skill_dir);
        let skill = repo.load_one(&skill_dir.join("SKILL.md")).await.unwrap();
        assert_eq!(skill.name, "deploy");
        assert_eq!(skill.description, "Deploy to production");
        assert_eq!(skill.body, "# Deploy\n\nSteps here.");
    }

    #[test]
    fn read_empty_scheduler_logs() {
        let dir = tempfile::tempdir().unwrap();
        let entries = read_scheduler_logs(dir.path(), 20).unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn read_scheduler_logs_with_entries() {
        let dir = tempfile::tempdir().unwrap();
        let log_path = dir.path().join("scheduler.jsonl");
        let lines = [
            r#"{"ts":"2024-01-01T00:00:00Z","skill":"backup","trigger":"cron","status":"ok"}"#,
            r#"{"ts":"2024-01-01T01:00:00Z","skill":"deploy","trigger":"event","status":"failed"}"#,
        ];
        std::fs::write(&log_path, lines.join("\n")).unwrap();

        let entries = read_scheduler_logs(dir.path(), 20).unwrap();
        assert_eq!(entries.len(), 2);
        // Most recent first
        assert_eq!(entries[0]["skill"], "deploy");
    }

}
