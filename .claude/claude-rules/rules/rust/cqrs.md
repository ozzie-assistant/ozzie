---
title: "CQRS / Event Sourcing — Rust"
---

## Library

[`cqrs-rust-lib`](https://github.com/dohrm/cqrs-rust-lib)

## Opinionated Deviations from Pure CQRS-ES

| Pure CQRS-ES | This implementation |
|---|---|
| Snapshot = optional optimization | Snapshot = primary read model, written on every command |
| One command type per operation | `CreateCommand` + `UpdateCommand` per aggregate |
| Views updated after event persistence | `ViewDispatcher` fires after (event + snapshot) pair |
| No prescribed query layering | Query in domain + `QueryBuilder` in infrastructure |

## Aggregate Pattern

```rust
impl Aggregate for User {
    const TYPE: &'static str = "users";
    type CreateCommand = CreateUserCommands;
    type UpdateCommand = UpdateUserCommands;
    type Event = UserEvents;
    type Services = Arc<dyn UserServices + Send + Sync>;
    type Error = UserError;

    async fn handle_create(cmd: Self::CreateCommand, svc: &Self::Services) -> Result<Vec<Self::Event>, Self::Error>;
    async fn handle_update(&self, cmd: Self::UpdateCommand, svc: &Self::Services) -> Result<Vec<Self::Event>, Self::Error>;
    fn apply(&mut self, event: Self::Event) -> Result<(), Self::Error>;
}
```

## Query / QueryBuilder Split

`UserQuery` — plain data, no DB knowledge — lives in domain.
`UserQueryBuilder` — translates `UserQuery` to a DB `Document` — lives in infrastructure.

## CqrsContext

Carries user identity for audit trails. Extracted in HTTP middleware, threaded through all commands.

## ViewDispatcher

`ViewDispatcher<A, V, Q>` fires after (event + snapshot) pair is persisted.

```rust
impl View<Account> for Movement {
    const TYPE: &'static str = "movement";
    const IS_CHILD_OF_AGGREGATE: bool = true; // true = one record/event; false = one record/aggregate

    fn view_id(event: &EventEnvelope<Account>) -> String;
    fn update(&self, event: &EventEnvelope<Account>) -> Option<Self>; // None = ignore this event
}
```

`update()` must be pure — no I/O, no side-effects.

## Rules

- `Aggregate` + `View` structs in domain crate — no infra imports
- `QueryBuilder` + `ViewDispatcher` wiring in infrastructure crate
- Events persisted via engine — no direct snapshot mutation
- `CqrsContext` propagated from entry point, never created ad-hoc in domain
