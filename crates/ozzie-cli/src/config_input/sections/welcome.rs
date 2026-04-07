use std::collections::HashSet;
use std::path::Path;

use ozzie_core::config;

use super::super::section::{
    BuildOutput, CollectResult, ConfigSection, FieldSpec, FieldValue, InputCollector, SectionId,
};

/// Output of the welcome section — wizard flow control only.
#[derive(Debug, Clone, Default)]
pub struct WelcomeConfig {
    pub existing: Option<config::Config>,
    /// Sections marked for reconfiguration.
    pub reconfigure: HashSet<SectionId>,
}

pub struct WelcomeSection {
    config_path: String,
    config_exists: bool,
}

impl WelcomeSection {
    pub fn new(config_path: &str, config_exists: bool) -> Self {
        Self {
            config_path: config_path.to_string(),
            config_exists,
        }
    }

    fn load_existing(&self) -> Option<config::Config> {
        if !self.config_exists {
            return None;
        }
        config::load(Path::new(&self.config_path)).ok()
    }

    fn existing_sections(cfg: &config::Config) -> Vec<SectionId> {
        let mut sections = Vec::new();
        if !cfg.models.providers.is_empty() {
            sections.push(SectionId::Providers);
        }
        if cfg.embedding.driver.is_some() {
            sections.push(SectionId::Embedding);
        }
        if !cfg.mcp.servers.is_empty() {
            sections.push(SectionId::Mcp);
        }
        if !cfg.connectors.0.is_empty() {
            sections.push(SectionId::Connectors);
        }
        if !cfg.skills.dirs.is_empty() {
            sections.push(SectionId::Skills);
        }
        if cfg.layered_context.is_enabled() {
            sections.push(SectionId::Memory);
        }
        sections.push(SectionId::Gateway);
        sections
    }
}

#[async_trait::async_trait]
impl ConfigSection for WelcomeSection {
    type Output = WelcomeConfig;

    fn id(&self) -> &str {
        SectionId::Welcome.as_str()
    }

    fn should_skip(&self, _current: Option<&Self::Output>) -> bool {
        false
    }

    fn fields(&self, _current: Option<&Self::Output>) -> Vec<FieldSpec> {
        vec![] // welcome is wizard-specific, driven by build()
    }

    fn validate(&self, _fragment: &Self::Output) -> Result<(), Vec<String>> {
        Ok(())
    }

    async fn build(
        &self,
        collector: &mut dyn InputCollector,
        _current: Option<&Self::Output>,
    ) -> anyhow::Result<Option<BuildOutput<Self::Output>>> {
        let existing = self.load_existing();

        // Per-section reconfiguration (only if existing config)
        let mut reconfigure = HashSet::new();
        if let Some(ref cfg) = existing {
            let section_list = Self::existing_sections(cfg);
            if !section_list.is_empty() {
                for section_id in &section_list {
                    if let Some(summary) = format_section_summary(cfg, *section_id) {
                        collector.show_info(&summary);
                    }

                    let key = format!("reconf_{section_id}");
                    let reconf_fields = vec![FieldSpec::confirm(&key, false)];
                    let reconf_values = match collector.collect(self.id(), &reconf_fields)? {
                        CollectResult::Values(v) => v,
                        _ => return Ok(None),
                    };
                    if reconf_values
                        .get(&key)
                        .and_then(FieldValue::as_bool)
                        .unwrap_or(false)
                    {
                        reconfigure.insert(*section_id);
                    }
                }
            }
        } else {
            for &section in SectionId::ALL_CONFIGURABLE {
                reconfigure.insert(section);
            }
        }

        Ok(Some(BuildOutput::new(WelcomeConfig {
            existing,
            reconfigure,
        })))
    }
}

fn format_section_summary(cfg: &config::Config, section_id: SectionId) -> Option<String> {
    use std::fmt::Write;
    let mut out = String::new();
    match section_id {
        SectionId::Providers => {
            let names: Vec<&str> = cfg.models.providers.keys().map(|s| s.as_str()).collect();
            if names.is_empty() {
                return None;
            }
            writeln!(out, "  \u{250c} Providers: {}", names.join(", ")).ok();
            if let Some(p) = cfg.models.providers.get(&cfg.models.default) {
                writeln!(
                    out,
                    "  \u{2502} Default: {} ({} / {})",
                    cfg.models.default, p.driver, p.model
                )
                .ok();
            }
            write!(out, "  \u{2514}").ok();
        }
        SectionId::Embedding => {
            let driver = cfg.embedding.driver?;
            let dims = cfg
                .embedding
                .dims
                .map(|d| format!(" ({d}d)"))
                .unwrap_or_default();
            writeln!(
                out,
                "  \u{250c} Embedding: {} / {}{dims}",
                driver, cfg.embedding.model
            )
            .ok();
            write!(out, "  \u{2514}").ok();
        }
        SectionId::Mcp => {
            if cfg.mcp.servers.is_empty() {
                return None;
            }
            let names: Vec<&str> = cfg.mcp.servers.keys().map(|s| s.as_str()).collect();
            writeln!(out, "  \u{250c} MCP: {}", names.join(", ")).ok();
            write!(out, "  \u{2514}").ok();
        }
        SectionId::Connectors => {
            if cfg.connectors.0.is_empty() {
                return None;
            }
            let names: Vec<&str> = cfg.connectors.0.keys().map(|s| s.as_str()).collect();
            writeln!(out, "  \u{250c} Connectors: {}", names.join(", ")).ok();
            write!(out, "  \u{2514}").ok();
        }
        SectionId::Skills => {
            if cfg.skills.dirs.is_empty() {
                return None;
            }
            writeln!(out, "  \u{250c} Skills dirs: {}", cfg.skills.dirs.join(", ")).ok();
            write!(out, "  \u{2514}").ok();
        }
        SectionId::Memory => {
            if !cfg.layered_context.is_enabled() {
                return None;
            }
            writeln!(
                out,
                "  \u{250c} Memory: recent={}, archives={}",
                cfg.layered_context.max_recent_messages, cfg.layered_context.max_archives
            )
            .ok();
            write!(out, "  \u{2514}").ok();
        }
        SectionId::Gateway => {
            writeln!(out, "  \u{250c} Gateway: {}:{}", cfg.gateway.host, cfg.gateway.port).ok();
            write!(out, "  \u{2514}").ok();
        }
        _ => return None,
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    use super::super::super::section::FieldValues;

    struct MockCollector(Vec<FieldValues>);
    impl InputCollector for MockCollector {
        fn collect(
            &mut self,
            _id: &str,
            _fields: &[FieldSpec],
        ) -> anyhow::Result<CollectResult> {
            if self.0.is_empty() {
                return Ok(CollectResult::Back);
            }
            Ok(CollectResult::Values(self.0.remove(0)))
        }
    }

    #[tokio::test]
    async fn build_fresh_install() {
        let section = WelcomeSection::new("/nonexistent/config.jsonc", false);
        let mut collector = MockCollector(vec![]);

        let output = section.build(&mut collector, None).await.unwrap().unwrap();
        assert!(output.config.existing.is_none());
        assert!(output.config.reconfigure.contains(&SectionId::Providers));
        assert!(output.config.reconfigure.contains(&SectionId::Gateway));
    }
}
