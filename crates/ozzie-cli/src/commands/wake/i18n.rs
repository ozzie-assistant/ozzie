use std::collections::HashMap;

use ozzie_utils::i18n;

/// Registers all wizard translation catalogs.
pub fn register_catalogs() {
    i18n::register("en", en());
    i18n::register("fr", fr());
}

fn en() -> HashMap<String, String> {
    HashMap::from([
        // ── Welcome ────────────────────────────────────────────────────
        ("wizard.title", "Ozzie Setup Wizard"),
        ("wizard.welcome", "Let's get Ozzie ready."),
        ("wizard.lang_prompt", "Choose your language:"),
        ("wizard.lang_en", "English"),
        ("wizard.lang_fr", "Français"),
        ("wizard.config_exists", "Existing configuration found at {path}."),
        ("wizard.config_keep", "Keep existing configuration?"),
        ("wizard.config_keep_yes", "Yes, keep it"),
        ("wizard.config_keep_no", "No, start fresh"),
        // ── Provider ───────────────────────────────────────────────────
        ("wizard.provider.title", "LLM Provider"),
        ("wizard.provider.prompt", "Choose your LLM provider:"),
        ("wizard.provider.anthropic", "Anthropic (Claude)"),
        ("wizard.provider.openai", "OpenAI"),
        ("wizard.provider.gemini", "Gemini (Google)"),
        ("wizard.provider.mistral", "Mistral"),
        ("wizard.provider.ollama", "Ollama (local)"),
        ("wizard.provider.openai_compat", "OpenAI-compatible (custom)"),
        ("wizard.provider.model_prompt", "Choose a model:"),
        ("wizard.provider.model_custom", "Custom model ID"),
        ("wizard.provider.model_custom_prompt", "Enter model ID:"),
        ("wizard.provider.base_url_prompt", "Base URL (leave empty for default):"),
        ("wizard.provider.api_key_prompt", "Enter {env_var} (or leave empty to set later):"),
        ("wizard.provider.api_key_saved", "API key saved."),
        ("wizard.provider.api_key_skip", "No key provided — set {env_var} later."),
        ("wizard.provider.caps_prompt", "Select model capabilities:"),
        ("wizard.provider.add_more", "Add another provider?"),
        ("wizard.provider.alias_prompt", "Provider alias (e.g. 'fast', 'smart'):"),
        // ── Default provider ───────────────────────────────────────────
        ("wizard.default.title", "Default Provider"),
        ("wizard.default.prompt", "Choose the default provider:"),
        // ── Embedding ──────────────────────────────────────────────────
        ("wizard.embedding.title", "Embedding Model"),
        ("wizard.embedding.enable", "Enable semantic memory (requires an embedding model)?"),
        ("wizard.embedding.driver_prompt", "Embedding provider:"),
        ("wizard.embedding.model_prompt", "Embedding model:"),
        ("wizard.embedding.dim_prompt", "Dimensions (leave empty for default):"),
        // ── MCP ────────────────────────────────────────────────────────
        ("wizard.mcp.title", "MCP Servers"),
        ("wizard.mcp.prompt", "Configure an MCP server?"),
        ("wizard.mcp.transport_prompt", "Transport type:"),
        ("wizard.mcp.command_prompt", "Command (e.g. npx my-mcp-server):"),
        ("wizard.mcp.url_prompt", "Server URL:"),
        ("wizard.mcp.name_prompt", "Server name:"),
        ("wizard.mcp.probing", "Probing server for available tools..."),
        ("wizard.mcp.tools_found", "Found {count} tools."),
        ("wizard.mcp.trusted_prompt", "Select trusted tools (no approval needed):"),
        ("wizard.mcp.add_more", "Add another MCP server?"),
        // ── Connectors ─────────────────────────────────────────────────
        ("wizard.connectors.title", "Connectors"),
        ("wizard.connectors.prompt", "Configure a connector?"),
        ("wizard.connectors.tui_default", "TUI (terminal) — recommended"),
        ("wizard.connectors.discord", "Discord"),
        ("wizard.connectors.file", "File bridge (dev/testing)"),
        // ── Skills ──────────────────────────────────────────────────────
        ("wizard.skills.title", "Skills"),
        ("wizard.skills.enable", "Enable skill engine?"),
        ("wizard.skills.dirs_hint", "The default skills directory is $OZZIE_PATH/skills/. You can add extra directories."),
        ("wizard.skills.dir_prompt", "Additional skills directory (empty to skip):"),
        // ── Memory ─────────────────────────────────────────────────────
        ("wizard.memory.title", "Memory & Context"),
        ("wizard.memory.enable", "Enable layered context (conversation compression)?"),
        ("wizard.memory.customize", "Customize memory parameters? (or use defaults)"),
        ("wizard.memory.max_recent", "Max recent messages to keep:"),
        ("wizard.memory.max_archives", "Max archive chunks:"),
        // ── Gateway ────────────────────────────────────────────────────
        ("wizard.gateway.title", "Gateway"),
        ("wizard.gateway.host_prompt", "Gateway host:"),
        ("wizard.gateway.port_prompt", "Gateway port:"),
        // ── Confirm ────────────────────────────────────────────────────
        ("wizard.confirm.title", "Summary"),
        ("wizard.confirm.prompt", "Apply this configuration?"),
        ("wizard.confirm.yes", "Yes, apply"),
        ("wizard.confirm.no", "No, go back"),
        ("wizard.confirm.applying", "Applying configuration..."),
        ("wizard.confirm.done", "Configuration saved."),
        // ── Acquaintance ───────────────────────────────────────────────
        ("wizard.acquaintance.title", "Getting to know each other"),
        ("wizard.acquaintance.name", "What's your name?"),
        ("wizard.acquaintance.context", "In a few words, what do you do?"),
        ("wizard.acquaintance.tone", "How should Ozzie talk to you? (casual, professional, concise...)"),
        ("wizard.acquaintance.thinking", "Ozzie is thinking..."),
        ("wizard.acquaintance.adjusting", "Ozzie is adjusting..."),
        ("wizard.acquaintance.confirm", "Looks good? (yes / or type adjustments)"),
        ("wizard.acquaintance.saved", "Profile saved."),
        ("wizard.acquaintance.skip", "Skipping acquaintance step."),
        // ── Common ─────────────────────────────────────────────────────
        ("wizard.yes", "Yes"),
        ("wizard.no", "No"),
        ("wizard.back", "Back"),
        ("wizard.next", "Next"),
        ("wizard.skip", "Skip"),
        ("wizard.cancel", "Cancel"),
        ("wizard.step_of", "Step {current} of {total}"),
        ("wizard.nav_hint", "↑/↓ navigate • Enter select • Esc back"),
        ("wizard.done.title", "Setup complete!"),
        ("wizard.done.gateway_hint", "Start the gateway with:"),
        ("wizard.done.ask_hint", "Then ask a question:"),
    ]
    .map(|(k, v)| (k.to_string(), v.to_string())))
}

