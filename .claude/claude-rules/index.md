---
title: Claude Rules
description: A personal library of reusable Claude Code assets — rules, skills, and guidelines.
template: splash
hero:
  tagline: Reusable Claude Code assets for consistent AI-assisted development.
  actions:
    - text: Guidelines
      link: guidelines/prompting/
      icon: open-book
    - text: GitHub
      link: https://github.com/dohrm/claude-rules
      icon: github
      variant: minimal
---

## What's in here

A curated set of rules, guidelines, and skills for [Claude Code](https://claude.ai/code) — built across projects and centralized here as a shared foundation.

| Section | Content |
|---------|---------|
| **Rules** | Importable via `@` in any `CLAUDE.md` — style, architecture, quality gates per technology |
| **Guidelines** | Patterns for working effectively with Claude Code |
| **Skills** | Custom slash commands for recurring tasks |

## Usage

```bash
git submodule add https://github.com/dohrm/claude-rules .claude/claude-rules
```

Then reference rules from your project's `.claude/rules/` files:

```markdown
@.claude/claude-rules/rules/rust/quality-gates.md
@.claude/claude-rules/rules/architecture/hexagonal.md
```
