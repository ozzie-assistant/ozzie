#![allow(dead_code)]

use std::collections::HashMap;

// ── Section ID ───────────────────────────────────────────────────────────

/// Identifies a wizard configuration section.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SectionId {
    Welcome,
    Language,
    Providers,
    Embedding,
    Mcp,
    Connectors,
    Skills,
    Memory,
    Gateway,
}

impl SectionId {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Welcome => "welcome",
            Self::Language => "language",
            Self::Providers => "providers",
            Self::Embedding => "embedding",
            Self::Mcp => "mcp",
            Self::Connectors => "connectors",
            Self::Skills => "skills",
            Self::Memory => "memory",
            Self::Gateway => "gateway",
        }
    }

    /// All configurable sections (excluding Welcome and Language which are flow-control).
    pub const ALL_CONFIGURABLE: &[SectionId] = &[
        Self::Providers,
        Self::Embedding,
        Self::Mcp,
        Self::Connectors,
        Self::Skills,
        Self::Memory,
        Self::Gateway,
    ];
}

impl std::fmt::Display for SectionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

// ── Build output ─────────────────────────────────────────────────────────

/// Result of building a section — config fragment + optional secrets.
pub struct BuildOutput<T> {
    pub config: T,
    /// Secrets accumulated during build: `(env_var_name, plaintext)`.
    pub secrets: Vec<(String, String)>,
}

impl<T> BuildOutput<T> {
    pub fn new(config: T) -> Self {
        Self {
            config,
            secrets: Vec::new(),
        }
    }

    pub fn with_secrets(config: T, secrets: Vec<(String, String)>) -> Self {
        Self { config, secrets }
    }
}

/// A unit of configuration — UI-agnostic, async-capable.
///
/// Each section produces a typed config fragment (`Output`).
/// The UI layer handles i18n via `section.id()` + `field.key`.
///
/// The `build()` method receives an `InputCollector` and drives the
/// interactive flow itself — enabling conditional fields, multi-phase
/// collection, and dynamic options based on previous answers.
#[async_trait::async_trait]
pub trait ConfigSection {
    /// The config fragment this section produces.
    type Output: Default + Clone + Send + Sync;

    /// Unique ID for i18n resolution and CLI addressing.
    /// Convention: `wizard.{id}.{field_key}` for UI labels.
    fn id(&self) -> &str;

    /// Whether this section should be skipped given the current state.
    fn should_skip(&self, current: Option<&Self::Output>) -> bool;

    /// Describes all possible fields (for introspection / `config set`).
    /// Not used during wizard flow — `build()` drives collection directly.
    fn fields(&self, current: Option<&Self::Output>) -> Vec<FieldSpec>;

    /// Validates the built config fragment.
    fn validate(&self, fragment: &Self::Output) -> Result<(), Vec<String>>;

    /// Builds the config fragment by driving the collector interactively.
    ///
    /// The section controls the flow: it calls `collector.collect()` as many
    /// times as needed, with different field sets based on previous answers.
    /// Returns `None` if the user cancelled or went back.
    async fn build(
        &self,
        collector: &mut dyn InputCollector,
        current: Option<&Self::Output>,
    ) -> anyhow::Result<Option<BuildOutput<Self::Output>>>;

    /// Applies a single field value to an existing config fragment.
    ///
    /// Used by `config set` for atomic field updates without running
    /// the full interactive wizard. Returns the updated config + any secrets.
    fn apply_field(
        &self,
        _current: &Self::Output,
        _field_path: &str,
        _value: &str,
    ) -> anyhow::Result<BuildOutput<Self::Output>> {
        anyhow::bail!(
            "section '{}' does not support atomic field updates",
            self.id()
        )
    }
}

// ── Field specifications ───────────────────────────────────────────────────

/// Describes a single field the UI should collect.
pub struct FieldSpec {
    /// Field key — used for i18n: `wizard.{section_id}.{key}`.
    pub key: String,
    /// What kind of input this field expects.
    pub kind: FieldKind,
    /// Whether the field must have a non-empty value.
    pub required: bool,
}

impl FieldSpec {
    pub fn text(key: &str) -> Self {
        Self {
            key: key.to_string(),
            kind: FieldKind::Text { default: None },
            required: false,
        }
    }

