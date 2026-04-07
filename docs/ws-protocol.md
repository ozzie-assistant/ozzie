# Ozzie WebSocket Protocol v2 (JSON-RPC 2.0)

> Reference specification for building Ozzie connectors (TUI, Web, Discord, Slack, ...).
> All connectors communicate with the Ozzie Gateway through this single protocol.
>
> Machine-readable spec: [`docs/openrpc.json`](openrpc.json)

## Table of Contents

- [Overview](#overview)
- [Connection](#connection)
- [Frame Format (JSON-RPC 2.0)](#frame-format-json-rpc-20)
- [Methods (Client → Server)](#methods-client--server)
- [Notifications (Server → Client)](#notifications-server--client)
- [Flows](#flows)
- [HTTP Endpoints](#http-endpoints)
- [Implementing a Connector](#implementing-a-connector)

---

## Overview

```
┌──────────┐      WebSocket (JSON-RPC 2.0 frames)     ┌──────────────┐
│ Connector│◄─────────────────────────────────────────►│ Ozzie Gateway│
│ (TUI,Web)│  requests + notifications (server-push)    │  :18420      │
└──────────┘                                            └──────┬───────┘
                                                               │
                                                         ┌─────▼─────┐
                                                         │ Event Bus │
                                                         └─────┬─────┘
                                                               │
                                                  ┌────────────┼────────────┐
                                                  │            │            │
                                            ┌─────▼──┐  ┌─────▼──┐  ┌─────▼──┐
                                            │ Agent  │  │ Tasks  │  │ Skills │
                                            └────────┘  └────────┘  └────────┘
```

The protocol uses **JSON-RPC 2.0** over WebSocket:

| Direction | JSON-RPC type | Purpose |
|-----------|--------------|---------|
| Client → Server | Request (`id` + `method`) | RPC calls (open session, send message, ...) |
| Server → Client | Response (`id` + `result`/`error`) | Response to an RPC call |
| Server → Client | Notification (`method`, no `id`) | Real-time push (streaming, tool calls, ...) |

Transport: **WebSocket text frames**, each containing one JSON-RPC 2.0 object.

---

## Connection

### Endpoint

```
ws://localhost:18420/api/ws
```

### Authentication

Token passed as query parameter during WebSocket upgrade:

```
ws://localhost:18420/api/ws?token=<token>
```

The token is stored in `$OZZIE_PATH/.token`.

### Lifecycle

```
1. Connect    → WebSocket handshake to /api/ws?token=...
2. Open       → send "open_session" request
3. Interact   → send messages, receive notifications
4. Disconnect → close WebSocket (server cleans up)
```

---

## Frame Format (JSON-RPC 2.0)

### Request (Client → Server)

```json
{
  "jsonrpc": "2.0",
  "id": "req_1",
  "method": "send_message",
  "params": { "session_id": "sess_xyz", "text": "Hello" }
}
```

### Success Response (Server → Client)

```json
{
  "jsonrpc": "2.0",
  "id": "req_1",
  "result": { "accepted": true }
}
```

### Error Response (Server → Client)

```json
{
  "jsonrpc": "2.0",
  "id": "req_1",
  "error": { "code": -32000, "message": "session not found" }
}
```

### Notification (Server → Client)

Notifications have no `id` — they are server-pushed events.

```json
{
  "jsonrpc": "2.0",
  "method": "assistant.stream",
  "params": {
    "session_id": "sess_xyz",
    "phase": "delta",
    "content": "Hello!",
    "index": 1
  }
}
```

### Error Codes

| Code | Name | Description |
|------|------|-------------|
| -32700 | Parse error | Invalid JSON |
| -32600 | Invalid request | Not a valid JSON-RPC request |
| -32601 | Method not found | Unknown method name |
| -32602 | Invalid params | Invalid method parameters |
| -32603 | Internal error | Internal server error |
| -32000 | Server error | Business logic error (session not found, etc.) |

---

## Methods (Client → Server)

### `open_session`

Create a new session or resume an existing one.

**Params:**
```json
{
  "session_id": null,
  "working_dir": "/home/user/project",
  "language": "fr",
  "model": "gemini"
}
```

All fields optional. Empty/null `session_id` creates a new session.

**Result:**
```json
{
  "session_id": "sess_cosmic_asimov",
  "root_dir": "/home/user/project"
}
```

---

### `send_message`

Send a user message. Triggers the LLM inference loop.

**Params:**
```json
{
  "session_id": "sess_xyz",
  "text": "What files are in the current directory?"
}
```

**Result:**
```json
{ "accepted": true }
```

The response is immediate. LLM output arrives as `assistant.stream` and `assistant.message` notifications.

**Message buffering:** If a ReactLoop is already active for this session, the message is buffered and injected before the next LLM call. Multiple rapid messages are batched together. This is transparent to connectors.

---

### `load_messages`

Load conversation history.

**Params:**
```json
{
  "session_id": "sess_xyz",
  "limit": 20
}
```

**Result:**
```json
{
  "messages": [
    { "role": "user", "content": "Hello" },
    { "role": "assistant", "content": "Hi!" }
  ]
}
```

---

### `accept_all_tools`

Auto-approve all dangerous tool calls for this session.

**Params:**
```json
{ "session_id": "sess_xyz" }
```

**Result:**
```json
{ "accepted": true }
```

---

### `prompt_response`

Respond to a prompt request (tool approval, text input, etc.).

**Params:**
```json
{
  "token": "approval-execute-abc123",
  "value": "once",
  "text": null
}
```

| Field | Type | Description |
|-------|------|-------------|
| `token` | string | Token from the `prompt.request` notification |
| `value` | string? | Selected option value (`"once"`, `"session"`, `"deny"`) |
| `text` | string? | Free-form text (for `"text"` prompt type) |

**Result:**
```json
{ "accepted": true }
```

---

### `cancel_session`

Cancel the active ReactLoop for a session. Idempotent.

**Params:**
```json
{ "session_id": "sess_xyz" }
```

**Result:**
```json
{ "cancelled": true }
```

Triggers an `agent.cancelled` notification.

---

## Notifications (Server → Client)

Notifications are JSON-RPC 2.0 messages without an `id`. The `method` field contains the event type. All event data is in `params`, which always includes `session_id` when scoped to a session.

### Assistant

#### `assistant.stream`

Streaming LLM output.

```json
{
  "jsonrpc": "2.0",
  "method": "assistant.stream",
  "params": {
    "session_id": "sess_xyz",
    "phase": "delta",
    "content": "Hello!",
    "index": 1
  }
}
```

| Phase | Meaning |
|-------|---------|
| `start` | New stream begins. Clear/prepare output area. |
| `delta` | Text chunk. Append to current output. |
| `end` | Stream finished. Finalize output. |

#### `assistant.message`

Final complete message (sent after stream ends).

```json
{
  "jsonrpc": "2.0",
  "method": "assistant.message",
  "params": {
    "session_id": "sess_xyz",
    "content": "The full response text...",
    "error": null
  }
}
```

If `error` is non-null, the LLM call failed.

---

### Tools

#### `tool.call`

Tool invocation started.

```json
{
  "jsonrpc": "2.0",
  "method": "tool.call",
  "params": {
    "session_id": "sess_xyz",
    "call_id": "call_abc",
    "tool": "execute",
    "arguments": "{\"command\": \"ls\"}"
  }
}
```

#### `tool.result`

Tool execution completed.

```json
{
  "jsonrpc": "2.0",
  "method": "tool.result",
  "params": {
    "session_id": "sess_xyz",
    "call_id": "call_abc",
    "tool": "execute",
    "result": "file1.txt\nfile2.txt",
    "is_error": false
  }
}
```

#### `tool.progress`

Progress update from a long-running tool.

```json
{
  "jsonrpc": "2.0",
  "method": "tool.progress",
  "params": {
    "session_id": "sess_xyz",
    "call_id": "call_abc",
    "tool": "execute",
    "message": "Processing step 3/10..."
  }
}
```

---

### Prompts

#### `prompt.request`

Server needs user input. Connector **must** display UI and reply with `prompt_response`.

```json
{
  "jsonrpc": "2.0",
  "method": "prompt.request",
  "params": {
    "session_id": "sess_xyz",
    "prompt_type": "select",
    "label": "Tool \"execute\" requires approval. Arguments: {\"command\": \"ls\"}",
    "token": "approval-execute-abc123",
    "options": [
      { "value": "once", "label": "Allow once" },
      { "value": "session", "label": "Always allow for this session" },
      { "value": "deny", "label": "Deny" }
    ]
  }
}
```

---

### Flow Control

#### `agent.cancelled`

ReactLoop cancelled by user (via `cancel_session`).

```json
{
  "jsonrpc": "2.0",
  "method": "agent.cancelled",
  "params": { "session_id": "sess_xyz", "reason": "user_request" }
}
```

#### `agent.yielded`

LLM voluntarily stopped via `yield_control` tool.

```json
{
  "jsonrpc": "2.0",
  "method": "agent.yielded",
  "params": {
    "session_id": "sess_xyz",
    "reason": "done",
    "resume_on": null
  }
}
```

| Reason | Meaning |
|--------|---------|
| `done` | Task complete — agent goes idle |
| `waiting` | Blocked on external event (see `resume_on`) |
| `checkpoint` | Progress saved, yielding to pending work |

---

### Sessions

#### `session.created` / `session.closed`

```json
{ "jsonrpc": "2.0", "method": "session.created", "params": { "session_id": "sess_xyz" } }
{ "jsonrpc": "2.0", "method": "session.closed", "params": { "session_id": "sess_xyz" } }
```

---

### Internal

#### `internal.llm.call`

LLM call telemetry.

```json
{
  "jsonrpc": "2.0",
  "method": "internal.llm.call",
  "params": {
    "session_id": "sess_xyz",
    "phase": "response",
    "tokens_input": 1200,
    "tokens_output": 450
  }
}
```

---

## Flows

### Basic Conversation

```
Client                              Server
  │                                   │
  ├─{"method":"open_session"} ──────►│
  │◄──── {"result":{"session_id":…}} ┤
  │                                   │
  ├─{"method":"send_message"} ──────►│
  │◄──────── {"result":{"accepted"}} ┤
  │                                   │
  │◄── notification: assistant.stream │  (phase: start)
  │◄── notification: assistant.stream │  (phase: delta)
  │◄── notification: assistant.stream │  (phase: end)
  │◄── notification: assistant.message│
  │                                   │
```

### Tool Call with Approval

```
Client                              Server
  │                                   │
  ├─{"method":"send_message"} ──────►│
  │◄──────── {"result":{"accepted"}} ┤
  │                                   │
  │◄── notification: tool.call ───────┤  (execute, ls)
  │◄── notification: prompt.request ──┤  (approve?)
  │                                   │
  ├─{"method":"prompt_response"} ───►│  (value: "once")
  │◄──────── {"result":{"accepted"}} ┤
  │                                   │
  │◄── notification: tool.result ─────┤  (result: file list)
  │◄── notification: assistant.stream │
  │◄── notification: assistant.message│
  │                                   │
```

### Cancel Mid-Execution

```
Client                              Server
  │                                   │
  ├─{"method":"send_message"} ──────►│
  │   ... tool calls in progress ...  │
  │                                   │
  ├─{"method":"cancel_session"} ────►│
  │◄── {"result":{"cancelled":true}} ┤
  │◄── notification: agent.cancelled ─┤
  │                                   │
  ├─{"method":"send_message"} ──────►│  (session reusable)
  │◄──────── {"result":{"accepted"}} ┤
  │                                   │
```

---

## HTTP Endpoints

| Method | Path | Auth | Description |
|--------|------|------|-------------|
| `GET` | `/api/health` | No | Health check → `{"status":"ok"}` |
| `GET` | `/api/ws` | **Yes** | WebSocket upgrade endpoint |
| `GET` | `/api/events?limit=50&session=...&type=...` | **Yes** | Recent event history |
| `GET` | `/api/sessions` | **Yes** | List all sessions |
| `GET` | `/api/tasks?session_id=...` | **Yes** | List tasks |

---

## Implementing a Connector

### Minimum Viable Connector

1. **Connect** and call `open_session`
2. **Send** user input via `send_message`
3. **Render** `assistant.stream` notifications (append `delta` content)
4. **Handle** `prompt.request` notifications (reply with `prompt_response`)

### Notification Handling Matrix

| Notification | Tier 1 (Basic) | Tier 2 (Standard) | Tier 3 (Full) |
|-------------|:-:|:-:|:-:|
| `assistant.stream` | **Required** | **Required** | **Required** |
| `assistant.message` | **Required** | **Required** | **Required** |
| `prompt.request` | **Required** | **Required** | **Required** |
| `tool.call` / `tool.result` | — | **Required** | **Required** |
| `agent.cancelled` / `agent.yielded` | — | Recommended | **Required** |
| `task.*` | — | — | **Required** |
| `skill.*` | — | — | **Required** |
| `internal.llm.call` | — | — | Optional |

### Pseudocode Reference

```python
import websocket, json

ws = websocket.connect("ws://localhost:18420/api/ws?token=...")
req_id = 0

# 1. Open session
req_id += 1
ws.send(json.dumps({
    "jsonrpc": "2.0", "id": f"req_{req_id}",
    "method": "open_session", "params": {}
}))
res = json.loads(ws.recv())
session_id = res["result"]["session_id"]

# 2. Send message
req_id += 1
ws.send(json.dumps({
    "jsonrpc": "2.0", "id": f"req_{req_id}",
    "method": "send_message",
    "params": {"session_id": session_id, "text": user_input}
}))

# 3. Event loop
while True:
    frame = json.loads(ws.recv())

    if "result" in frame or "error" in frame:
        continue  # RPC response (ack)

    method = frame.get("method", "")
    params = frame.get("params", {})

    if method == "assistant.stream":
        if params["phase"] == "delta":
            print(params["content"], end="")
        elif params["phase"] == "end":
            print()

    elif method == "assistant.message":
        pass  # final message

    elif method == "prompt.request":
        answer = show_prompt(params["prompt_type"], params["label"], params["options"])
        req_id += 1
        ws.send(json.dumps({
            "jsonrpc": "2.0", "id": f"req_{req_id}",
            "method": "prompt_response",
            "params": {"token": params["token"], "value": answer}
        }))

    elif method == "tool.call":
        print(f"[tool: {params['tool']}]")
```
