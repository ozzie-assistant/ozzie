---
title: "Leptos Gotchas (0.8+)"
---

## Component Children

- `Children` = `Box<dyn FnOnce() -> AnyView>` — rendered once
- `ChildrenFn` = `Box<dyn Fn() -> AnyView>` — required for conditional or repeated rendering
- `ChildrenMut` = `Box<dyn FnMut() -> AnyView>` — when mutation between renders is needed
- `<Show>` children must be `Fn`, not `FnOnce` → use `ChildrenFn` for wrapper components used inside `<Show>`
- Use `StoredValue` for non-Copy data accessed inside `Fn` children (see `patterns.md`)

## `<For>` Syntax

`let:entry` syntax has known issues in 0.8 (span/macro interactions) — prefer explicit `children` prop:

```rust
// FRAGILE — let: syntax, test carefully
<For each=move || items.get() key=|item| item.id let:item>
    <Item item=item />
</For>

// PREFERRED — explicit, no macro surprises
<For
    each=move || items.get()
    key=|item| item.id
    children=move |item| view! { <Item item=item /> }
/>
```

## Compilation

- Complex views can hit `queries overflow the depth limit!` → add to `lib.rs`:
  ```rust
  #![recursion_limit = "512"]
  ```
- `leptos_router` and `leptos_meta` 0.8 do **not** have a `"hydrate"` feature — only `leptos` itself does
- Import `web_sys` as `leptos::web_sys` (re-exported), not as a direct crate dependency

## Props

- Prefer `String` over `&str` for component props — use `.to_string()` at call sites
- `#[prop(into)]` eases ergonomics for string props

## WASM Safety

Server functions and anything pulling in `tokio` / `mio` are **not** WASM-safe.

For a crate that compiles to both SSR and WASM (e.g. a portal crate):
1. Declare SSR-only deps as `optional = true` in `Cargo.toml`
2. Enable them only in the `ssr` feature list
3. Verify with: `cargo check -p <crate> --features hydrate --target wasm32-unknown-unknown`

```toml
[dependencies]
my-domain = { workspace = true, optional = true }  # pulls in tokio via cqrs

[features]
ssr  = ["dep:my-domain", ...]
hydrate = [...]  # must NOT include dep:my-domain
```

A CI check against the `hydrate` target is the only reliable guard — the compiler will
catch any accidental SSR import in WASM-compiled code.

## `FnOnce` in `view!` — the non-Copy capture problem

A `move` closure that captures a non-Copy value (e.g. `String`) and then moves it into
a nested `async move` block makes the outer closure `FnOnce`. If that closure is inside
a `view!` that requires `Fn` (e.g. children of `<Show>`, reactive closures), you get:

```
expected a closure that implements the `Fn` trait, but this closure only implements `FnOnce`
```

See `patterns.md` — StoredValue vs Double-Clone — for the two canonical fixes.
