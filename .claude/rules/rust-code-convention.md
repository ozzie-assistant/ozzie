# Rust Code Conventions

@.claude/claude-rules/rules/rust/code-style.md

## Module structure: folder-based modules

When a module contains **multiple logical units** (e.g. several tool implementations, several store backends), use a **folder-based module** instead of a single monolithic file.

```
# Instead of:
native/file.rs          # 5 tools mixed together

# Prefer:
native/file/
  mod.rs                # re-exports + shared constants
  read.rs               # FileReadTool
  write.rs              # FileWriteTool
  list_dir.rs           # ListDirTool
  glob.rs               # GlobTool
  grep.rs               # GrepTool
```

### `mod.rs` responsibilities

`mod.rs` contains **only**:
- `mod` declarations
- `pub use` re-exports
- Shared constants or types used across submodules

No business logic. No tests.

### Naming

Each file is named after the **concept** it implements:
- `read.rs` not `file_read_tool.rs`
- `list_dir.rs` not `list_dir_tool.rs`

### Tests live with the code they test

Unit tests go in a `#[cfg(test)] mod tests` block at the bottom of the file they test — not in the parent `mod.rs`.

```rust
// grep.rs
pub struct GrepTool;
// ...impl...

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn grep_finds_pattern() { /* ... */ }
}
```

This way `cargo test` output shows the full path (`native::file::grep::tests::grep_finds_pattern`), tests have access to private types via `use super::*`, and deleting a module deletes its tests with it.

## Tagged enums over magic strings

When a field acts as a discriminator with distinct payloads per value, use a **`#[serde(tag = "...")]` enum** instead of a string field + conditional optional fields.

```rust
// Avoid: flat struct with magic string matching
#[derive(Deserialize)]
struct Input {
    command: String,            // "create" | "view" | ...
    file_text: Option<String>,  // only for "create"
    old_str: Option<String>,    // only for "str_replace"
    // caller has to guess which fields go with which command
}

// Prefer: tagged enum — each variant carries exactly its fields
#[derive(Deserialize)]
#[serde(tag = "command", rename_all = "snake_case")]
enum Input {
    Create { path: String, file_text: Option<String> },
    StrReplace { path: String, old_str: String, new_str: Option<String> },
    View { path: String, view_range: Option<Vec<usize>> },
}
```

Benefits:
- **No runtime validation**: serde rejects missing required fields at deserialization
- **Exhaustive match**: adding a variant forces handling it at compile time
- **Self-documenting**: each variant shows exactly what it expects

When a field is shared across all variants (like `path` above), **duplicate it** rather than using `#[serde(flatten)]` — flatten produces more complex schemas and harder-to-debug deserialization errors.
