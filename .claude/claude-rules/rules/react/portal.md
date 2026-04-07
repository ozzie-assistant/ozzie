---
title: "React Portal Architecture"
---

## Business Models — OpenAPI Generation

Business models are **generated from the backend OpenAPI spec** — no manual type duplication.

```bash
# Regenerate after backend changes
npm run generate:api   # fetches spec from running backend, outputs to api/generated/
```

Generated output lives in `api/generated/` and includes:
- TypeScript types matching backend structs exactly
- TanStack Query hooks (`useQuery` / `useMutation`) per endpoint
- Zod schemas for runtime validation

**Rule:** never hand-write types that mirror backend models — always generate.

## Server State — TanStack Query

All server interactions go through **generated TanStack Query hooks**. No raw `fetch`, no manual `useEffect` for data fetching.

```tsx
// features/billing/api/index.ts — re-export generated hooks, add domain context
export { useBillingFilter, useBillingCreate } from '@/api/generated/@tanstack/react-query.gen';

// features/billing/components/BillingList.tsx
const { data, isLoading } = useBillingFilter({ query: { skip: 0, limit: 20 } });
```

Cache invalidation is handled by generated mutation hooks — mutations invalidate related queries by convention.

## App State — Portal Context

Portal-wide state (current user, locale, theme) lives in `core/contexts/` as React Contexts.
Not in global stores — these are stable values that change rarely and wrap the full app.

```
core/
└── contexts/
    ├── auth-context.tsx      # current_user, isAuthenticated, logout()
    ├── locale-context.tsx    # locale, setLocale()
    └── theme-context.tsx     # theme, setTheme()
```

Provider hierarchy in `app/providers.tsx`:

```tsx
<LocaleProvider>
  <ThemeProvider>
    <AuthProvider>       {/* depends on locale for error messages */}
      <QueryClientProvider>
        {children}
      </QueryClientProvider>
    </AuthProvider>
  </ThemeProvider>
</LocaleProvider>
```

Consume via typed hooks:

```tsx
const { user, logout } = useAuth();
const { locale } = useLocale();
```

## Cross-Feature State

Cross-feature data goes through server state (TanStack Query), not shared stores.
If two features need the same data, they each call the same generated hook — the cache deduplicates the request.

Never share state between features via props-drilling or a shared store — use the query cache.

## `api/` Structure

```
api/
└── generated/              # never edit manually
    ├── types.gen.ts
    ├── sdk.gen.ts
    └── @tanstack/
        └── react-query.gen.ts

core/
└── http/
    └── client.ts           # Axios instance — auth interceptor (Bearer token), base URL
```

## Rules

- All server interactions via generated TanStack Query hooks — no raw fetch in features
- Never edit files in `api/generated/` — regenerate instead
- Portal state (user, locale, theme) in `core/contexts/`, not in feature-level state
- Cross-feature data via query cache, not shared stores