fn fr() -> HashMap<String, String> {
    HashMap::from([
        // ── Welcome ────────────────────────────────────────────────────
        ("wizard.title", "Assistant de configuration Ozzie"),
        ("wizard.welcome", "Préparons Ozzie."),
        ("wizard.lang_prompt", "Choisissez votre langue :"),
        ("wizard.lang_en", "English"),
        ("wizard.lang_fr", "Français"),
        ("wizard.config_exists", "Configuration existante trouvée dans {path}."),
        ("wizard.config_keep", "Conserver la configuration existante ?"),
        ("wizard.config_keep_yes", "Oui, la garder"),
        ("wizard.config_keep_no", "Non, repartir de zéro"),
        // ── Provider ───────────────────────────────────────────────────
        ("wizard.provider.title", "Fournisseur LLM"),
        ("wizard.provider.prompt", "Choisissez votre fournisseur LLM :"),
        ("wizard.provider.anthropic", "Anthropic (Claude)"),
        ("wizard.provider.openai", "OpenAI"),
        ("wizard.provider.gemini", "Gemini (Google)"),
        ("wizard.provider.mistral", "Mistral"),
        ("wizard.provider.ollama", "Ollama (local)"),
        ("wizard.provider.openai_compat", "OpenAI-compatible (custom)"),
        ("wizard.provider.model_prompt", "Choisissez un modèle :"),
        ("wizard.provider.model_custom", "ID de modèle personnalisé"),
        ("wizard.provider.model_custom_prompt", "Entrez l'ID du modèle :"),
        ("wizard.provider.base_url_prompt", "URL de base (laisser vide pour le défaut) :"),
        ("wizard.provider.api_key_prompt", "Entrez {env_var} (ou laisser vide pour plus tard) :"),
        ("wizard.provider.api_key_saved", "Clé API sauvegardée."),
        ("wizard.provider.api_key_skip", "Pas de clé — définir {env_var} plus tard."),
        ("wizard.provider.caps_prompt", "Sélectionnez les capacités du modèle :"),
        ("wizard.provider.add_more", "Ajouter un autre fournisseur ?"),
        ("wizard.provider.alias_prompt", "Alias du fournisseur (ex: 'fast', 'smart') :"),
        // ── Default provider ───────────────────────────────────────────
        ("wizard.default.title", "Fournisseur par défaut"),
        ("wizard.default.prompt", "Choisissez le fournisseur par défaut :"),
        // ── Embedding ──────────────────────────────────────────────────
        ("wizard.embedding.title", "Modèle d'embedding"),
        ("wizard.embedding.enable", "Activer la mémoire sémantique (nécessite un modèle d'embedding) ?"),
        ("wizard.embedding.driver_prompt", "Fournisseur d'embedding :"),
        ("wizard.embedding.model_prompt", "Modèle d'embedding :"),
        ("wizard.embedding.dim_prompt", "Dimensions (laisser vide pour le défaut) :"),
        // ── MCP ────────────────────────────────────────────────────────
        ("wizard.mcp.title", "Serveurs MCP"),
        ("wizard.mcp.prompt", "Configurer un serveur MCP ?"),
        ("wizard.mcp.transport_prompt", "Type de transport :"),
        ("wizard.mcp.command_prompt", "Commande (ex: npx my-mcp-server) :"),
        ("wizard.mcp.url_prompt", "URL du serveur :"),
        ("wizard.mcp.name_prompt", "Nom du serveur :"),
        ("wizard.mcp.probing", "Interrogation du serveur pour les outils disponibles..."),
        ("wizard.mcp.tools_found", "{count} outils trouvés."),
        ("wizard.mcp.trusted_prompt", "Sélectionnez les outils de confiance (sans approbation) :"),
        ("wizard.mcp.add_more", "Ajouter un autre serveur MCP ?"),
        // ── Connectors ─────────────────────────────────────────────────
        ("wizard.connectors.title", "Connecteurs"),
        ("wizard.connectors.prompt", "Configurer un connecteur ?"),
        ("wizard.connectors.tui_default", "TUI (terminal) — recommandé"),
        ("wizard.connectors.discord", "Discord"),
        ("wizard.connectors.file", "File bridge (dev/test)"),
        // ── Skills ──────────────────────────────────────────────────────
        ("wizard.skills.title", "Compétences"),
        ("wizard.skills.enable", "Activer le moteur de compétences ?"),
        ("wizard.skills.dirs_hint", "Le répertoire par défaut est $OZZIE_PATH/skills/. Vous pouvez ajouter d'autres répertoires."),
        ("wizard.skills.dir_prompt", "Répertoire de compétences supplémentaire (vide pour passer) :"),
        // ── Memory ─────────────────────────────────────────────────────
        ("wizard.memory.title", "Mémoire & Contexte"),
        ("wizard.memory.enable", "Activer le contexte en couches (compression de conversation) ?"),
        ("wizard.memory.customize", "Personnaliser les paramètres mémoire ? (ou garder les défauts)"),
        ("wizard.memory.max_recent", "Nombre max de messages récents à garder :"),
        ("wizard.memory.max_archives", "Nombre max de chunks d'archive :"),
        // ── Gateway ────────────────────────────────────────────────────
        ("wizard.gateway.title", "Passerelle"),
        ("wizard.gateway.host_prompt", "Hôte de la passerelle :"),
        ("wizard.gateway.port_prompt", "Port de la passerelle :"),
        // ── Confirm ────────────────────────────────────────────────────
        ("wizard.confirm.title", "Résumé"),
        ("wizard.confirm.prompt", "Appliquer cette configuration ?"),
        ("wizard.confirm.yes", "Oui, appliquer"),
        ("wizard.confirm.no", "Non, revenir en arrière"),
        ("wizard.confirm.applying", "Application de la configuration..."),
        ("wizard.confirm.done", "Configuration sauvegardée."),
        // ── Acquaintance ───────────────────────────────────────────────
        ("wizard.acquaintance.title", "Faire connaissance"),
        ("wizard.acquaintance.name", "Comment vous appelez-vous ?"),
        ("wizard.acquaintance.context", "En quelques mots, que faites-vous ?"),
        ("wizard.acquaintance.tone", "Comment Ozzie doit-il vous parler ? (familier, professionnel, concis...)"),
        ("wizard.acquaintance.thinking", "Ozzie réfléchit..."),
        ("wizard.acquaintance.adjusting", "Ozzie ajuste..."),
        ("wizard.acquaintance.confirm", "Ça vous convient ? (oui / ou tapez vos ajustements)"),
        ("wizard.acquaintance.saved", "Profil sauvegardé."),
        ("wizard.acquaintance.skip", "Étape de présentation passée."),
        // ── Common ─────────────────────────────────────────────────────
        ("wizard.yes", "Oui"),
        ("wizard.no", "Non"),
        ("wizard.back", "Retour"),
        ("wizard.next", "Suivant"),
        ("wizard.skip", "Passer"),
        ("wizard.cancel", "Annuler"),
        ("wizard.step_of", "Étape {current} sur {total}"),
        ("wizard.nav_hint", "↑/↓ naviguer • Entrée sélectionner • Échap retour"),
        ("wizard.done.title", "Configuration terminée !"),
        ("wizard.done.gateway_hint", "Lancez la passerelle avec :"),
        ("wizard.done.ask_hint", "Puis posez une question :"),
    ]
    .map(|(k, v)| (k.to_string(), v.to_string())))
}
