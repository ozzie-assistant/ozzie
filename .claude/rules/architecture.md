# Architecture

@.claude/claude-rules/rules/architecture/hexagonal.md

Ozzie follows the **gateway pattern**: one persistent process (`ozzie gateway`) orchestrates everything, clients connect via WebSocket (JSON-RPC 2.0).

## Workspace layout

```
crates/              → core + server
  ozzie-types/         → shared payload types (publishable)
  ozzie-protocol/      → JSON-RPC 2.0 protocol (publishable)
  ozzie-core/          → pure domain (zero infra dependency)
  ozzie-llm/           → LLM providers (anthropic, openai, gemini, mistral, groq, ollama, xai)
  ozzie-runtime/       → agent runtime (EventRunner, ReactLoop, conversations, tasks, scheduler)
  ozzie-tools/         → tool registry, native tools, MCP client
  ozzie-gateway/       → HTTP server (axum) + WebSocket hub + auth
  ozzie-memory/        → semantic memory (SQLite + FTS5 + vector)
  ozzie-client/        → WebSocket client SDK
  ozzie-utils/         → shared utilities
  ozzie-cli/           → CLI binary (single binary: gateway, ask, chat, config)

connectors/          → standalone JSON-RPC bridges
  ozzie-discord-bridge/  → Discord (serenity + OzzieClient)
  ozzie-file-bridge/     → File JSONL (dev/testing)

clients/             → UI clients
  ozzie-tui/           → TUI (deprecated — Leptos/Tauri planned)
```

## OZZIE_PATH

All Ozzie data lives under a single root directory:
- `$OZZIE_PATH` if set, otherwise `~/.ozzie`
- Created by `ozzie wake` (onboarding command)
- Contains: `config.jsonc`, `.env`, `logs/`, `skills/`, `conversations/`, `memory/`, `tasks/`
- Resolved via `config::ozzie_path()`, `config::config_path()`, `config::dotenv_path()`

## Key files

| What              | Where                                              |
|-------------------|----------------------------------------------------|
| CLI entry point   | `crates/ozzie-cli/src/main.rs`                     |
| CLI commands      | `crates/ozzie-cli/src/commands/`                   |
| Config            | `ozzie-core/src/config/`                           |
| Domain ports      | `ozzie-core/src/domain/ports.rs`                   |
| Event bus         | `ozzie-core/src/events/`                           |
| Prompt system     | `ozzie-core/src/prompt/`                           |
| Conscience        | `ozzie-core/src/conscience/` (sandbox, permissions) |
| Layered context   | `ozzie-core/src/layered/` (L0/L1/L2 + BM25)       |
| Skills            | `ozzie-core/src/skills/` (DAG, loader)             |
| Policy            | `ozzie-core/src/policy/` (pairing, resolver)       |
| Connector types   | `ozzie-core/src/connector/` (Identity, messages)   |
| User profile      | `ozzie-core/src/profile/` (UserProfile, WhoamiEntry) |
| Agent runtime     | `ozzie-runtime/src/event_runner.rs` + `react.rs`   |
| Conversation runtime | `ozzie-runtime/src/conversation_runtime.rs` + `conversation_registry.rs` |
| ProcessSupervisor | `ozzie-runtime/src/connector/supervisor.rs`        |
| Tool registry     | `ozzie-tools/src/registry.rs`                      |
| Native tools      | `ozzie-tools/src/native/`                          |
| MCP client        | `ozzie-tools/src/mcp/`                             |
| LLM drivers       | `ozzie-llm/src/providers/`                         |
| Gateway           | `ozzie-gateway/src/`                               |
| Memory            | `ozzie-memory/src/`                                |
| Wiki pages        | `ozzie-memory/src/page_store.rs` + `page_frontmatter.rs` |
| Wiki domain       | `ozzie-core/src/domain/wiki.rs` + `memory_schema.rs` |
| Dream pipeline    | `ozzie-runtime/src/dream/` (classifier, synthesizer, lint, index) |
| Page retriever    | `ozzie-runtime/src/page_retriever.rs`              |

## Key conventions

- **Event-driven**: components communicate through the event bus, not direct calls
- **Config**: JSONC with `${{ .Env.VAR }}` templating. Defaults applied in `config::load()`
- **DDD Hexagonal**: `ozzie-core` must never depend on infra crates. Domain ports in `ozzie-core/src/domain/ports.rs`, adapters in `ozzie-runtime` and `ozzie-tools`
- **Models**: 7 drivers, lazy-init. Auth: config → env var → driver default. FallbackProvider with circuit breaker
- **CLI**: clap derive macros. Entry: `crates/ozzie-cli/src/main.rs`
- **Protocol**: JSON-RPC 2.0 over WebSocket. Spec: `docs/openrpc.json` + `docs/ws-protocol.md`
