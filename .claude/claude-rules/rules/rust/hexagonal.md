---
title: "Hexagonal Architecture — Rust"
---

## Rust-Specific Rules

### Allowed dependencies in core

- `serde` — serialization traits and derive macros only (format-agnostic)
- `thiserror` — typed error definitions
- `async-trait` — async port definitions
- `uuid`, `chrono` — value objects
- `http` — HTTP types as pure value objects (`StatusCode`, `Uri`, `HeaderMap`…) — **not** as a client

### Forbidden in core

- `serde_json`, `serde_bson`, `quick-xml` — format-specific serialization (belongs in infra)
- `mongodb`, `sqlx`, `diesel` — database drivers
- `axum`, `actix`, `rocket` — web frameworks
- `reqwest`, `hyper` — HTTP clients
- `jsonwebtoken`, `openidconnect` — auth implementations
- `anyhow` — not allowed in port (trait) signatures

### Port Definition Pattern

```rust
// core — defines the contract
#[async_trait]
pub trait UserRepository: Send + Sync {
    async fn find_by_id(&self, id: UserId) -> Result<Option<User>, UserError>;
}
```

### Adapter Implementation Pattern

```rust
// infrastructure — implements the contract
pub struct MongoUserRepository { /* ... */ }

impl UserRepository for MongoUserRepository {
    async fn find_by_id(&self, id: UserId) -> Result<Option<User>, UserError> {
        // MongoDB-specific code here
    }
}
```

### No Leaky Abstractions

```rust
// BAD — infra type in domain
pub struct User {
    pub id: mongodb::bson::oid::ObjectId,
}

// GOOD — pure domain type
pub struct UserId(pub Uuid);
pub struct User {
    pub id: UserId,
}
```

### Cargo.toml Checklist

- [ ] Core crate has no infra dependencies
- [ ] No `use mongodb::` / `use axum::` / `use sqlx::` / `use reqwest::` in core
- [ ] Port signatures use typed errors (`thiserror`), not `anyhow` or `Box<dyn Error>`
