---
description: Ozzie domain concepts — tools, MCP, connectors, memory, skills, sandbox, ReAct loop
---

# Key Concepts

- **Tool schema** — Tool args derive `schemars::JsonSchema`. `SchemaAdapter` trait customizes schema per LLM provider.
- **ToolSet** — Two-tier: core tools always active, plugin/MCP tools activated on demand via `activate`. `ToolRegistry` stores `ToolSpec` with JSON Schema parameters.
- **Dangerous tool approval** — `DangerousToolWrapper` prompts user (allow once / always for session / deny). Approvals in `Session.ApprovedTools`.
- **MCP** — Client via `rmcp` (stdio). Server via `rmcp::ServerHandler` (stdio + HTTP/SSE). External servers in `config.mcp.servers`. Tools dangerous by default unless `trusted_tools`.
- **Connectors** — Standalone JSON-RPC bridge processes managed by `ProcessSupervisor`. Config: `config.connectors` with `ConnectorProcessConfig` (command, args, env, config, auto_pair, restart). Env vars: `OZZIE_GATEWAY_URL`, `OZZIE_GATEWAY_TOKEN`, `OZZIE_CONNECTOR_CONFIG`.
- **Entity IDs** — `names::generate_id("task", exists_fn)` → `task_cosmic_asimov`. SF-themed, human-readable.
- **Memory** — SQLite + FTS5 + brute-force cosine similarity. Multi-level decay. LLM-based consolidation.
- **Skills** — `WorkflowRunner` executes DAG of steps with parallel execution via `tokio::spawn`.
- **Prompt system** — `Composer` assembles sections (persona, profile, tools, memory, skills). Section builders are pure functions.
- **Sandbox** — AST-based command validation via `brush-parser`. Path jail enforcement.
- **Sub-agents** — User-configured in `config.sub_agents`, each becomes tool `agent_{name}`. One-shot ReactLoop with custom persona/model/tools. No nesting. No user profile/memories. Dangerous approvals bubble up to parent via shared event bus. `SubAgentRunner` port in `ozzie-core`, `DirectSubAgentRunner` impl in gateway.
- **ReactLoop** — `ReactObserver` for event bridging. `SessionRuntime` per session (CancellationToken + message buffer). `yield_control` for cooperative yield. `cancel_session` for explicit cancellation.
