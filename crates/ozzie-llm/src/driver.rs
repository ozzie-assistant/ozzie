use serde::{Deserialize, Serialize};

/// LLM driver — identifies which provider backend to use.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Driver {
    #[serde(rename = "anthropic")]
    #[default]
    Anthropic,
    #[serde(rename = "openai")]
    OpenAi,
    #[serde(rename = "gemini")]
    Gemini,
    #[serde(rename = "mistral")]
    Mistral,
    #[serde(rename = "ollama")]
    Ollama,
    #[serde(rename = "groq")]
    Groq,
    #[serde(rename = "xai")]
    Xai,
    #[serde(rename = "openai-compatible")]
    OpenAiCompatible,
    #[serde(rename = "lm-studio")]
    LmStudio,
    #[serde(rename = "vllm")]
    Vllm,
}

impl Driver {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Anthropic => "anthropic",
            Self::OpenAi => "openai",
            Self::Gemini => "gemini",
            Self::Mistral => "mistral",
            Self::Ollama => "ollama",
            Self::Groq => "groq",
            Self::Xai => "xai",
            Self::OpenAiCompatible => "openai-compatible",
            Self::LmStudio => "lm-studio",
            Self::Vllm => "vllm",
        }
    }

    /// Human-readable label for UI display.
    pub const fn display_name(self) -> &'static str {
        match self {
            Self::Anthropic => "Anthropic (Claude)",
            Self::OpenAi => "OpenAI",
            Self::Gemini => "Gemini (Google)",
            Self::Mistral => "Mistral",
            Self::Ollama => "Ollama (local)",
            Self::Groq => "Groq",
            Self::Xai => "xAI (Grok)",
            Self::OpenAiCompatible => "OpenAI-compatible (custom)",
            Self::LmStudio => "LM Studio",
            Self::Vllm => "vLLM",
        }
    }

    /// Environment variable name for the API key.
    pub const fn env_var(self) -> &'static str {
        match self {
            Self::Anthropic => "ANTHROPIC_API_KEY",
            Self::OpenAi | Self::OpenAiCompatible | Self::LmStudio | Self::Vllm => "OPENAI_API_KEY",
            Self::Gemini => "GEMINI_API_KEY",
            Self::Mistral => "MISTRAL_API_KEY",
            Self::Ollama => "OLLAMA_API_KEY",
            Self::Groq => "GROQ_API_KEY",
            Self::Xai => "XAI_API_KEY",
        }
    }

    /// Whether this driver requires an API key.
    pub const fn needs_api_key(self) -> bool {
        !matches!(
            self,
            Self::Ollama | Self::OpenAiCompatible | Self::LmStudio | Self::Vllm
        )
    }

    /// Whether this driver needs a custom base URL.
    pub const fn needs_base_url(self) -> bool {
        matches!(self, Self::Ollama | Self::OpenAiCompatible | Self::LmStudio | Self::Vllm)
    }

    /// Default base URL, if any.
    pub const fn default_base_url(self) -> Option<&'static str> {
        match self {
            Self::Ollama => Some("http://localhost:11434"),
            _ => None,
        }
    }

    /// Drivers available for LLM provider configuration.
    pub const ALL_LLM: &[Driver] = &[
        Self::Anthropic,
        Self::OpenAi,
        Self::Gemini,
        Self::Mistral,
        Self::Groq,
        Self::Xai,
        Self::Ollama,
        Self::OpenAiCompatible,
    ];

    /// Drivers available for embedding configuration.
    pub const ALL_EMBEDDING: &[Driver] = &[
        Self::OpenAi,
        Self::Mistral,
        Self::Gemini,
        Self::Ollama,
        Self::OpenAiCompatible,
    ];
}

impl std::fmt::Display for Driver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for Driver {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "anthropic" => Ok(Self::Anthropic),
            "openai" => Ok(Self::OpenAi),
            "gemini" => Ok(Self::Gemini),
            "mistral" => Ok(Self::Mistral),
            "ollama" => Ok(Self::Ollama),
            "groq" => Ok(Self::Groq),
            "xai" => Ok(Self::Xai),
            "openai-compatible" => Ok(Self::OpenAiCompatible),
            "lm-studio" => Ok(Self::LmStudio),
            "vllm" => Ok(Self::Vllm),
            other => Err(format!("unknown driver: {other}")),
        }
    }
}
