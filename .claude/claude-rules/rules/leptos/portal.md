---
title: "Leptos Portal Architecture"
---

> **Note** — Project-specific conventions (DI wiring, folder structure, auth patterns) belong
> in your project's own CLAUDE.md, not here. This file covers generic SSR-first architecture.

## Core Principle — SSR-First

Unlike SPA portals, server state is the **default** in Leptos SSR. The server renders the full
page with data on the first request — no client-side loading spinners for initial content.

```
SPA mindset:  render shell → client fetches data → re-render
SSR mindset:  server fetches data → renders full HTML → client hydrates
```

## Business Models — Shared via Crate

No API client generation. Business models are **shared directly** from backend crates as
Rust types. The frontend crate depends on the domain crate.

```toml
# portal/Cargo.toml
[dependencies]
my-domain = { path = "../domain" }  # types shared, no duplication
```

Server functions receive and return domain types directly — type safety without a generation step.

## Server State — SSR + Resources

Server-rendered data is fetched in server functions and passed to the view. For reactive
re-fetching on the client, use `Resource`:

```rust
// Rendered on server, reactive on client
let invoices = Resource::new(
    move || (page.get(), search.get()),
    |(page, search)| fetch_invoices(page, search),   // server function
);

view! {
    <Suspense fallback=|| view! { <Skeleton /> }>
        {move || invoices.get().map(|res| match res {
            Ok(data) => view! { <InvoiceList data=data /> }.into_any(),
            Err(e) => view! { <ErrorMessage error=e.to_string() /> }.into_any(),
        })}
    </Suspense>
}
```

Use `LocalResource` for client-only data (no SSR requirement, no `Send` constraint).

## App State — Portal Context via Cookies

Portal-wide state (current user, locale, theme) bridges server and client via **httpOnly cookies**.

### Why cookies over localStorage

- `localStorage` is client-only — SSR renders without it → flash of wrong theme / no user
- httpOnly cookies are sent with every request → server reads them → first HTML render is correct
- Hydration then gives the client the same value — no extra fetch, no re-render

### `current_user`

```
Login → JWT in httpOnly cookie
         ↓
Every SSR request → server extracts JWT → validates → fetches additional user data
                                                      ↓
                                          provide_context(cx, CurrentUser)
                                                      ↓
                                          First render has user — no flash
                                                      ↓
                                          Hydration → client signal has value
```

Server-side extraction (in Axum middleware or server function):

```rust
// Server: extract JWT cookie → build CurrentUser → inject into Leptos context
provide_context(current_user);

// Component: consume anywhere
let user = use_context::<CurrentUser>().expect("CurrentUser not provided");
```

### `locale` / `theme`

Cookie-based persistence (server-readable, no FOUC):

```rust
// Server reads locale cookie → provide_context(Locale::Fr)
// Component:
let locale = use_context::<Locale>().unwrap_or_default();
```

Abstract the preference source behind a struct in context — components stay unchanged
whether the source is a cookie or a DB:

```rust
pub struct UserPreferences { pub locale: Locale, pub theme: Theme }
provide_context(user_prefs); // source is opaque to consumers
```

## Cross-Feature State

In SSR-first, most "cross-feature state" is simply server state re-fetched per route.
For client-side shared state that must persist across navigation, use Leptos signals stored
at the app root and passed via context:

```rust
// App root
let theme = RwSignal::new(Theme::default());
provide_context(theme);

// Any component
let theme = use_context::<RwSignal<Theme>>().unwrap();
```

## Feature Module Structure

Recommended structure — adapt to your project's conventions:

```
features/billing/
├── components/         # Leptos components — consume domain types directly
├── server/             # Server functions (#[server] — SSR only)
└── logic/              # Local signals, derived values, client-side state
```

No `api/` folder — server functions replace the API layer. Domain types come from crate deps.

## Rules

- First render must be correct — no loading flash for portal state (user, locale, theme)
- App state bridged via httpOnly cookies → server context → hydrated signals
- Business types shared via crate dependencies — never duplicate domain types in the portal
- Server functions are SSR-only — no WASM-incompatible dependencies (see `gotchas.md`)
