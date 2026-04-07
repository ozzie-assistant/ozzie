# claude-rules

A personal library of reusable Claude Code assets — rules, skills, and guidelines — built up across projects and meant to be shared.

**Site:** https://dohrm.github.io/claude-rules/

## Goals

- Centralize rules written across multiple projects into a single source of truth
- Provide a reusable foundation: import rules directly into any project via `@`
- Serve as a reference and onboarding base for team members working with Claude Code

## Structure

```
claude-rules/
├── rules/                    # Reusable rule files (@-importable in any CLAUDE.md)
│   ├── language.md           # Language split: artifacts in English, communication in preferred language
│   ├── architecture/
│   │   ├── hexagonal.md             # Hexagonal architecture: dependency direction, domain purity, trait pattern
│   │   ├── cqrs.md                  # CQRS / Event Sourcing: flow, aggregate pattern, query/QueryBuilder split
│   │   └── frontend-flat-domain.md  # Frontend modular portal: ui/features/core/pages, dependency rules
│   ├── rust/
│   │   ├── code-style.md     # Naming, control flow, ownership, async, serde
│   │   ├── error-handling.md # thiserror/anyhow usage, unwrap rules, propagation
│   │   ├── logging.md        # tracing levels, secrets, structured fields, #[instrument]
│   │   └── quality-gates.md  # cargo build/test/clippy/fmt
│   ├── go/
│   │   ├── quality-gates.md          # golangci-lint / go test -race / govulncheck
│   │   └── hexagonal-packaging.md    # core/infra/pkg layout, dependency rule
│   ├── react/
│   │   └── portal.md         # OpenAPI gen, TanStack Query, portal context (user/locale/theme)
│   └── leptos/
│       ├── patterns.md       # Resource/Suspense, StoredValue, spawn_local, server functions
│       ├── gotchas.md        # Children/ChildrenFn, For syntax, compilation quirks, WASM safety
│       └── portal.md         # SSR-first, shared crate types, cookie-based app state (exploratory)
├── skills/                   # Custom skill definitions for Claude Code
│   └── rust-add-domain.md    # Add a new domain module to the Rust DI container
└── guidelines/               # Patterns and recommendations for working with Claude Code
    ├── claude-md-hierarchy.md  # CLAUDE.md file roles and loading mechanics
    ├── how-to-use-rules.md     # Import strategies per technology
    ├── prompting.md            # Prompt structure, syntax, and iteration tips
    └── tooling.md              # Tech radar: MCPs, plugins, hooks (Adopt/Trial/Assess/Hold)
```

## Usage

The recommended approach is to add this repository as a git submodule:

```bash
git submodule add https://github.com/dohrm/claude-rules .claude/claude-rules
```

Then reference rules from your project's `.claude/rules/` files:

```markdown
@.claude/claude-rules/rules/language.md
@.claude/claude-rules/rules/rust/quality-gates.md
@.claude/claude-rules/rules/architecture/hexagonal.md
```

See [`guidelines/how-to-use-rules.md`](./guidelines/how-to-use-rules.md) for per-technology setup examples.

## Guidelines

See [`guidelines/`](./guidelines/) for documented patterns on topics such as:

- [How to use these rules in a project](./guidelines/how-to-use-rules.md)
- [CLAUDE.md hierarchy in multi-module projects](./guidelines/claude-md-hierarchy.md)
- [Prompting Claude — practical guide](./guidelines/prompting.md)
- [Tooling — Tech Radar](./guidelines/tooling.md)
