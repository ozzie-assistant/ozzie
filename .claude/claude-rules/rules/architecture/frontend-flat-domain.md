---
title: "Frontend Architecture — Flat-Domain Modular Portal"
---

Applicable to React/TypeScript and Leptos/Rust. The structure is identical regardless of the framework.

## Module Map

```
src/
├── ui/          # Design system — atomic & molecular components
├── layouts/     # Structural shells — slot/children injection only
├── core/        # Global infrastructure — HTTP client, auth, i18n, config
├── features/    # Business modules — one directory per domain
│   └── {domain}/
│       ├── components/   # Domain-coupled components (e.g. InvoiceList)
│       ├── api/          # Network calls — hooks (React) or resources (Leptos)
│       └── logic/        # Local state, validation, derived values
└── pages/       # Route entry points — assembly only
```

## Dependency Rules

```
pages/ → features/ → ui/
pages/ → layouts/
features/ → core/
ui/ → (nothing — zero business knowledge)
core/ → (nothing — no feature imports)
```

- `ui/` has **zero** knowledge of the API, business domain, or global state
- `features/` never imports from another `features/` module — cross-feature data goes through `core/`
- `pages/` contains no logic — it assembles layouts and features only
- `layouts/` defines injection zones (slots/children) but carries no business content

## Layer Responsibilities

### `ui/` — Design System

Pure visual components. No API calls, no global state, no business types.
Styling via Tailwind CSS with static variant definitions (CVA pattern) — no dynamic class strings that escape the compiler scan.

```
ui/
├── button/
├── input/
├── modal/
└── table/
```

### `layouts/` — Structural Shells

High-level page structures that define where content goes. Accept children/slots only.

```
layouts/
├── main-layout/
├── sidebar-layout/
└── dashboard-shell/
```

### `core/` — Global Infrastructure

Shared primitives needed to communicate with the outside world. Initialized once, consumed everywhere.

```
core/
├── http/       # Base HTTP client, interceptors
├── auth/       # Token management, session
└── i18n/       # Translations, locale
```

### `features/{domain}/` — Business Module

Self-contained vertical slice. Everything a feature needs lives inside its own directory.
Deleting a feature = deleting its directory, with no dead code left behind.

```
features/billing/
├── components/   # BillingList, InvoiceCard — coupled to billing data
├── api/          # useBillingData() / Resource::new(...)
└── logic/        # validation, state machines, derived values
```

### `pages/` — Route Orchestration

Glue only. Imports a layout and one or more features, wires them together for a route.

```
pages/
├── dashboard/
└── settings/
```

## State Categories

Three distinct categories — never conflate them:

| Category | What | Lives in | Example |
|----------|------|----------|---------|
| **Server state** | Data from the backend | `features/{domain}/api/` | invoice list, user profile |
| **App state** | Portal-wide context | `core/` | current_user, locale, theme |
| **Local state** | UI-only, ephemeral | `features/{domain}/logic/` | modal open, form draft |

**Cross-feature data goes through server state** — two features that need the same data each
fetch it independently. The caching layer (TanStack Query, SSR) deduplicates the request.
Never route data between features via shared stores or props-drilling.

## Business Models

Business models are the shared language between frontend and backend. How they are shared
depends on the technology:

- **React / TypeScript** — generated from OpenAPI spec (single source of truth is the backend)
- **Leptos / Rust** — shared directly via crate dependencies (compiler enforces the contract)

Either way: never hand-write types that duplicate backend models.

## Rules for Adding a Feature

1. Create `features/{domain}/` with `components/`, `api/`, `logic/`
2. Build UI elements from `ui/` — do not create ad-hoc styled elements in `features/`
3. Register the route entry point in `pages/`
4. If global infrastructure is needed (auth token, HTTP client) → consume from `core/`, do not duplicate

## LLM Boundary Contract

When asked to build a new view:
- New visual primitives → `ui/`
- New business view → `features/{domain}/components/`
- New data fetching → `features/{domain}/api/`
- New route → `pages/`
- Never mix layers within a single component file
