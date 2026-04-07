---
title: "Hexagonal Architecture"
---

## Core Principle

Dependencies flow **inward only**. The domain (core) layer is pure and has zero infrastructure dependencies.

```
[Web / CLI] → [Infrastructure / Adapters] → [Domain / Core]
                                                    ↑
                                            NEVER depends on
                                            anything above
```

## Layer Responsibilities

### Domain / Core

- Defines **business objects** and value types (entities, domain models, enums…)
- Defines capability **ports** (interfaces/traits) — no implementations
- In a CQRS context: aggregates, commands, events play the role of business objects
- Query parameter structures as plain data — no database mapping
- **Allowed**: error definition libraries, value types (uuid, chrono), HTTP data model libraries used as pure DTOs (e.g. `StatusCode`, `Uri` as value objects — not HTTP clients)
- **Serialization (pragmatic position)**: serialization *traits / annotations* with no format dependency are acceptable in core (e.g. marker annotations, derive macros that carry no format knowledge). Format-specific serialization (`json`, `bson`, `xml`…) belongs in the infrastructure layer. This is a language-level trade-off — some ecosystems make it harder to avoid than others.
- **Forbidden**: database drivers, HTTP clients, web frameworks, auth implementations, any infrastructure deps

### Infrastructure / Adapters

- Implements ports defined by core
- All database mapping, HTTP clients, auth implementations live here
- Depends on core, never the reverse

### Web / CLI (Entry Points)

- Dependency injection wiring
- Route mounting, middleware, configuration
- Drives execution (Hollywood Principle: adapters conform to interfaces, they do not drive control flow)

## Critical Rules

- Flag any import in core that references an infra or adapter package
- Flag any infrastructure type leaking into a domain port signature
- Query-to-database mapping belongs in the infrastructure layer, never in domain query structs
- Port signatures must use typed errors — no opaque error boxes in core interfaces

## Port / Adapter Pattern

```
// core — defines the contract (interface/trait)
interface UserRepository {
    findById(id: UserId): Result<User?>
}

// infrastructure — implements the contract
class MongoUserRepository implements UserRepository {
    findById(id: UserId): Result<User?> {
        // database-specific code here
    }
}
```

## No Leaky Abstractions

Infrastructure types must not appear in domain types:

```
// BAD — DB type in domain
struct User {
    id: mongo::ObjectId   // leaking infra
}

// GOOD — pure domain type
struct UserId(Uuid)
struct User {
    id: UserId
}
```

## Review Checklist

- [ ] Core has no infra dependencies (no DB drivers, no HTTP clients, no web frameworks)
- [ ] No direct HTTP/DB calls in domain logic
- [ ] Port signatures use typed errors
- [ ] Query structures in core are plain data (no DB document conversion)
- [ ] HTTP types in core, if any, are used as pure value objects — not to make HTTP calls
