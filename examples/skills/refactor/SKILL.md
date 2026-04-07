---
name: refactor
description: Identify code smells, plan a safe refactoring strategy, apply transformations, and verify nothing breaks
allowed-tools:
  - read_file
  - str_replace_editor
  - write_file
  - search
  - run_command
---

# Refactor

Safely refactor code by identifying improvement opportunities, applying incremental transformations, and verifying no
regressions. Each change should be minimal, reversible, and behavior-preserving.
