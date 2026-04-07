---
title: "Rust Error Handling"
---

| Context | Library |
|---------|---------|
| Domain / library crates | `thiserror` — typed, matchable errors |
| Infrastructure / adapter crates | `thiserror` or `anyhow` with context |
| CLI entry points / `main.rs` | `anyhow` or `miette` |

- Never `Box<dyn Error>` in domain crates
- Never `anyhow` in port (trait) signatures

## thiserror

```rust
#[derive(Debug, thiserror::Error)]
pub enum UserError {
    #[error("User not found: {0}")]
    NotFound(UserId),
    #[error("Repository failure")]
    Repository(#[from] RepositoryError),  // #[from] → impl From<T>, enables ?
}
```

- `#[error(transparent)]` — delegates `Display` and `source()` to inner error (thin wrappers only)
- One error enum per module boundary — variants named after *what went wrong*, not *where*
- No catch-all `Other(String)` variants — they destroy matchability

## anyhow

```rust
repository.find(id).await.context("Failed to fetch user")?;
heavy_op().with_context(|| format!("Processing item {id}"))?;
bail!("Item list must not be empty");
ensure!(items.len() <= MAX, "Too many items: {} > {MAX}", items.len());
```

## miette (CLI only)

```rust
#[derive(Debug, miette::Diagnostic, thiserror::Error)]
#[error("Parse failed")]
#[diagnostic(code(myapp::parse), help("Check the input format"))]
pub struct ParseError {
    #[source_code] src: NamedSource<String>,
    #[label("here")] span: SourceSpan,
}
```

## unwrap / expect

- No `.unwrap()` in production code
- `.expect("reason")` only for invariants that cannot be violated — message must explain *why*
- Both allowed freely in `#[cfg(test)]`

## Error Propagation

Propagate with `?`. Never discard silently with `let _ = ...` — log before discarding.
