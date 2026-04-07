---
title: "CQRS / Event Sourcing"
---

## Write Flow

```
Command → Command Handler → Aggregate validates → Events emitted → Events persisted
```

## Read Flow

```
Events → Event Handlers → Projections / Read Models → Query → Response
```

Read models are derived **exclusively from events**. They are never written to directly.

## Core Concepts

- **Command**: intent to change state — rejected if invalid, no event emitted
- **Aggregate**: consistency boundary — validates commands, emits events, applies state
- **Event**: immutable fact about what happened — the source of truth
- **Event Store**: append-only log of events — never modified or deleted
- **Projection / Read Model**: a view derived from replaying events — optimized for query

## Core Rules

- **Events are immutable** — never modify or delete persisted events
- **State changes go through commands** — no direct mutation of aggregate state
- **Read models derived from events only** — via event handlers, never written directly
- **Commands can be rejected** — a rejected command produces no event and no state change
- **Port signatures use typed errors** — no opaque error boxes in domain interfaces

## Domain Layer

- Aggregate: validates commands, emits events, holds current state (rebuilt by applying events)
- Commands: express intent — one command type per operation
- Events: facts — named in past tense (`UserCreated`, `OrderShipped`)
- Query structs: plain data describing read-side filter parameters — no DB knowledge

## Infrastructure Layer

- Event store: append-only persistence of events
- Snapshot store *(optional optimization)*: cached aggregate state to avoid full event replay — not a primary read model
- Projections: listen to events, maintain read-optimized views
- Query translation: maps domain query structs to DB-specific queries

## Denormalized Views

Additional read models derived from events for specific query shapes the aggregate state does not serve well (e.g. per-event history, cross-aggregate summaries).

```
Event persisted → Event handler triggered → View updated
```

### View Rules

- A view defines how to fold a single event into its state — irrelevant events are ignored
- Views are read-only from the domain perspective — only event handlers write to them
- One view record per event (history/child) or one per aggregate (summary) — depends on the shape
- View storage lives in the infrastructure layer

## Checklist

- [ ] State changes go through commands → aggregate → events
- [ ] Events persisted before any read model is updated
- [ ] Read models updated via event handlers only — never written directly
- [ ] Snapshots, if present, are an optimization — not the primary read model
- [ ] Query structs have no DB mapping logic
- [ ] Rejected commands produce no events
