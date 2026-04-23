# Ozzie Gateway — API Reference for Tauri Frontend

> This document describes all available services for the Tauri (Leptos) app.
> The gateway runs at `http://localhost:18420` and exposes REST + WebSocket APIs.

## Authentication

All protected routes require a Bearer token:
- **Header:** `Authorization: Bearer <token>`
- **Query param (WS only):** `?token=<token>`
- **Token location:** `$OZZIE_PATH/.token` (auto-generated on first run)

Device pairing flow available for first-time clients (see Pairing section).

---

## WebSocket — Chat & Agent Interaction

### Connection

```
ws://localhost:18420/api/ws?token=<token>
```

### Protocol: JSON-RPC 2.0

All messages are JSON text frames. Three message types:
- **Request** (client → server): `{jsonrpc, id, method, params}`
- **Response** (server → client): `{jsonrpc, id, result/error}`
- **Notification** (server → client): `{jsonrpc, method, params}` (no `id`)

### Methods (Client → Server)

| Method | Params | Result | Description |
|--------|--------|--------|-------------|
| `open_session` | `{conversation_id?, working_dir?, language?, model?}` | `{conversation_id, root_dir?}` | Create/resume session |
| `send_message` | `{conversation_id, text, images?}` | `{accepted: true}` | Send user message (triggers LLM) |
| `send_connector_message` | `{connector, channel_id, author, content, message_id?}` | `{accepted: true}` | Route connector message |
| `load_messages` | `{conversation_id, limit?}` | `{messages: [...]}` | Load conversation history |
| `accept_all_tools` | `{conversation_id}` | `{accepted: true}` | Auto-approve all tools |
| `prompt_response` | `{token, value?, text?}` | `{accepted: true}` | Reply to prompt |
| `cancel_session` | `{conversation_id}` | `{cancelled: true}` | Cancel active ReactLoop |

### Image Attachments

`send_message` supports multimodal input:
```json
{
  "conversation_id": "sess_xyz",
  "text": "What's in this image?",
  "images": [
    { "base64": "iVBORw0K...", "media_type": "image/png", "alt": "screenshot" }
  ]
}
```

### Notifications (Server → Client)

#### Tier 1 — Required

| Event | Params | Description |
|-------|--------|-------------|
| `assistant.stream` | `{conversation_id, phase, content, index}` | Streaming LLM output. Phase: `start` / `delta` / `end` |
| `assistant.message` | `{conversation_id, content, error?}` | Final complete response |
| `prompt.request` | `{conversation_id, prompt_type, label, token, options}` | Needs user input → reply with `prompt_response` |

#### Tier 2 — Recommended

| Event | Params | Description |
|-------|--------|-------------|
| `tool.call` | `{conversation_id, call_id, tool, arguments}` | Tool invocation started |
| `tool.result` | `{conversation_id, call_id, tool, result, is_error}` | Tool execution result |
| `tool.progress` | `{conversation_id, call_id, tool, message}` | Progress update |
| `agent.cancelled` | `{conversation_id, reason}` | ReactLoop cancelled |
| `agent.yielded` | `{conversation_id, reason, resume_on?}` | Agent yielded (`done` / `waiting` / `checkpoint`) |

#### Tier 3 — Optional

| Event | Params | Description |
|-------|--------|-------------|
| `session.created` / `session.closed` | `{conversation_id}` | Session lifecycle |
| `skill.started` / `skill.completed` | `{conversation_id, ...}` | Skill execution |
| `internal.llm.call` | `{conversation_id, phase, tokens_input, tokens_output}` | LLM telemetry |
| `dream.completed` | `{sessions_processed, memories_created, ...}` | Consolidation done |

### Typical Flows

**Basic conversation:**
```
open_session → send_message → assistant.stream (start/delta/end) → assistant.message
```

**Tool with approval:**
```
send_message → tool.call → prompt.request → prompt_response → tool.result → assistant.message
```

**Cancel:**
```
cancel_session → agent.cancelled (session reusable for next message)
```

---

## REST API — Data & Admin

### Sessions

| Method | Path | Auth | Description |
|--------|------|------|-------------|
| `GET` | `/api/sessions` | No | List all sessions with metadata (id, status, title, message_count, token_usage) |

### Memory & Wiki

All read-only. Writes go through agent tools (`store_memory`, `forget_memory`) or the dream consolidation job.

