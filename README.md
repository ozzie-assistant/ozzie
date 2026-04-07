# Ozzie

**Your personal AI agent operating system.**

> *Connect everything. Trust nothing. Know your human.*

Ozzie is a self-hosted, event-driven personal AI agent — not a generalist code monkey, but an honest companion
that knows its user, challenges bad ideas, delegates to experts when it should, and keeps you on track.

Built with zero-trust security from day one, multi-provider LLM support (cloud and local), and a gateway
architecture that connects to any tool, messaging platform, or workflow.

Named after [Ozzie Isaacs](https://en.wikipedia.org/wiki/Commonwealth_Saga) from Peter F. Hamilton's *Commonwealth
Saga* — co-inventor of wormholes, creator of Sentient Intelligence, and architect of the Gaiafield.

---

## Why Ozzie?

Most AI agent frameworks let plugins access your filesystem, leak secrets at runtime, and mix instructions with
executable code. Ozzie takes a different approach:

- **Zero-trust by default** — Sandbox blocks destructive commands, dangerous tools require explicit approval
- **Personal agent** — Learns who you are (profile, memory, consolidation), adapts tone and context, delegates to
  specialist tools/agents rather than hallucinating
- **Gateway pattern** — One persistent process (`ozzie gateway`) orchestrates everything; clients (CLI, web, connectors)
  connect ephemerally via WebSocket
- **Event-driven backbone** — Every action (message, tool call, task event) is an immutable event — auditable, replayable,
  human-readable
- **Multi-LLM** — 7 drivers (Anthropic, OpenAI, Gemini, Mistral, Groq, Ollama, xAI) with fallback chains, circuit
  breaker, and local-first SLM support
- **Async task delegation** — Background tasks with dependency chains, multi-step plans, crash recovery
- **Semantic memory** — Hybrid retrieval (keyword + vector), decay model, LLM-based consolidation, implicit injection
- **Layered context compression** — L0/L1/L2 hierarchical compression for long conversations
- **MCP client & server** — Connect external MCP servers (with `trusted_tools`); expose Ozzie tools via MCP for Claude Code
- **Dynamic tool activation** — Core tools always active, plugin/MCP tools activated on demand via `activate`
- **Dangerous tool approval** — Interactive 3-option prompt (allow once / always for session / deny)
- **Container-ready** — Multi-arch Docker images, 5-target CI/CD, GitHub Actions

## Architecture

```
┌───────────────────────────────────────────────┐
│           GATEWAY  (ozzie gateway)            │
│           127.0.0.1:18420                     │
├───────────────────────────────────────────────┤
│  Agent Runtime (ReAct loop)                   │
│  ├─ EventRunner (event bus → LLM → tools)     │
│  ├─ Task Runner (async, deps, recovery)       │
│  ├─ Scheduler (cron, interval, event)         │
│  └─ Connectors (Discord, File, extensible)   │
│                                               │
│  Core Domain (ozzie-core)                     │
│  ├─ Event Bus (35+ typed events)              │
│  ├─ Conscience (AST sandbox, permissions)     │
│  ├─ User Profile (acquaintance, whoami)       │
│  ├─ Layered Context (L0/L1/L2 + BM25)        │
│  ├─ Skill Engine (DAG workflows)              │
│  └─ Prompt System (composer, sections)        │
│                                               │
│  Tools (ozzie-tools)                          │
│  ├─ 15+ native tools                         │
│  ├─ MCP client (rmcp, stdio)                 │
│  └─ Tool registry (JSON Schema)              │
│                                               │
│  Memory (ozzie-memory)                        │
│  └─ SQLite + FTS5 + vector + consolidation   │
└────────────────────┬──────────────────────────┘
                     │ WebSocket (JSON-RPC 2.0)
           ┌─────────┼─────────┐
           ↓         ↓         ↓
         CLI      Discord    Web
      (chat/ask)  (bridge)  (Leptos, planned)
```

## Quick Start

```bash
# Build
cargo build --release --package ozzie-cli

# First-time setup
./target/release/ozzie wake

# Start the gateway
./target/release/ozzie gateway

# In another terminal
./target/release/ozzie ask "Hello, who are you?"
```

### Docker

```bash
# Pre-built image
docker run -d \
  -v ~/.ozzie:/home/ozzie/.ozzie \
  -p 18420:18420 \
  ghcr.io/dohr-michael/ozzie:latest

# Or build locally
make docker
```

## Native Tools

| Tool | Category | Description |
|------|----------|-------------|
| `execute` | Execution | Shell commands (sandbox + constraints) |
| `git` | Execution | Git operations (status, diff, log, add, commit, ...) |
| `file_read` / `file_write` | Filesystem | Read and write files |
| `list_dir` / `glob` / `grep` | Filesystem | Explore filesystem |
| `str_replace_editor` | Filesystem | Rich editor (view, create, str_replace, insert, undo) |
| `web_fetch` / `web_search` | Web | Fetch pages, search the web |
| `store_memory` / `query_memories` / `forget_memory` | Memory | Persistent semantic memory |
| `schedule_task` / `list_schedules` / `unschedule_task` / `trigger_schedule` | Schedule | Recurring tasks (cron, interval, event) |
| `run_subtask` | Autonomy | Delegate to sub-ReAct loop (depth max 3) |
| `agent_{name}` | Autonomy | User-configured sub-agents with dedicated persona, model, and tools |
| `activate` / `tool_search` | Control | Discover and activate on-demand tools and skills |
| `update_session` | Control | Update session metadata |
| `yield_control` | Control | Cooperative loop yield (done / waiting / checkpoint) |

## MCP Integration

### As MCP Server

Expose Ozzie tools to Claude Code or any MCP-compatible client:

```bash
ozzie mcp-serve
```

Add to `.mcp.json`:

```json
{
    "mcpServers": {
        "ozzie": {
            "type": "stdio",
            "command": "./target/release/ozzie",
            "args": ["mcp-serve"],
            "env": { "OZZIE_PATH": "./dev_home" }
        }
    }
}
```

### As MCP Client

Connect to external MCP servers in `config.jsonc`:

```jsonc
{
    "mcp": {
        "servers": {
            "my-server": {
                "transport": "stdio",
                "command": "my-mcp-server",
                "dangerous": true,
                "trusted_tools": ["read_only_tool"],
                "denied_tools": ["destructive_tool"]
            }
        }
    }
}
```

## Development

```bash
make check          # Quality gates (check + clippy + test) — all --release
make build          # Build release binary
make test           # Run tests
make lint           # Run clippy
make run-gateway    # Start gateway (dev)
make run-ask        # Send test message
make docker         # Build local Docker image
make clean          # Clean build artifacts
```

### Quality gates

Every change must pass all three — no exceptions:

```bash
cargo check --workspace     # compile
cargo clippy --workspace    # lint (zero warnings)
cargo test --workspace      # tests
```

## Tech Stack

| Component     | Choice               | Why                                                 |
|---------------|----------------------|-----------------------------------------------------|
| Language      | **Rust (2024 edition)** | Performance, safety, single static binary        |
| HTTP/WS       | **axum**             | Async, tower-based, ws built-in                     |
| LLM Drivers   | 7 providers          | Anthropic, OpenAI, Gemini, Mistral, Groq, Ollama, xAI |
| MCP           | **rmcp**             | Rust MCP SDK, stdio + HTTP/SSE transport            |
| Memory        | **rusqlite** (bundled) | SQLite FTS5 + brute-force vector, no external deps |
| Secrets       | **age**              | Encryption for .env secrets                         |
| CLI           | **clap**             | Derive-based CLI parsing                            |
| Async         | **tokio**            | Full-featured async runtime                         |
| Config        | **JSONC**            | Comments in JSON, env variable templating           |
| Build/Release | **cargo + Docker**   | Multi-arch images, 5-target CI matrix               |
| CI/CD         | **GitHub Actions**   | Quality gates + smoke tests + semver release        |

## Workspace Layout

```
crates/
  ozzie-types/             → shared payload types (zero logic, serde only)
  ozzie-protocol/          → JSON-RPC 2.0 protocol (Frame, Request enum, EventKind)
  ozzie-core/              → pure domain (zero infra deps, DDD hexagonal)
  ozzie-llm/               → LLM providers (7 drivers)
  ozzie-runtime/           → agent runtime, sessions, tasks, scheduler
  ozzie-tools/             → tool registry, native tools, MCP client
  ozzie-gateway/           → HTTP server (axum) + WebSocket hub + auth
  ozzie-memory/            → semantic memory (SQLite + FTS5 + vector)
  ozzie-client/            → WebSocket client SDK (for connectors)
  ozzie-utils/             → shared utilities (i18n, names, secrets, config)
  ozzie-cli/               → CLI binary (single binary: gateway, ask, chat, config)

connectors/
  ozzie-discord-bridge/    → Discord connector (serenity + OzzieClient)
  ozzie-file-bridge/       → File JSONL connector (dev/testing)

clients/
  ozzie-tui/               → TUI (deprecated — Leptos/Tauri planned)
```

## Status

### Working (validated)

| Feature                         | Status    | Notes                                                  |
|---------------------------------|-----------|--------------------------------------------------------|
| Gateway (axum + WS hub + auth)  | done      | JSON-RPC 2.0, 35+ event types, bearer token auth      |
| ReAct loop                      | done      | Budget (50 turns / 32k tokens / 5min), subtasks, yield |
| 7 LLM providers                 | done      | Streaming + tool calling on all, fallback + circuit breaker |
| 15+ native tools                | done      | File, shell, git, web, memory, scheduler, subtask, sub-agents |
| AST sandbox                     | done      | brush-parser, path jail, dangerous tool approval       |
| Semantic memory                 | done      | SQLite + FTS5 + cosine, hybrid search, decay model     |
| User profile                    | done      | Wizard acquaintance, LLM synthesis, prompt injection   |
| Layered context compression     | done      | L0/L1/L2, BM25, heuristic fallback without LLM        |
| Skill engine                    | done      | Markdown + YAML DAG, parallel steps, triggers          |
| Discord connector               | done      | Slash commands, guild config, roles, pairing, policies |
| File connector                  | done      | JSONL in/out for dev/testing                           |
| Process supervisor              | done      | Spawn, monitor (2s), restart on crash, graceful shutdown |
| MCP client                      | done      | rmcp, stdio, trusted/denied/allowed tools              |
| CLI (chat, ask, wake, ...)      | done      | 12 commands, all functional                            |
| Secrets management              | done      | age encryption, set/list/delete/rotate                 |
| CI/CD                           | done      | 5-target matrix, Docker multi-arch, GitHub Release     |
| Benchmark suite                 | done      | 32 tests, 9 categories, 236 pts. Gemini Flash 87%, Qwen3 30B 81% |

### In progress / Gaps

| Feature                              | Status       | What's missing                                           |
|--------------------------------------|--------------|----------------------------------------------------------|
| Memory consolidation ("dream" job)   | infra ready  | Scheduled job, triage (profile vs factual), profile enrichment from conversations |
| MCP server                           | partial      | Experimental, minimal tests                              |
| Discord e2e tests                    | missing      | Zero e2e tests on the full connector flow                |
| DM-initiated pairing                 | partial      | `/pair` slash command exists, not DM-triggered           |
| Guild invite handling                | missing      | No `guild_create` event handler                          |
| Self-improvement (config)            | planned      | Agent should add skills/connectors/config via conversation |
| Observability                        | basic        | JSONL event logs, no rotation, no metrics, no OTel       |

### Planned (long-term)

| Feature                              | Notes                                                    |
|--------------------------------------|----------------------------------------------------------|
| Web UI (Leptos + Tauri)              | Admin portal + chat, replaces deprecated TUI             |
| Claude Code connector                | "Channels" bridge — delegate coding tasks to expert agent |
| WASM plugin system (WIT)             | Sandboxed tool + connector plugins. Tools first, connectors blocked on WASM stream maturity |
| Recursive self-improvement           | Agent modifies own source code. Needs safety framework + benchmark as fitness function |
| Vision / image support               | Not in provider layer yet                                |
| Connector abstraction trait          | Shared `ConnectorBridge` base to reduce duplication across bridges |

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.