    pub fn text_default(key: &str, default: &str) -> Self {
        Self {
            key: key.to_string(),
            kind: FieldKind::Text {
                default: Some(default.to_string()),
            },
            required: false,
        }
    }

    pub fn secret(key: &str) -> Self {
        Self {
            key: key.to_string(),
            kind: FieldKind::Secret,
            required: false,
        }
    }

    pub fn select(key: &str, options: Vec<SelectOption>, default: usize) -> Self {
        Self {
            key: key.to_string(),
            kind: FieldKind::Select { options, default },
            required: true,
        }
    }

    pub fn confirm(key: &str, default: bool) -> Self {
        Self {
            key: key.to_string(),
            kind: FieldKind::Confirm { default },
            required: false,
        }
    }

    pub fn multi_select(key: &str, options: Vec<SelectOption>) -> Self {
        Self {
            key: key.to_string(),
            kind: FieldKind::MultiSelect { options },
            required: false,
        }
    }

    pub fn required(mut self) -> Self {
        self.required = true;
        self
    }
}

/// What kind of input a field expects.
pub enum FieldKind {
    /// Free text input.
    Text { default: Option<String> },
    /// Masked text input (API keys, tokens).
    Secret,
    /// Single selection from a list.
    Select {
        options: Vec<SelectOption>,
        default: usize,
    },
    /// Yes/no confirmation.
    Confirm { default: bool },
    /// Multiple selection from a list.
    MultiSelect { options: Vec<SelectOption> },
}

/// A selectable option.
#[derive(Debug, Clone)]
pub struct SelectOption {
    /// Machine value (stored in config).
    pub value: String,
    /// Display label — resolved by UI via i18n, or used directly.
    pub label: String,
}

impl SelectOption {
    pub fn new(value: &str, label: &str) -> Self {
        Self {
            value: value.to_string(),
            label: label.to_string(),
        }
    }
}

// ── Collected values ───────────────────────────────────────────────────────

/// Values collected by the UI for a set of fields.
pub type FieldValues = HashMap<String, FieldValue>;

/// A single collected value.
#[derive(Debug, Clone)]
pub enum FieldValue {
    Text(String),
    Bool(bool),
    Index(usize),
    Indices(Vec<usize>),
}

impl FieldValue {
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::Text(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub fn as_index(&self) -> Option<usize> {
        match self {
            Self::Index(i) => Some(*i),
            _ => None,
        }
    }

    pub fn as_indices(&self) -> Option<&[usize]> {
        match self {
            Self::Indices(v) => Some(v),
            _ => None,
        }
    }
}

// ── Input collector trait ──────────────────────────────────────────────────

/// Result of a collect operation.
pub enum CollectResult {
    /// Values collected successfully.
    Values(FieldValues),
    /// User wants to go back.
    Back,
    /// User cancelled.
    Cancel,
}

/// UI adapter that collects field values from the user.
///
/// Implementations handle i18n, rendering, and input.
/// The section ID is passed for i18n resolution: `wizard.{section_id}.{field_key}`.
pub trait InputCollector: Send {
    /// Collects values for a set of fields.
    fn collect(
        &mut self,
        section_id: &str,
        fields: &[FieldSpec],
    ) -> anyhow::Result<CollectResult>;

    /// Displays a section title/header (e.g. "── Providers ──").
    fn show_title(&mut self, _section_id: &str) {}

    /// Displays an informational message (summaries, status).
    fn show_info(&mut self, _message: &str) {}

    /// Displays validation errors.
    fn show_errors(&mut self, _errors: &[String]) {}
}

// ── Helpers ──────────────────────────────────────────────────────────────

/// Asks the user whether to add more items. Returns `false` on Back/Cancel.
pub fn confirm_add_more(
    section_id: &str,
    collector: &mut dyn InputCollector,
    key: &str,
) -> anyhow::Result<bool> {
    let fields = vec![FieldSpec::confirm(key, false)];
    match collector.collect(section_id, &fields)? {
        CollectResult::Values(v) => Ok(v.get(key).and_then(FieldValue::as_bool).unwrap_or(false)),
        _ => Ok(false),
    }
}
