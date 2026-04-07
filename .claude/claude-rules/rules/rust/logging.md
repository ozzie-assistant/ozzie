---
title: "Rust Logging (tracing)"
---

| Level | Usage |
|-------|-------|
| `error` | At handling boundary only — not at every `?` |
| `warn` | Degraded but recoverable |
| `info` | Program flow |
| `debug` | Decision paths, intermediate values |
| `trace` | Heavy diagnostic, scripts only |

Log at the **handling boundary** — one log per error, at the point where propagation stops. Never log at every `?`.

Never discard errors silently with `let _ = ...` — log before discarding.

**Never log secret values** — log the key name only.

## Structured Fields

```rust
info!(email = %email, "Creating user");  // % = Display, ? = Debug, no sigil = typed value
```

## Instrumentation

```rust
#[instrument(skip_all, fields(user_id = %cmd.user_id))]
async fn create_user(cmd: CreateUser, repository: &dyn UserRepository) -> Result<()> { }

#[instrument(skip_all, ret)]
async fn find_by_id(&self, id: UserId) -> Result<Option<User>> { }
```

- `skip_all` + explicit `fields(...)` — never auto-log params that may contain secrets
- Instrument entry points only — not hot-path inner functions
- Any crate performing I/O must declare `tracing` as a dependency
