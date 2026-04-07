---
title: "Claude Code Tooling — Tech Radar"
---

| Quadrant | Meaning |
|----------|---------|
| **Adopt** | In production, predictable behavior |
| **Trial** | Actively tested, value likely |
| **Assess** | To explore, not enough hindsight |
| **Hold** | Identified issues — do not generalize |

---

## Adopt

### MCP — context7

Fetches up-to-date documentation for libraries directly from source.

- Claude defaults to context7 over web search for library research
- Avoids stale training data on fast-moving libraries (Leptos, Tokio, etc.)
- Active by default once installed — for versioned APIs, prompt explicitly:
  ```
  Use context7 to verify the current Leptos 0.8 API before generating code.
  ```

---

## Trial

### Plugin — LSP

Provides language server integration — type information, diagnostics, and cross-file navigation.

- Significantly reduces the number of files Claude needs to read manually on large codebases
- Most useful on projects with non-trivial type hierarchies

**Known limitation:** LSP server can fall out of sync after large refactors or incomplete
compilations. Claude detects it but recovery has a token cost. If Claude reports inconsistent
type information, restart the LSP server before continuing.

### Hook — RTK (Rust Token Killer)

CLI proxy that filters and compresses command output to reduce token consumption.

- Meaningful token savings on repetitive dev operations (build, test, git)

**Known limitation:** Over-filtering occurs on some commands — Claude receives incomplete output
and reruns the command in a different form, which can negate the savings.
Observe cases where Claude reruns a command unexpectedly — likely a filtering artifact.

---

## Assess

### MCP — github

GitHub integration — PR management, issue tracking, review comments directly from Claude.

- Relevant for multi-repo workflows or when context-switching between code and issues is frequent
- Risk: actions visible to others (comments, PR updates) — scope carefully


### MCP — postgres / sqlite / mongodb

Direct database queries from Claude. MongoDB is the primary database.

- Useful for debugging, data exploration, migration validation
- Requires strict scoping — read-only access recommended

### Hook — pre-tool-use (destructive commands)

Intercept destructive shell commands (`rm`, `git reset --hard`, `git push --force`, etc.)
and prompt for confirmation before execution.

- Complements Claude's built-in caution but enforces it at the tool level

### Hook — post-edit (auto-format)

Trigger `cargo fmt` / `gofmt` automatically after each file edit.

- Removes the need to include formatting in quality gate reminders
- Verify it does not conflict with LSP diagnostics mid-edit

### Skill — review-pr

Custom PR review skill applying project-specific criteria:
architecture rules, quality gates, naming conventions.

### Skill — add-feature

Guided feature scaffolding following hexagonal + CQRS structure:
domain model → commands → events → adapter → wiring.

### MCP — Serena (oraios/serena)

Symbol-aware code intelligence via LSP exposed as MCP tools (`find_symbol`, `find_referencing_symbols`,
`insert_after_symbol`, etc.). Supports 40+ languages.

- Overlaps directly with the LSP plugin (already in Trial) but operates at the MCP level — potentially
  more token-efficient and without the desync limitation
- If it delivers on the promise, it could replace the LSP plugin entirely
- Requires `uv` + per-language server installation

### MCP — Playwright (@playwright/mcp)

Browser automation via MCP — navigation, clicks, form input, screenshots, network interception,
multi-browser (Chromium, Firefox, WebKit).

- Natural fit for E2E testing on Leptos SSR and React portals
- Higher-level than Chrome DevTools — cross-browser, headless-friendly, test-oriented

### MCP — Chrome DevTools (ChromeDevTools/chrome-devtools-mcp)

Low-level browser access via Chrome DevTools Protocol — console logs with source-mapped stack
traces, network inspection, performance tracing (CrUX), screenshots.

- Complements Playwright: where Playwright handles automation, Chrome DevTools handles debugging
  and profiling
- Caution: exposes all browser content to the MCP client — avoid sessions with sensitive data
- Collects usage statistics by default (opt-out available)

### MCP — markitdown (microsoft/markitdown)

Converts file formats (PDF, Word, Excel, PowerPoint, HTML, images, audio…) to Markdown for LLM
consumption. Ships an MCP server package (`markitdown-mcp`).

- Useful for feeding external documentation or specs to Claude without manual copy-paste
- Output is optimized for machine consumption, not human-readable fidelity

---

## Hold

### MCP — sequential-thinking

Forces explicit step-by-step decomposition before responding.

- Sonnet and Opus already do this natively through training — the MCP duplicates a built-in
  capability and adds token overhead with no observed benefit
