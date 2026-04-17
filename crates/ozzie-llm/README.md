# ozzie-llm

Multi-provider LLM client for Rust with streaming, tool calling, circuit breaker, and fallback chains.

Part of the [Ozzie](https://github.com/ozzie-assistant/ozzie) assistant.

## Features

- **10 providers** -- Anthropic, OpenAI, Gemini, Mistral, Groq, xAI, Ollama, OpenAI-compatible, LM Studio, vLLM
- **Unified `Provider` trait** -- `chat()` and `chat_stream()` across all backends
- **Multimodal** -- text and base64 images via `Content` enum
- **Tool calling** -- JSON Schema-based `ToolDefinition` with native support on all major providers
- **XML tool shim** -- automatic XML encoding for providers without native tool calling
- **Streaming** -- async `Stream<Item = ChatDelta>` for real-time output
- **Resilience** -- `FallbackProvider` with `CircuitBreaker` and ordered fallback chains
- **Pluggable auth** -- `SecretResolver` trait for custom credential backends (Vault, encrypted store, etc.)
- **Schema normalization** -- adapts JSON Schema to each provider's quirks automatically
- **Zero Ozzie dependency** -- standalone crate, usable in any Rust project

## Installation

```bash
cargo add ozzie-llm
```

## Usage

### Create a provider and chat

```rust
use ozzie_llm::{
    providers::AnthropicProvider, resolve_auth, Driver,
    ChatMessage, ChatRole, EnvSecretResolver,
};

// Resolve API key from environment
let auth = resolve_auth(
    Driver::Anthropic,
    None, // api_key override
    None, // token override
    &EnvSecretResolver,
)?;

let provider = AnthropicProvider::new(
    auth,
    Some("claude-sonnet-4-20250514"),
    None,  // base_url
    None,  // max_tokens
    None,  // timeout
);

let messages = vec![
    ChatMessage::text(ChatRole::System, "You are a helpful assistant."),
    ChatMessage::text(ChatRole::User, "What is a wormhole?"),
];

let response = provider.chat(&messages, &[]).await?;
println!("{}", response.content);
println!("Tokens: {} in / {} out", response.usage.input_tokens, response.usage.output_tokens);
```

### Streaming

```rust
use futures_util::StreamExt;
use ozzie_llm::ChatDelta;

let mut stream = provider.chat_stream(&messages, &[]).await?;

while let Some(delta) = stream.next().await {
    match delta? {
        ChatDelta::Content(text) => print!("{text}"),
        ChatDelta::Done { usage, .. } => {
            println!("\nTokens: {} in / {} out", usage.input_tokens, usage.output_tokens);
        }
        _ => {}
    }
}
```

### Tool calling

```rust
use ozzie_llm::ToolDefinition;

let tools = vec![ToolDefinition {
    name: "get_weather".into(),
    description: "Get current weather for a city".into(),
    parameters: schemars::schema_for!(WeatherParams),
}];

let response = provider.chat(&messages, &tools).await?;

for tc in &response.tool_calls {
    println!("Tool: {}({})", tc.name, tc.arguments);
}
```

### Fallback with circuit breaker

```rust
use ozzie_llm::{FallbackProvider, CircuitBreakerConfig};

let primary = AnthropicProvider::new(/* ... */);
let fallback = OpenAIProvider::new(/* ... */);
let local = OllamaProvider::new("llama3", Some("http://localhost:11434"), None, true);

let provider = FallbackProvider::new(
    vec![
        Arc::new(primary),
        Arc::new(fallback),
        Arc::new(local),
    ],
    CircuitBreakerConfig::default(),
);

// Tries Anthropic first. On failure, trips circuit breaker after N errors,
// falls through to OpenAI, then Ollama.
let response = provider.chat(&messages, &[]).await?;
```

### Custom secret resolver

```rust
use ozzie_llm::SecretResolver;

struct VaultResolver { /* ... */ }

impl SecretResolver for VaultResolver {
    fn get(&self, key: &str) -> Option<String> {
        // Fetch from HashiCorp Vault, AWS Secrets Manager, etc.
        todo!()
    }
}
```

### Multimodal (images)

```rust
use ozzie_llm::{ChatMessage, ChatRole, Content};

let message = ChatMessage {
    role: ChatRole::User,
    content: vec![
        Content::text("What's in this image?"),
        Content::image("image/png", base64_data),
    ],
    tool_calls: vec![],
    tool_call_id: None,
};
```

## Supported providers

| Driver | Streaming | Tool calling | Images | Notes |
|--------|-----------|-------------|--------|-------|
| Anthropic | yes | native | yes | Claude models |
| OpenAI | yes | native | yes | GPT + o-series |
| Gemini | yes | native | yes | Google AI |
| Mistral | yes | native | no | Mistral AI |
| Groq | yes | native | no | Fast inference |
| xAI | yes | native | no | Grok models |
| Ollama | yes | native/shim | yes | Local models |
| OpenAI-compatible | yes | native | varies | Any OpenAI-API server |
| LM Studio | yes | native | varies | Local |
| vLLM | yes | native | varies | Local |

## License

MIT
