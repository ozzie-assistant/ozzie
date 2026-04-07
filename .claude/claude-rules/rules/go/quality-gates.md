---
title: "Go Quality Gates"
---

Every change must pass all four gates before being considered done:

```bash
go build ./...              # Successful compilation
golangci-lint run ./...     # Zero lint warnings (includes staticcheck, errcheck, govet, gofmt…)
go test -race ./...         # All tests pass, no data races
govulncheck ./...           # No known vulnerabilities in dependencies
```

All four are equally important. Any lint warning, race condition, or test failure is treated as a build failure.

## golangci-lint

`golangci-lint` is the standard Go linter aggregator — it runs staticcheck and a curated set of additional linters in one pass.

Minimum recommended linters (`.golangci.yml`):

```yaml
linters:
  enable:
    - staticcheck   # SA*, S1*, QF* checks
    - errcheck      # unhandled errors
    - govet         # go vet checks
    - gofmt         # formatting
    - gosimple      # simplification suggestions
    - unused        # unused code
    - gosec         # security anti-patterns
```

- No `//nolint` without an explicit justification comment on the same line
- `staticcheck:ignore` directives are also forbidden without justification

## Style

- Follow standard Go conventions (`gofmt`, Effective Go)
- Prefer `internal/` packages — avoid exporting at the module root unless intentional
- Error wrapping: use `fmt.Errorf("context: %w", err)` consistently
