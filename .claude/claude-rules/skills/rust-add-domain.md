---
title: "Skill: Add Domain Module (Rust)"
name: rust-add-domain
description: Add a new business module container to the Rust DI system (AppState pattern)
---

Add a new business module to the DI container system following the established pattern (@rules/rust/di-container.md).

Ask the user for the module name if not provided, then execute the following steps in order.

Use $MODULE for the module name in snake_case (e.g. `invoice`) and $Module for PascalCase (e.g. `Invoice`).

---

## Step 1 — Module crate

Verify the module crate exists and exposes at minimum:
- A services trait `${Module}Services`
- A services implementation

Depending on the module, it may also expose repositories, query types, etc.
If the minimum is missing, stop and inform the user before continuing.

---

## Step 2 — Module container struct

Create the container at the appropriate location:

```rust
use std::sync::Arc;
use tokio::sync::OnceCell;
// OnceCell: lazily initialize expensive resources on first access
// while keeping the container itself cheap to construct at startup.

pub struct ${Module}Container {
    services: Arc<dyn ${Module}Services + Send + Sync>,
    // Add OnceCell fields for resources expensive to init and not always needed:
    // heavy_resource: OnceCell<Arc<HeavyResource>>,
}

impl ${Module}Container {
    /// All direct dependencies passed explicitly — no service locator.
    pub async fn new(
        // list every direct dep: database, config, other module services, etc.
    ) -> Result<Self> {
        todo!()
    }

    pub fn services(&self) -> &Arc<dyn ${Module}Services + Send + Sync> {
        &self.services
    }

    // Add route method only if this module exposes HTTP endpoints.
    // The signature is framework-specific — adapt to the router type in use.
    // pub async fn routes(&self) -> Result<Router> { todo!() }

    // Add register_xxx(self, ...) methods only for cross-module wiring (Phase 2).
    // Consuming self makes the state transition explicit and prevents double-registration.
    // pub fn register_notifications(mut self, notifier: Arc<dyn Notifier>) -> Result<Self> { }
}
```

---

## Step 3 — Add to AppState struct

In `dependency_container.rs`, add the container field:

```rust
pub struct AppState {
    // ... existing fields
    pub(crate) ${module}: ${Module}Container,
}
```

---

## Step 4 — Wire in AppState::new()

Construct the container in the correct topological position — after its dependencies, before its dependents. Pass all direct dependencies explicitly:

```rust
// Phase 1
let ${module} = ${Module}Container::new(
    database.clone(),
    other_module.services().clone(), // only if actually needed
).await?;

// In Ok(Self { ... }):
${module},
```

---

## Step 5 — Register cross-module wiring (if needed)

If this module reacts to other modules (or vice versa), add a consuming registration in `init_side_effects()`:

```rust
// Consuming pattern — register_xxx takes self and returns Self
self.${module} = self.${module}.register_xxx(/* cross-module dep */)?;
```

If the module is self-contained, skip this step entirely.

---

## Step 6 — AppState accessor file

Create `di/${module}_container.rs`:

```rust
use super::AppState;

impl AppState {
    pub fn ${module}_services(&self) -> &Arc<dyn ${Module}Services + Send + Sync> {
        self.${module}.services()
    }

    // Add route accessor if the module exposes HTTP endpoints:
    // pub async fn ${module}_routes(&self) -> Result<Router> {
    //     self.${module}.routes().await
    // }
}
```

Declare the module in `di/mod.rs`:

```rust
mod ${module}_container;
```

---

## Step 7 — Register routes (if applicable)

If the module exposes HTTP endpoints, add one entry to the route registry file.
The exact API depends on the HTTP framework in use:

```rust
// e.g. in routes_container.rs
("/api/v1/${modules}", self.${module}_routes().await?),
```

Add the path constant at the top of the file:

```rust
pub const ${MODULE_UPPER}_API_PATH: &str = "/api/v1/${modules}";
```

---

## Step 8 — Verify

```bash
cargo build
cargo clippy -- -D warnings
cargo test
```

---

## Checklist

- [ ] Module crate exposes services trait and implementation
- [ ] `${Module}Container::new(...)` takes all direct deps explicitly
- [ ] Container field added to `AppState` struct
- [ ] Container constructed in `AppState::new()` in correct topological order
- [ ] `register_xxx(self, ...)` added and called in `init_side_effects()` if cross-module wiring needed
- [ ] `di/${module}_container.rs` created with `impl AppState` accessors
- [ ] Module declared in `di/mod.rs`
- [ ] Route entry added to route registry if module exposes HTTP endpoints
- [ ] Quality gates pass
