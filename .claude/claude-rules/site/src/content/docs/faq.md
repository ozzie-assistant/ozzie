---
title: FAQ
description: Frequently asked questions about Claude Code.
---

## I accidentally used `/clear` — can I recover my conversation?

Yes. Claude Code stores a full audit of every session under `~/.claude/projects/`.

Each subdirectory corresponds to a project, named after its path on disk (e.g. `-Users-you-devs-myapp`). Inside, sessions are stored as `<UUID>.jsonl` files — one JSON line per turn.

**Steps to recover:**

1. Browse to the relevant project directory:
   ```bash
   ls ~/.claude/projects/
   ```

2. Identify the session file by reading the log (look for recognizable messages):
   ```bash
   cat ~/.claude/projects/<project-dir>/<UUID>.jsonl | head -50
   ```

3. Resume the session with:
   ```bash
   claude --resume <UUID>
   ```

> The conversation history will be restored exactly where you left off.

## What is the difference between a root `CLAUDE.md` and one in a subdirectory?

The root `CLAUDE.md` is always loaded. A `CLAUDE.md` placed in a subdirectory is loaded **on demand** — only when Claude Code is working within that directory.

This is useful to scope technology-specific rules (e.g. Leptos, React) to the crate or package they apply to, avoiding token cost when working elsewhere.

See [How to Use These Rules](../guidelines/how-to-use-rules/#rust--multi-crate-workspace-with-cqrs--leptos-portal) for a concrete example.

## What is the difference between a rule and a skill?

A **rule** sets the frame — it defines constraints, conventions, and context that Claude applies passively throughout a session (code style, architecture patterns, quality gates…).

A **skill** is a recipe — an invocable workflow triggered explicitly via a slash command (e.g. `/commit`, `/rust-add-domain`). It describes a sequence of steps Claude should follow to accomplish a specific task.

| | Rule | Skill |
|---|---|---|
| **Loaded** | Automatically, at session start | On demand, via `/skill-name` |
| **Purpose** | Shape Claude's behavior globally | Execute a specific workflow |
| **Example** | "Always use `thiserror` for errors" | "Add a new domain entity with its ports" |

## How do I import a rule from this repo into my project?

See [How to Use These Rules](../guidelines/how-to-use-rules/) — it covers three import strategies: git submodule (recommended), direct `@`-import, and copy-into-project.
