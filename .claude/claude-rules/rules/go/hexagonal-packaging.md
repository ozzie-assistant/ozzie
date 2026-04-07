---
title: "Go Hexagonal Packaging"
---

## Layer Layout

```
internal/config/   → shared config DTOs (neither core nor infra)
internal/core/     → pure domain (zero infra dependency)
internal/infra/    → adapters & infrastructure implementations
pkg/               → importable libraries (no internal/ dependency)
cmd/               → assembly, CLI entry points
```

## Dependency Rule

```
cmd/ → internal/infra/ → internal/core/
                               ↑
                       NEVER depends on
                       anything above

pkg/ → never imports internal/
```

## `internal/core/` — Pure Domain

- Defines domain ports (interfaces/traits) and types
- Zero dependency on infrastructure packages (no DB drivers, no HTTP frameworks, no external SDKs)
- Can be tested without any infrastructure

## `internal/infra/` — Adapters

- Implements ports defined by `core/`
- All database, HTTP, external SDK code lives here
- May import `core/`, never the reverse

## `internal/config/`

Stays neutral — imported by both `core/` and `infra/`. Contains DTOs only: neither domain logic nor infrastructure code.

## `pkg/` — Importable Libraries

- Reusable libraries with **no dependency** on internal application wiring (`config/`, `infra/`, sessions, events…)
- If a package depends on internal wiring, it belongs in `internal/`, not `pkg/`

## Package Hygiene

- No orphan packages: a package with 1-2 files and a single consumer should be merged into its consumer
- Use sub-packages for large families, but avoid fragmentation — no sub-package for fewer than 3 files
- User-facing clients (TUI, WS client) live in `clients/` — internal interface implementations live in `internal/infra/`
