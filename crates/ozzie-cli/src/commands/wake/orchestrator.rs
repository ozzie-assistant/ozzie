use std::path::Path;

use ozzie_core::config::{AgentConfig, Config};
use ozzie_utils::i18n;

use crate::config_input::section::{BuildOutput, ConfigSection, InputCollector, SectionId};
use crate::config_input::sections::{
    ConnectorsSection, EmbeddingSection, GatewaySection, LanguageSection, McpServersSection,
    MemorySection, ProvidersSection, SkillsSection, WelcomeSection,
};

/// Runs the full wizard using the given input collector.
///
/// Returns `Ok(Some(Config))` on success, `Ok(None)` if cancelled.
pub async fn run<C: InputCollector>(
    collector: &mut C,
    base: &Path,
) -> anyhow::Result<Option<Config>> {
    let config_path = base.join("config.jsonc");
    let config_exists = config_path.exists();
    let config_display = config_path.display().to_string();

    // ── Language ───────────────────────────────────────────────────────
    let existing_lang = if config_exists {
        ozzie_core::config::load(&config_path)
            .ok()
            .and_then(|c| c.agent.preferred_language)
            .unwrap_or_else(i18n::detect)
    } else {
        i18n::detect()
    };
    let language_section = LanguageSection::new(&existing_lang);
    let language = match run_section(&language_section, collector, None).await? {
        Some(output) => output.config.language,
        None => return Ok(None),
    };
    i18n::set_lang(&language);

    // ── Welcome (config detection + reconf flags) ─────────────────────
    let welcome_section = WelcomeSection::new(&config_display, config_exists);
    let welcome = match run_section(&welcome_section, collector, None).await? {
        Some(output) => output.config,
        None => return Ok(None),
    };

    let reconf = |section: SectionId| welcome.reconfigure.contains(&section);
    let existing = welcome.existing.as_ref();

    // ── Providers ──────────────────────────────────────────────────────
    let (models, provider_secrets) = if reconf(SectionId::Providers) {
        let provider_section = ProvidersSection::new();
        let current = existing.map(|e| &e.models);
        match run_section(&provider_section, collector, current).await? {
            Some(output) => (Some(output.config), output.secrets),
            None => return Ok(None),
        }
    } else {
        (None, Vec::new())
    };

    // ── Embedding ──────────────────────────────────────────────────────
    let (embedding, embedding_secrets) = if reconf(SectionId::Embedding) {
        let embedding_section = EmbeddingSection::new();
        let current = existing.map(|e| &e.embedding);
        match run_section(&embedding_section, collector, current).await? {
            Some(output) => (Some(output.config), output.secrets),
            None => return Ok(None),
        }
    } else {
        (None, Vec::new())
    };

    // ── MCP ────────────────────────────────────────────────────────────
    let mcp = if reconf(SectionId::Mcp) {
        let mcp_section = McpServersSection;
        let current = existing.map(|e| &e.mcp);
        match run_section(&mcp_section, collector, current).await? {
            Some(output) => Some(output.config),
            None => return Ok(None),
        }
    } else {
        None
    };

    // ── Connectors ─────────────────────────────────────────────────────
    let connectors = if reconf(SectionId::Connectors) {
        let connector_section = ConnectorsSection;
        let current = existing.map(|e| &e.connectors);
        match run_section(&connector_section, collector, current).await? {
            Some(output) => Some(output.config),
            None => return Ok(None),
        }
    } else {
        None
    };

    // ── Skills ─────────────────────────────────────────────────────────
    let skills = if reconf(SectionId::Skills) {
        let skills_section = SkillsSection;
        let current = existing.map(|e| &e.skills);
        match run_section(&skills_section, collector, current).await? {
            Some(output) => Some(output.config),
            None => return Ok(None),
        }
    } else {
        None
    };

    // ── Memory ─────────────────────────────────────────────────────────
    let layered_context = if reconf(SectionId::Memory) {
        let memory_section = MemorySection;
        let current = existing.map(|e| &e.layered_context);
        match run_section(&memory_section, collector, current).await? {
            Some(output) => Some(output.config),
            None => return Ok(None),
        }
    } else {
        None
    };

    // ── Gateway ────────────────────────────────────────────────────────
    let gateway = if reconf(SectionId::Gateway) {
        let gateway_section = GatewaySection;
        let current = existing.map(|e| &e.gateway);
        match run_section(&gateway_section, collector, current).await? {
            Some(output) => Some(output.config),
            None => return Ok(None),
        }
    } else {
        None
    };

    // ── Assemble ───────────────────────────────────────────────────────
    let ex = existing.cloned().unwrap_or_default();

    let cfg = Config {
        models: models.unwrap_or(ex.models),
        agent: AgentConfig {
            preferred_language: Some(language.clone()),
            ..ex.agent
        },
        gateway: gateway.unwrap_or(ex.gateway),
        embedding: embedding.unwrap_or(ex.embedding),
        skills: skills.unwrap_or(ex.skills),
        layered_context: layered_context.unwrap_or(ex.layered_context),
        mcp: mcp.unwrap_or(ex.mcp),
        connectors: connectors.unwrap_or(ex.connectors),
        events: ex.events,
        plugins: ex.plugins,
        tools: ex.tools,
        sandbox: ex.sandbox,
        runtime: ex.runtime,
        web: ex.web,
        policies: ex.policies,
        sub_agents: ex.sub_agents,
    };

    // ── Validate ───────────────────────────────────────────────────────
    validate(&cfg)?;

    // ── Encrypt secrets ────────────────────────────────────────────────
    let mut all_secrets = provider_secrets;
    all_secrets.extend(embedding_secrets);
    crate::config_input::store_secrets(base, &all_secrets)?;
    // TODO: MCP secrets when env var support is added

    Ok(Some(cfg))
}

fn validate(cfg: &Config) -> anyhow::Result<()> {
    if cfg.models.providers.is_empty() {
        anyhow::bail!("at least one LLM provider must be configured");
    }
    if !cfg.models.default.is_empty()
        && !cfg.models.providers.contains_key(&cfg.models.default)
    {
        anyhow::bail!(
            "default provider '{}' not found in: [{}]",
            cfg.models.default,
            cfg.models.providers.keys().cloned().collect::<Vec<_>>().join(", ")
        );
    }
    if cfg.gateway.port == 0 {
        anyhow::bail!("gateway port must be > 0");
    }
    Ok(())
}

// ── Section runner ─────────────────────────────────────────────────────────

/// Runs a single ConfigSection: build (with collector) → validate.
///
/// Returns `None` if the user cancelled or went back.
async fn run_section<S: ConfigSection>(
    section: &S,
    collector: &mut impl InputCollector,
    current: Option<&S::Output>,
) -> anyhow::Result<Option<BuildOutput<S::Output>>> {
    if section.should_skip(current) {
        return Ok(current.cloned().map(BuildOutput::new));
    }

    collector.show_title(section.id());

    // Build (section drives collection internally)
    let output = match section.build(collector, current).await? {
        Some(o) => o,
        None => return Ok(None),
    };

    // Validate
    if let Err(errors) = section.validate(&output.config) {
        collector.show_errors(&errors);
        // For now, continue anyway — TODO: retry loop
    }

    Ok(Some(output))
}
