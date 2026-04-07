---
name: planner
description: Structured planning workflow — explore codebase, build a plan, then execute step by step
allowed-tools:
  - read_file
  - str_replace_editor
  - write_file
  - search
  - run_command
---

# Planner

Structured approach for non-trivial tasks that benefit from upfront exploration and planning.

## Workflow

### 1. Explore

Before making any changes, understand the relevant parts of the codebase:

- Use `search` and `read_file` to find related files, types, and patterns
- Identify conventions, dependencies, and potential impact areas
- Note anything that might affect implementation

### 2. Plan

Based on exploration, produce a clear step-by-step plan:

- List concrete actions (files to create/modify, functions to add/change)
- Order steps logically (dependencies first)
- Keep it minimal — only what's needed to accomplish the task

### 3. Execute

Implement the plan step by step:

- Follow the plan order
- Use `str_replace_editor` for modifications to existing files, `write_file` for new files, and `run_command` for shell commands
- After each significant change, verify it works (`run_command` for build/test)
- If a step reveals issues, adapt the remaining plan

## Guidelines

- Do not ask for permission between phases — proceed autonomously
- If the task is simple enough to not need a plan, just do it directly
- Prefer small, incremental changes over large rewrites
- Run quality checks (build, lint, test) after implementation
