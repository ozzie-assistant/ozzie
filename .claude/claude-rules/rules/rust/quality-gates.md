---
title: "Rust Quality Gates"
---

Before considering any change complete, all of the following must pass:

```bash
cargo build               # Successful compilation
cargo test                # All tests pass
cargo clippy -- -D warnings  # Zero warnings
cargo fmt --check         # Formatting compliant
```

Do not skip or ignore any of these checks.

## Clippy Lints (library crates)

Deny `unwrap` and `expect` in production code, but allow them in tests:

```rust
// At crate root (lib.rs)
#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]
```

Using `#![deny(...)]` without `cfg_attr` will also reject `.unwrap()` in `#[cfg(test)]` modules,
causing test compilation failures.

## Testing Conventions

- Unit tests: `#[cfg(test)]` module in the same file as the code under test
- Integration tests: `tests/` directory at crate root
- Prefer trait-based fakes over `mockall` unless complexity justifies it
