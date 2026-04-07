---
title: "Leptos Patterns (0.8+)"
---

## Signals — Choosing the Right Primitive

| Primitive | Reactive | Ownership | Use for |
|-----------|----------|-----------|---------|
| `RwSignal<T>` | ✅ | Arena-allocated | Local component state, read-write in one value |
| `ReadSignal<T>` / `WriteSignal<T>` | ✅ | Arena-allocated | Split read/write access |
| `ArcRwSignal<T>` | ✅ | Ref-counted | Cross-owner / cross-component sharing |
| `StoredValue<T>` | ❌ | Arena-allocated | Non-reactive data in reactive closures (callbacks, config) |
| `Memo<T>` | ✅ (derived) | Arena-allocated | Derived values, computed once per dependency change |

Use `StoredValue` for non-Copy data that does **not** need to trigger re-renders.
Use `RwSignal` when the value must trigger reactivity.

## Resource + Suspense

Standard pattern for async data fetching. The source signal drives re-fetching automatically:

```rust
let items = Resource::new(
    move || search.get(),          // source: re-runs fetcher when this changes
    |s| {
        let search = if s.is_empty() { None } else { Some(s) };
        list_items(search)
    },
);

view! {
    <Suspense fallback=|| view! { <Skeleton /> }>
        {move || items.get().map(|result| match result {
            Ok(items) => view! { /* render */ }.into_any(),
            Err(e) => view! { <ErrorMessage error=e.to_string() /> }.into_any(),
        })}
    </Suspense>
}
```

**`|| ()` source = fetches exactly once.** No dependency → the resource never re-fetches after
the initial load. Use this intentionally for static reference data (e.g. a list of plans in a
dialog), and document it:

```rust
// Plans are static during a session — fetch once, no re-fetch needed.
let plans = Resource::new(|| (), |_| list_plans(None));
```

If re-fetch on open/close is needed, use a trigger signal instead:

```rust
let trigger = RwSignal::new(0u32);
let plans = Resource::new(move || trigger.get(), |_| list_plans(None));
// trigger.update(|n| *n += 1) to force a refresh
```

Use `LocalResource` when the future does not need to be `Send` (client-side only, no SSR):

```rust
let items = LocalResource::new(move || fetch_local(search.get()));
```

## StoredValue vs Double-Clone for Non-Copy Data

The core problem: `view!` generates reactive closures that must be `Fn`. A `move` closure
capturing a non-Copy value (e.g. `String`) that is then moved into a nested closure makes
the outer closure `FnOnce` — causing a compile error.

**Two valid solutions:**

### StoredValue — use when data is accessed in multiple closures

`get_value()` returns a fresh clone each call without consuming the `StoredValue`.

```rust
let data = StoredValue::new(some_string);

view! {
    <Show when=move || open.get()>
        {move || data.get_value()}
    </Show>
    <button on:click=move |_| use_something(data.get_value())>"Click"</button>
}
```

### Double-clone — use when data is accessed in a single event handler

Clone outside the `move` closure (so the outer context keeps its copy), clone again inside
for the `async move` block:

```rust
let id = some_string; // non-Copy

on:click={
    let id = id.clone();        // outside: outer view closure keeps `id`
    move |_| {
        let id = id.clone();    // inside: async block gets its own copy
        spawn_local(async move {
            do_thing(id).await;
        });
    }
}
```

**Choosing between them:**

| Situation | Pattern |
|-----------|---------|
| Data used in one event handler | Double-clone |
| Data used in multiple closures (`Show`, `For`, effects, handlers) | `StoredValue` |
| Data is `Copy` (bool, u32, …) | Neither — capture directly |

## Control Flow — `Either` vs `into_any()`

For binary branches, prefer `Either` to preserve type information:

```rust
// GOOD — typed, no erasure
view! {
    {if logged_in {
        Either::Left(view! { <Dashboard /> })
    } else {
        Either::Right(view! { <Login /> })
    }}
}

// Use into_any() when branches have more than 2 types or in match arms
view! {
    {move || match state.get() {
        State::Loading => view! { <Spinner /> }.into_any(),
        State::Error(e) => view! { <Error msg=e /> }.into_any(),
        State::Ready(data) => view! { <Content data=data /> }.into_any(),
    }}
}
```

## Action / ServerAction vs spawn_local

Two patterns for calling server functions. Choose based on whether you have a `<form>`.

### `ServerAction` — for form-driven mutations

Prefer when using `<ActionForm>`. Built-in `.pending()` and `.value()` signals.

```rust
let create_user = ServerAction::<CreateUser>::new();

view! {
    <Show when=move || create_user.pending().get()>
        <Spinner />
    </Show>
    {move || create_user.value().get().map(|res| match res {
        Ok(_) => view! { <SuccessMessage /> }.into_any(),
        Err(e) => view! { <ErrorMessage error=e.to_string() /> }.into_any(),
    })}
    <ActionForm action=create_user>
        <input type="text" name="username" />
        <button type="submit">"Create"</button>
    </ActionForm>
}
```

Key properties: `.pending()`, `.value()`, `.input()` (current arg while pending), `.version()` (completion count).

### `spawn_local` — for imperative mutations (no form)

Use in `on:click` / toggle handlers where you own the control flow:

```rust
on:click={
    let id = id.clone();
    move |_| {
        let id = id.clone();
        spawn_local(async move {
            match delete_item(id).await {
                Ok(()) => on_success.run(()),
                Err(e) => error_signal.set(Some(e.to_string())),
            }
        });
    }
}
```

Always set an error signal on `Err` — do not log to console only (user gets no feedback).

## Async Actions in Event Handlers

Clone outside the `move` closure, then again inside the async block:

```rust
on:click={
    let id = id.clone();           // clone OUTSIDE move closure
    move |_: leptos::ev::MouseEvent| {
        let id = id.clone();       // clone again for async block
        leptos::task::spawn_local(async move {
            match do_thing(id).await {
                Ok(_) => { /* handle success */ }
                Err(e) => { /* handle error */ }
            }
        });
    }
}
```

## Server Functions

```rust
#[server]
pub async fn list_items(search: Option<String>) -> Result<Vec<ItemView>, ServerFnError> {
    // Extract typed context (DI via Leptos context)
    let store = item_store()?;
    let result = store
        .filter(search)
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?;
    Ok(result)
}
```

- Server functions are SSR-only — do not reference WASM-incompatible types
- Extract dependencies via typed Leptos contexts, not global state
