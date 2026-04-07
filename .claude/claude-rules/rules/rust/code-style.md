---
title: "Rust Code Style"
---

## Control Flow

- Exhaustive match on enums — no wildcard `_` catch-all for meaningful variants
- Use `matches!(val, Pattern)` for boolean pattern checks instead of `match` returning `true`/`false`
- Use `if let` / `while let` when only one pattern is relevant

## Ownership & Borrowing

- Prefer `Option<&T>` over `&Option<T>` in function signatures
- Prefer `impl Trait` over `Box<dyn Trait>` for return types when the concrete type is known at compile time
- Use `Cow<str>` / `Cow<[T]>` for functions that sometimes own, sometimes borrow
- Prefer iterator chains (`map`, `filter`, `flat_map`, `fold`) over imperative `for` loops

## Function Size

- **Target: ≤ 50 lines.** Hard limit: 100 lines — extract named helpers unconditionally beyond this.
- One function, one responsibility — if a comment separates sections, it's two functions.

## Serde

Domain types: field names as-is. API-facing types:

```rust
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
pub struct CreateUserRequest { /* ... */ }
```

- Tagged enums: `#[serde(tag = "type")]` or `#[serde(tag = "type", content = "data")]`
- Optional fields: `#[serde(skip_serializing_if = "Option::is_none")]`
- New optional fields: `#[serde(default)]` for backward compatibility
- Prefer `rename_all` at struct level over per-field `#[serde(rename)]`

## Tokio

Never block the runtime — use `tokio::time::sleep`, `tokio::fs::*`. Use `tokio::task::spawn_blocking` for sync/CPU-bound work.

Never hold a `std::sync::Mutex` guard across `.await` — drop before awaiting or use `tokio::sync::Mutex`.

```rust
// Structured concurrency
let mut set = JoinSet::new();
for item in items { set.spawn(process(item)); }
while let Some(res) = set.join_next().await { res??; }
```

Channels: `mpsc` (bounded preferred), `broadcast` (fan-out), `watch` (latest value), `oneshot` (request/reply).

All external I/O must have explicit timeouts:

```rust
tokio::time::timeout(Duration::from_secs(5), external_call()).await??;
```
