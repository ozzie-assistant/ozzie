---
title: "Dependency Injection — Rust"
---

## Structure

```
DependencyContainer  →  build_app_state()  →  AppState
                                               ├── infrastructure clients (db, APIs)
                                               └── XxxContainer × N

di/
├── dependency_container.rs  → DependencyContainer + AppState + AppState::new() + init_side_effects()
├── routes_container.rs      → route registry (axum)
└── {module}_container.rs    → impl AppState accessors for that module
```

## Two-Phase Initialization

**Phase 1** (`new()`) — resolve all containers in topological order, direct deps as constructor args.
**Phase 2** (`init_side_effects()`) — wire cross-module callbacks. Each `register_xxx` **consumes `self`** to prevent double-registration.

```rust
impl AppState {
    async fn new(config: AppConfig) -> Result<Self> {
        let company = CompanyContainer::new(database.clone()).await?;
        let license = LicenseContainer::new(database.clone(), company.services().clone()).await?;
        Ok(Self { company, license })
    }
    async fn init_side_effects(mut self) -> Result<Self> {
        self.tenant = self.tenant.register_notifications(self.notification.clone())?;
        Ok(self)
    }
}
```

## Module Container

```rust
pub struct XxxContainer {
    services: Arc<XxxServicesImpl>,
    heavy_resource: OnceCell<Arc<HeavyResource>>,  // lazy: cost paid on first access
}
impl XxxContainer {
    pub async fn new(database: Database, dep: Arc<dyn DepServices + Send + Sync>) -> Result<Self> { }
    pub fn services(&self) -> &Arc<XxxServicesImpl> { &self.services }
    pub async fn heavy_resource(&self) -> Result<&Arc<HeavyResource>> {
        self.heavy_resource.get_or_try_init(|| async { HeavyResource::connect().await }).await
    }
    pub fn register_notifications(mut self, notifier: Arc<dyn Notifier>) -> Result<Self> { Ok(self) }
}
```

Each `{module}_container.rs` — accessors only, no logic:

```rust
impl AppState {
    pub fn xxx_services(&self) -> &Arc<XxxServicesImpl> { self.xxx.services() }
}
```

## Route Registry

```rust
impl AppState {
    pub async fn all_routes(&self) -> Result<Vec<(&'static str, Router)>> {
        Ok(vec![("/api/v1/companies", self.company.routes().await?)])
    }
}
```

Adding a module = one line here. Only place modules are listed.

## Eager vs Lazy Init

- Services, repositories, clients → eager in `new()` (fail-fast on bad config)
- Connection pools, compiled assets → lazy via `OnceCell` (cheap startup)
- Optional infra clients → `Option<Client>` (degrade gracefully)

## Rules

- `new(...)` — all direct dependencies explicit, no global state, no service locator
- `register_xxx(self, ...)` — consuming, Phase 2 only, prevents double-registration
- No business logic in `DependencyContainer` or `AppState` — wiring only
- Route registry is the only place modules are listed
