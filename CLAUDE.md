# Ozzie — Agent OS

@README.md

## Current state

Full agent OS with 7 LLM drivers (anthropic, openai, gemini, mistral, groq, ollama, xai),
ReAct loop, async tasks, semantic memory + wiki pages, user profile, MCP client/server (rmcp),
skill engine, scheduler, connector system (Discord, File), and dangerous tool approval flow.

Working: `ozzie gateway` → `ozzie ask "hello"` → streamed LLM response with tool calling.

## CI/CD

- **Snapshot** (PR/main): 5-target quality matrix + smoke tests (binary + Docker)
- **Release** (tag `v*`): quality + release binaries (5 targets) + Docker multi-arch + GitHub Release
- Version: `OZZIE_VERSION` env var → `build.rs` → `ozzie --version`

## Shared rules

Reusable rules from `.claude/claude-rules/` (git submodule):

@.claude/claude-rules/rules/rust/error-handling.md
@.claude/claude-rules/rules/rust/logging.md
