---
title: "CLAUDE.md Hierarchy"
---

## Core Principle

Keep `CLAUDE.md` light (< 200 lines). Adherence to rules decreases as file size grows.
Split concerns across files and load them via the `@path/to/file.md` import syntax.

## File Roles

| File | Audience | Content |
|------|----------|---------|
| `README.md` | Humans + Claude (via `@README.md`) | Project vision, goals, product direction |
| `CLAUDE.md` | Claude only | Technical context, stack, workflows, `@` references to rules |
| `.claude/rules/*.md` | Claude only (auto-discovered) | Specific rules: style, quality gates, language, security, … |

## Loading Mechanics

### Eager (every session)

Loaded at launch, token cost paid on every conversation:

- Ancestor `CLAUDE.md` files (root and above the working directory)
- `.claude/rules/*.md` files **without** `paths` frontmatter
- All files `@`-imported by any of the above (up to 5 levels deep)

### On-demand

Loaded only when relevant, token cost paid only when needed:

- **Subdirectory `CLAUDE.md`** — loaded when Claude reads a file in that directory
- **Path-scoped rules** — `.claude/rules/*.md` files with `paths` frontmatter, loaded when Claude opens a matching file

```markdown
---
paths:
  - "src/**/*.rs"
---
# This rule only loads when Claude opens a .rs file
```

> **Implication:** every rule in `.claude/rules/` without a `paths` filter is always in context.
> Use path-scoped rules or subdirectory `CLAUDE.md` files to limit scope in large projects.

## Hierarchical Loading in Multi-Module Projects

**Recommended structure:**

```
project/
├── CLAUDE.md              # Global: stack, shared rules (@rules/style.md, @README.md, …)
├── README.md              # Product vision
├── .claude/
│   └── rules/
│       ├── code-style.md           # always loaded
│       ├── quality-gates.md        # always loaded
│       └── leptos.md               # path-scoped: only when *.rs files opened
└── workspace-rust/
    └── CLAUDE.md          # on-demand: loaded when Claude works in this directory
```

## Subdirectory CLAUDE.md

Carries both vision and technical rules for the submodule — audience is Claude, not a human onboarding. Keep the vision short (2-3 lines): enough for Claude to understand the *why* behind local constraints.

Example (`workspace-rust/CLAUDE.md`):
```markdown
# Rust Workspace

This module handles real-time audio processing. Latency is a hard constraint — avoid allocations on the hot path.

@../rules/code-style.md
```

## Summary

| Mechanism | When loaded | Token cost |
|-----------|-------------|------------|
| Root / ancestor `CLAUDE.md` | Every session | Always |
| `.claude/rules/*.md` (no paths filter) | Every session | Always |
| `@`-imported files | Every session | Always |
| `.claude/rules/*.md` (with `paths`) | On matching file open | On demand |
| Subdirectory `CLAUDE.md` | On file access in that dir | On demand |

## Other

- `@path/to/file.md` imports are recursive up to 5 levels deep
- More specific (deeper) files override parent instructions on conflict
- In large monorepos, use `claudeMdExcludes` in `.claude/settings.local.json` to skip irrelevant CLAUDE.md files

## Sources

- [Memory & CLAUDE.md — Claude Code Docs](https://code.claude.com/docs/en/memory)
- [Settings — Claude Code Docs](https://docs.anthropic.com/en/docs/claude-code/settings)