| Method | Path | Auth | Response |
|--------|------|------|----------|
| `GET` | `/api/memory/entries` | Yes | `{entries: [{id, title, type, tags, importance, confidence, created_at, updated_at}]}` |
| `GET` | `/api/memory/entries/{id}` | Yes | `{id, title, type, tags, content, importance, confidence, source, created_at, updated_at}` |
| `GET` | `/api/memory/entries/search?q=...&limit=20` | Yes | `{results: [{id, title, type, tags}]}` |
| `GET` | `/api/memory/pages` | Yes | `{pages: [{id, title, slug, tags, source_ids, revision, created_at, updated_at}]}` |
| `GET` | `/api/memory/pages/{slug}` | Yes | `{id, title, slug, tags, source_ids, revision, content, created_at, updated_at}` |
| `GET` | `/api/memory/pages/search?q=...&limit=20` | Yes | `{results: [{id, title, slug, tags}]}` |
| `GET` | `/api/memory/index` | Yes | `{pages: [{title, slug, source_count, revision}], total_entries, uncategorized_count}` |
| `GET` | `/api/memory/schema` | Yes | `{max_page_chars, language, instructions}` |

**Wiki architecture:** The dream job (every 12h) clusters memory entries by shared tags and synthesizes thematic wiki pages via LLM. Pages are higher-signal summaries. The retriever searches pages first, then falls back to individual entries.

### User Profile

| Method | Path | Auth | Response |
|--------|------|------|----------|
| `GET` | `/api/profile` | Yes | `{name, tone?, language?, whoami: [{info, source, created_at}], created_at, updated_at}` |
| `PUT` | `/api/profile` | Yes | Request: `{name?, tone?, language?}` → `{ok: true}` |
| `GET` | `/api/profile/whoami` | Yes | `{whoami: [{info, source, created_at}]}` |

### Device Pairing

For first-time app connection. Same-home shortcut: if the app sends the gateway's device key, pairing is auto-approved.

| Method | Path | Auth | Description |
|--------|------|------|-------------|
| `POST` | `/api/pair` | No | `{client_type: "tauri", device_key?}` → `{request_id, expires_at}` |
| `GET` | `/api/pair/{id}` | No | Poll → `{status, device_id?, token?}` (15min TTL) |
| `GET` | `/api/pairings/requests` | Yes | List pending requests (admin) |
| `POST` | `/api/pairings/requests/{id}/approve` | Yes | Approve request |
| `POST` | `/api/pairings/requests/{id}/reject` | Yes | Reject request |

### Events History

| Method | Path | Auth | Description |
|--------|------|------|-------------|
| `GET` | `/api/events?limit=50&type=...&session=...` | Yes | Recent events (all 44 event types) |

### Health

| Method | Path | Auth | Description |
|--------|------|------|-------------|
| `GET` | `/api/health` | No | `{status: "ok"}` |

---

## Data Directory

All data lives under `$OZZIE_PATH` (default: `~/.ozzie`):

```
$OZZIE_PATH/
├── .token                    # Auth token (read this for API access)
├── .key                      # Device key (for same-home pairing shortcut)
├── config.jsonc              # Main config (providers, connectors, etc.)
├── profile.jsonc             # User profile
├── memory_schema.md          # Wiki governance (max_page_chars, language, conventions)
├── memory/
│   ├── pages/                # Wiki pages (markdown SsoT)
│   │   ├── _index.md         # Auto-generated catalogue
│   │   └── {slug}.md
│   ├── {slug}_{id}.md        # Memory entries (markdown SsoT)
│   └── .cache/memory.db      # SQLite FTS5 index
├── sessions/                 # Session history
├── logs/                     # Event logs (JSONL)
└── skills/                   # Installed skills
```

---

## What's NOT Yet Available (Planned)

- **Wizard/Onboarding API** — currently CLI-only (`ozzie wake`). Will be exposed as REST state machine.
- **Config API** — read/write runtime config
- **Tools/Skills listing** — introspect available tools and skills
- **Connector management** — list/restart connector processes
- **Metrics/Usage** — token usage, cost tracking per session

## Full Protocol Spec

See `docs/ws-protocol.md` for the complete JSON-RPC 2.0 specification and `docs/openrpc.json` for the machine-readable OpenRPC spec.
