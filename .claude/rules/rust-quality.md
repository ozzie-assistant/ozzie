---
description: Rust code quality gates — applies to all Rust file changes
globs: "**/*.rs"
---

# Rust Quality Gates

@.claude/claude-rules/rules/rust/quality-gates.md

## Ozzie overrides

All gates run with `--workspace` — every crate must pass:

```bash
cargo check --workspace    # compile
cargo clippy --workspace   # lint (zero warnings)
cargo test --workspace     # tests
```

A clippy warning is a build failure. No `#[allow(clippy::...)]` without explicit justification.
