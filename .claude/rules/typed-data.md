# Typed Data — No Raw JSON, No Magic Strings

Raw JSON manipulation (`serde_json::Value`, `.get("field")`, `json!({...})`) is **only allowed in infrastructure boundaries**:

- Transport/protocol layers (Frame serialization, HTTP response parsing)
- External API adapters (LLM provider responses, MCP wire format)
- Config loading (JSONC parsing)

## Everywhere else: use typed structs

All data crossing module boundaries MUST use `Serialize`/`Deserialize` structs:

```rust
// BAD — fragile, no compile-time safety, magic strings
let token = params.get("token").and_then(|v| v.as_str()).unwrap_or("");
let value = params.get("value").and_then(|v| v.as_str());

// GOOD — schema is explicit, compiler catches mismatches
let params: PromptResponseParams = serde_json::from_value(raw)?;
// params.token, params.value — typed, documented, refactorable
```

## Event types: use `EventKind` enum, not string literals

When matching on event types, use the `EventKind` enum from `ozzie-protocol`:

```rust
// BAD — typo = silent bug, no exhaustiveness check
match frame.event.as_deref().unwrap_or("") {
    "assistant.stream" => { ... }
    "assitant.message" => { ... } // typo, never matches
}

// GOOD — compiler catches typos and missing arms
match frame.event_kind() {
    Some(EventKind::AssistantStream) => { ... }
    Some(EventKind::AssistantMessage) => { ... }
    _ => { ... }
}
```

String literals for event types are only acceptable in:
- `EventPayload` serde rename tags (the source of truth in `ozzie-core`)
- `EventKind::as_str()` / `EventKind::parse()` (the bidirectional mapping in `ozzie-protocol`)
- Event bus `subscribe()` filters (infra layer, accepts `&[&str]`)

## Why

- **Resistance to change**: renaming a field or event is a compile error, not a silent runtime bug
- **Maintainability**: struct/enum definition IS the documentation
- **No magic strings**: identifiers exist once in the type, not scattered across match arms
- **Clear interfaces**: function signatures tell you exactly what data flows through
- **Exhaustiveness**: `match` on enums warns about unhandled variants

## Shared protocol types

Types exchanged over WebSocket live in `ozzie-protocol` (no domain dependency).
Domain types live in `ozzie-core`. Protocol types may mirror domain types for client-side use
without creating a dependency on `ozzie-core`.

- `EventKind` — typed event identifiers (mirrors `EventPayload` variant names)
- `PromptRequestPayload` / `PromptResponseParams` — typed prompt data
- `PromptOption` — prompt choice
