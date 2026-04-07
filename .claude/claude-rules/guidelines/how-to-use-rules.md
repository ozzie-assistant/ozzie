---
title: "How to Use These Rules"
---

## Import Strategies

### Option A — Git Submodule (recommended)

Add the library as a git submodule inside `.claude/`. The path becomes stable and relative
across all machines, and you get explicit version pinning per project.

```bash
git submodule add https://github.com/dohrm/claude-rules .claude/claude-rules
```

Then reference rules with a relative path:

```markdown
<!-- .claude/rules/code-style.md -->
@.claude/claude-rules/rules/rust/code-style.md
```

Update to latest when needed:

```bash
git submodule update --remote .claude/claude-rules
```

### Option B — Direct @-import

Reference an absolute path on the local machine. Simpler to set up, but path varies per machine.

```markdown
<!-- .claude/rules/code-style.md -->
@/path/to/claude-rules/rules/rust/code-style.md
```

### Option C — Copy into project

Copy relevant files into `.claude/rules/`. Use when you need project-specific overrides
or want no external dependency.

---

## Rust — Single Crate

Minimal setup for a standalone Rust service.

```
my-service/
├── CLAUDE.md
├── README.md
└── .claude/
    └── rules/
        ├── code-style.md       → @/path/to/claude-rules/rules/rust/code-style.md
        ├── error-handling.md   → @/path/to/claude-rules/rules/rust/error-handling.md
        ├── logging.md          → @/path/to/claude-rules/rules/rust/logging.md
        └── quality-gates.md   → @/path/to/claude-rules/rules/rust/quality-gates.md
```

`CLAUDE.md`:
```markdown
# My Service

@README.md

## Stack
- Rust 2024, tokio, axum
```

---

## Rust — Multi-Crate Workspace with Hexagonal Architecture

```
my-app/
├── CLAUDE.md
├── README.md
├── .claude/
│   └── rules/
│       ├── code-style.md       → @/path/to/claude-rules/rules/rust/code-style.md
│       ├── error-handling.md   → @/path/to/claude-rules/rules/rust/error-handling.md
│       ├── logging.md          → @/path/to/claude-rules/rules/rust/logging.md
│       ├── quality-gates.md   → @/path/to/claude-rules/rules/rust/quality-gates.md
│       └── hexagonal.md        → @/path/to/claude-rules/rules/rust/hexagonal.md
└── crates/
    ├── domain/
    ├── infrastructure/
    └── api/
```

`CLAUDE.md`:
```markdown
# My App

@README.md

## Stack
- Rust 2024 edition, tokio, axum, MongoDB
- Hexagonal architecture — see .claude/rules/hexagonal.md
```

---

## Rust — Multi-Crate Workspace with CQRS + Leptos Portal

The most common full-stack setup. Generic Rust rules at workspace root, Leptos-specific rules
scoped to the portal crate via subdirectory CLAUDE.md (loaded on demand).

```
my-app/
├── CLAUDE.md
├── README.md
├── .claude/
│   └── rules/
│       ├── code-style.md       → @/path/to/claude-rules/rules/rust/code-style.md
│       ├── error-handling.md   → @/path/to/claude-rules/rules/rust/error-handling.md
│       ├── logging.md          → @/path/to/claude-rules/rules/rust/logging.md
│       ├── quality-gates.md   → @/path/to/claude-rules/rules/rust/quality-gates.md
│       ├── hexagonal.md        → @/path/to/claude-rules/rules/rust/hexagonal.md
│       └── cqrs.md             → @/path/to/claude-rules/rules/rust/cqrs.md
└── crates/
    ├── domain/
    ├── infrastructure/
    ├── api/
    └── portal/                      # Leptos SSR frontend crate
        └── CLAUDE.md                # loaded on demand — only when working in portal/
```

`crates/portal/CLAUDE.md`:
```markdown
# Portal — Leptos SSR frontend

This crate is the user-facing UI. It uses Leptos 0.8 with SSR + WASM hydration.
Server functions are SSR-only — do not introduce WASM-incompatible dependencies.

@/path/to/claude-rules/rules/leptos/patterns.md
@/path/to/claude-rules/rules/leptos/gotchas.md
```

> **Why subdirectory CLAUDE.md for portal?**
> Leptos rules are only relevant when working inside `portal/`. Using a subdirectory CLAUDE.md
> ensures they load on demand — no token cost when working elsewhere in the workspace.
> See [loading mechanics](./claude-md-hierarchy.md).

---

## Go — Hexagonal Service

```
my-go-service/
├── CLAUDE.md
├── README.md
└── .claude/
    └── rules/
        ├── quality-gates.md        → @/path/to/claude-rules/rules/go/quality-gates.md
        └── hexagonal-packaging.md  → @/path/to/claude-rules/rules/go/hexagonal-packaging.md
```

`CLAUDE.md`:
```markdown
# My Go Service

@README.md

## Stack
- Go 1.23+, chi, MongoDB
- Hexagonal packaging — see .claude/rules/hexagonal-packaging.md
```

---

## Language Rule

Always add the language rule — either project-specific (if you want to enforce a specific
communication language) or from the library (generic):

```markdown
<!-- .claude/rules/language.md -->
@/path/to/claude-rules/rules/language.md
```

Or override directly for the project:

```markdown
<!-- .claude/rules/language.md -->
# Language
- All written artifacts must be in **English**.
- Communicate with the user in **French**.
```

---

## Rule Precedence

When a project rule conflicts with an imported library rule, the project rule wins.
Place project-specific overrides **after** the `@`-import in the same file:

```markdown
<!-- .claude/rules/code-style.md -->
@/path/to/claude-rules/rules/rust/code-style.md

## Project Overrides
- Function size hard limit: 80 lines (stricter than library default)
```
