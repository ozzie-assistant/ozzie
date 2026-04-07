# Ozzie — Pairing System

Architecture document. Covers the full pairing design across the three
client categories: device clients, chat connectors, and local CLI.

---

## 1. Three Categories of Pairing

Pairing establishes **who is allowed to interact with the gateway and under which
constraints**. Three categories with distinct mechanisms:

| Category | Examples | Trust established by | Credential (client-side) |
|----------|----------|---------------------|--------------------------|
| **Local CLI** | `ozzie ask`, same-machine TUI | Shared token file on filesystem | `~/.ozzie/token` |
| **Device client** | Remote TUI, webapp, Tauri, mobile | Key exchange + admin approval | Per-platform credential store |
| **Chat connector** | Discord, Slack, Teams, Telegram | Message DM + admin approval | None — policy resolved per-message |

These are fundamentally different problems:

- **Local CLI** — already works. Token on shared filesystem. No pairing needed.
- **Device client** — trust a new *application* connecting from a potentially remote host.
  Results in a stored credential the client uses for all future connections.
- **Chat connector** — trust a *human user* on a third-party platform.
  Results in a `PairingStore` entry mapping their identity to a policy.
  No credential issued — their identity is validated on every message.

---

## 2. Existing Foundations

### What exists today

| Component | File | Status |
|-----------|------|--------|
| `PairingKey` + `Pairing` + `PairingStore` (JSON, RwLock) | `ozzie-core/src/policy/pairing.rs` | Complete |
| `Policy` struct + 4 predefined policies (admin/support/executor/readonly) | `ozzie-core/src/policy/types.rs` | Complete |
| `PolicyResolver` — merges predefined + config overrides | `ozzie-core/src/policy/resolver.rs` | Complete |
| `Authenticator` trait + `LocalAuth` + `InsecureAuth` | `ozzie-core/src/auth/mod.rs` | Complete |
| Bearer token auth middleware | `ozzie-gateway/src/auth.rs` | Complete |
| `Identity` (platform/server_id/channel_id/user_id) | `ozzie-core/src/connector/types.rs` | Complete |
| `PairingRequest` / `PairingApproved` event variants | `ozzie-core/src/events/types.rs` | **Empty — no fields, never emitted** |

### `PairingKey` specificity resolution (existing)

```
exact match (platform + server_id + channel_id + user_id)
  → channel wildcard (user_id = "*")
  → server wildcard  (channel_id = "*", user_id = "*")
  → platform wildcard (server_id = "*", channel_id = "*", user_id = "*")
```

### Predefined policies

| Policy | session_mode | allowed_tools | approval_mode | client_facing |
|--------|-------------|---------------|---------------|---------------|
| `admin` | persistent | all | sync | true |
| `support` | ephemeral | all except run_command/write_file/edit_file | none | true |
| `executor` | per-request | all | none | false |
| `readonly` | ephemeral | none | none | true |

---

## 3. Device Client Pairing

### 3.1 Problem

Remote clients (TUI on another machine, webapp, Tauri app) cannot share the local
token file. They need a way to request trust from the gateway and receive a credential
they can store locally.

### 3.2 Flow

```
Client (TUI / webapp / Tauri)             Gateway
  │                                          │
  │── POST /api/pair ────────────────────────>│
  │   { client_type, label, pubkey_fp? }      │
  │                                          │  PendingDevices.insert(request_id)
  │<── 202 Accepted { request_id } ───────────│  bus.publish(PairingRequest { ... })
  │                                          │
  │  (polling GET /api/pair/{request_id})     │
  │                                          │  Admin sees request:
  │                                          │    TUI prompt / CLI / already-paired device
  │                                          │  ozzie pairing approve <request_id>
  │                                          │
  │<── 200 OK { token, device_id } ───────────│  DeviceStorage.add(record)
  │                                          │  bus.publish(PairingApproved { ... })
  │
  │  CredentialStore.save(token, gateway_url)
  │
  │── WS connect  Authorization: Bearer <token> ─>│
```

### 3.3 Client-side: `CredentialStore` trait

The credential storage format is identical across all device clients. Storage backend
varies by platform.

```rust
// crates/ozzie-client/src/credential.rs

pub struct Credential {
    pub token: String,
    pub device_id: String,
    pub gateway_url: String,
    pub issued_at: DateTime<Utc>,
    pub label: Option<String>,
}

pub trait CredentialStore: Send + Sync {
    fn save(&self, credential: &Credential) -> Result<(), CredentialError>;
    fn load(&self) -> Result<Option<Credential>, CredentialError>;
    fn clear(&self) -> Result<(), CredentialError>;
}

/// Filesystem — TUI, Tauri, CLI on remote host.
/// Default path: $OZZIE_PATH/client.json (or --credential-file override)
pub struct FileCredentialStore { path: PathBuf }

/// In-memory — tests, ephemeral sessions.
pub struct MemoryCredentialStore { inner: Mutex<Option<Credential>> }
```

Web clients (webapp) implement the equivalent in JavaScript:
`LocalStorageCredentialStore`, `IndexedDbCredentialStore` — same interface, different
runtime. Not in `ozzie-client` (Rust), implemented in the JS client SDK.

### 3.4 Gateway-side: `DeviceStorage` trait

```rust
// crates/ozzie-core/src/domain/ports.rs

pub struct DeviceRecord {
    pub device_id: String,       // generated by gateway on approval
    pub client_type: String,     // "tui" | "webapp" | "tauri" | "mobile"
    pub label: Option<String>,   // human label: "MacBook Pro Michael"
    pub token: String,           // bearer token for WS auth
    pub paired_at: DateTime<Utc>,
    pub last_seen: Option<DateTime<Utc>>,
}

pub trait DeviceStorage: Send + Sync {
    fn add(&self, record: DeviceRecord) -> Result<(), PairingError>;
    fn verify_token(&self, token: &str) -> Option<DeviceRecord>;
    fn list(&self) -> Vec<DeviceRecord>;
    fn revoke(&self, device_id: &str) -> Result<bool, PairingError>;
    fn touch(&self, device_id: &str) -> Result<(), PairingError>; // update last_seen
}
```

Default implementation: `JsonDeviceStore` persisted to `$OZZIE_PATH/devices.json`.

### 3.5 `Authenticator` evolution

The current `LocalAuth` validates a single static token. With `DeviceStorage`:

```rust
/// Multi-device authenticator backed by DeviceStorage.
pub struct DeviceAuth {
    store: Arc<dyn DeviceStorage>,
}

#[async_trait]
impl Authenticator for DeviceAuth {
    async fn authenticate(&self, token: &str) -> Result<String, AuthError> {
        match self.store.verify_token(token) {
            Some(record) => {
                let _ = self.store.touch(&record.device_id);
                Ok(record.device_id)
            }
            None => Err(AuthError::Unauthorized("unknown device token".into())),
        }
    }
}
```

`LocalAuth` is kept for local CLI (single known token from `~/.ozzie/token`).
Gateway initializes with `DeviceAuth` if a `devices.json` exists.

---

## 4. Chat Connector Pairing

### 4.1 Problem

Chat users (Discord, Slack, Teams, Telegram, ...) interact via a bot on their platform.
There is no shared filesystem or token exchange. Trust is established by:

1. A user explicitly requesting pairing (DM to the bot)
2. An admin approving the request
3. The user's `Identity` being stored in `PairingStore` with an assigned policy

On every subsequent message, the connector resolves the user's identity to a policy.
No credential is issued — the identity *is* the credential.

### 4.2 Flow — DM user pairing

```
Chat User                         Bot (Connector)              ConnectorManager / EventRunner
  │                                    │                              │
  │── DM: "/pair" ────────────────────>│                              │
  │                                    │  EventBusSender.on_message   │
  │                                    │  detects command:"pair"       │
  │                                    │  is_dm: true                 │
  │<── "Request submitted (ID: abc)"───│  pm.on_pair_request()        │
  │                                    │  bus.publish(ConnectorReply) │
  │                                    │  bus.publish(PairingRequest) │
  │                                    │                              │  Admin sees:
  │                                    │                              │    TUI / CLI / another channel
  │                                    │                              │  ozzie pairing approve <request_id> --policy support
  │                                    │                              │  PairingStore.add(identity → "support")
  │                                    │                              │  bus.publish(PairingApproved)
  │
  │── "hello ozzie" ──────────────────>│                              │
  │                                    │  EventBusSender publishes    │
  │                                    │  ConnectorMessage { roles }  │
  │                                    │                              │  EventRunner resolves policy
  │                                    │                              │  runs ReAct loop
  │                                    │                              │  publishes ConnectorReply
  │<── agent response ─────────────────│  ConnectorManager routes     │
  │                                    │  connector.reply → send()    │
```

### 4.3 Discord slash commands

Registered automatically when the Discord connector starts (`ready` handler):

| Command  | Scope     | Handler                  | Response                                      |
|----------|-----------|--------------------------|-----------------------------------------------|
| `/pair`  | DM only   | `EventBusSender`         | ACK with request ID; admin approves via CLI   |
| `/status`| Anywhere  | `EventBusSender`         | "✅ Connected. Policy: `support`." or not paired |

Commands are handled in `EventBusSender.on_message()` *before* the message reaches
the event bus — no LLM call for pairing commands.

### 4.4 Role-based access (no explicit pairing required)

Administrators can grant access to entire Discord roles without requiring each user
to `/pair` individually. Discord role IDs are guild-specific snowflakes, so the
config is **per-guild** :

```jsonc
// config.jsonc
"connectors": {
  "discord": {
    "token": "${{ .Env.DISCORD_TOKEN }}",
    "guilds": {
      "123456789012345678": {          // guild_id (Discord server snowflake)
        "admin_channel": "987654321098765432",
        "role_policies": {
          "111222333444555666": "admin",    // role_id → policy_name
          "777888999000111222": "support"
        }
      },
      "222333444555666777": {          // a second guild, independent role IDs
        "role_policies": {
          "333444555666777888": "readonly"
        }
      }
    }
  }
}
```

On every message:
1. Discord connector fetches the sender's guild roles via serenity `get_member()`
2. Roles are carried in `IncomingMessage.roles` and `ConnectorMessage.roles`
3. `PairingManager.resolve_policy(identity, roles)` checks:
   - Exact identity match in `PairingStore` first (explicit `/pair` approval)
   - Then guild-scoped role fallback: looks up `identity.server_id` in
     `guild_role_policies`, then checks each role — first match wins

### 4.5 Flow — Channel setup via commands (future)

```
Admin invites bot to a server/workspace
  → Connector registers slash commands on platform
  → /ozzie setup channel #dev-chat --policy support
  → /ozzie setup role @senior-dev --policy admin
      ↓
PairingStore entries:
  { platform: "discord", server_id: X, channel_id: Y, user_id: "*" } → "support"
  (role-based: handled via role_policies config, not stored in PairingKey)
```

### 4.6 The `Connector` trait — extended

```rust
// crates/ozzie-core/src/connector/types.rs

pub trait Connector: Send + Sync {
    // Existing
    async fn start(&self, bus: Arc<dyn EventBus>) -> Result<(), ConnectorError>;
    async fn send(&self, msg: OutgoingMessage) -> Result<(), ConnectorError>;
    async fn stop(&self) -> Result<(), ConnectorError>;

    // New — commands to register on the platform (slash commands, app commands, etc.)
    // Default: pair + setup + status
    fn pairing_commands(&self) -> Vec<ConnectorCommand> {
        ConnectorCommand::defaults()
    }

    // New — user's roles at the time of a message (platform-specific resolution)
    // Discord: member.roles, Slack: user groups, Teams: AAD groups
    // Default: no roles (platforms without role concept)
    async fn resolve_roles(
        &self,
        identity: &Identity,
    ) -> Result<Vec<String>, ConnectorError> {
        Ok(Vec::new())
    }
}

pub struct ConnectorCommand {
    pub name: String,        // "pair", "setup", "status"
    pub description: String,
    pub scope: CommandScope,
}

pub enum CommandScope {
    DmOnly,      // "pair" — DM only
    ChannelOnly, // "setup" — channel admin only
    Any,         // "status" — anywhere
}
```

### 4.7 `IncomingMessage` — enriched

```rust
pub struct IncomingMessage {
    pub identity: Identity,
    pub content: String,
    pub is_dm: bool,

    // New
    pub command: Option<String>,  // "pair" | "setup" | "status" | None
    pub command_args: Vec<String>, // parsed args from the command
    pub roles: Vec<String>,       // roles resolved at message time (opaque strings)
}
```

### 4.8 Role resolution — design decision

Role IDs (Discord snowflakes, Slack group IDs) are platform-specific. Two options:

**Option A — Roles in `PairingKey`** (generic but leaky)
```rust
pub struct PairingKey {
    pub platform: String,
    pub server_id: String,
    pub channel_id: String,
    pub user_id: String,
    pub role_id: String,  // "" = no role filter
}
```
Specificity: `user_exact` → `role_match` → `channel_wildcard` → `server_wildcard`

**Option B — Role→policy in connector config** (platform-specific, clean domain)
```jsonc
// config.jsonc
"connectors": {
  "discord": {
    "role_policies": {
      "1234567890": "admin",   // role_id → policy_name
      "9876543210": "support"
    }
  }
}
```
Connector resolves roles → policy before PairingStore lookup. PairingKey stays clean.

**Decision: Option B** for role-based access. Role IDs are Discord/Slack artifacts
and do not belong in a domain port. The connector resolves roles to a policy name
*before* creating the Identity, or passes `roles: Vec<String>` to the PairingManager
which applies the config mapping.

---

## 5. `PairingManager` — Unified Runtime Component

A single component in `ozzie-runtime` handles both flows. It owns:
- `PendingPairings` — temporary store for pending requests (both device and chat)
- Subscribes to connector messages and PairingApproved events
- Sends approval notifications back to connectors

```rust
// crates/ozzie-runtime/src/pairing_manager.rs

pub struct PairingManager {
    pending: Arc<dyn PendingPairings>,
    chat_storage: Arc<dyn PairingStorage>,
    device_storage: Arc<dyn DeviceStorage>,
    connector_manager: Arc<ConnectorManager>,
    bus: Arc<dyn EventBus>,
}

impl PairingManager {
    // Called by ConnectorManager when IncomingMessage has command == "pair"
    pub fn on_pair_request(&self, msg: &IncomingMessage) { ... }

    // Called on /setup command
    pub fn on_setup_command(&self, msg: &IncomingMessage) { ... }

    // Called when admin runs: ozzie pairing approve <request_id> --policy X
    pub async fn approve_chat(&self, request_id: &str, policy: &str) -> Result<...> { ... }

    // Called when admin runs: ozzie pairing approve <request_id> (device)
    pub async fn approve_device(&self, request_id: &str) -> Result<DeviceRecord, ...> { ... }

    // Called by EventRunner on every IncomingMessage (non-command)
    pub fn resolve_policy(&self, identity: &Identity, roles: &[String]) -> Option<String> { ... }
}
```

---

## 6. New Domain Ports

All in `crates/ozzie-core/src/domain/ports.rs`:

```rust
// --- Chat pairing storage ---

pub trait PairingStorage: Send + Sync {
    fn add(&self, pairing: &Pairing) -> Result<(), PairingError>;
    fn remove(&self, key: &PairingKey) -> Result<bool, PairingError>;
    fn resolve(&self, identity: &Identity) -> Option<String>;
    fn list(&self) -> Vec<Pairing>;
}

// Default: JsonPairingStore (rename of current PairingStore)

// --- Device pairing storage ---

pub trait DeviceStorage: Send + Sync {
    fn add(&self, record: DeviceRecord) -> Result<(), PairingError>;
    fn verify_token(&self, token: &str) -> Option<DeviceRecord>;
    fn list(&self) -> Vec<DeviceRecord>;
    fn revoke(&self, device_id: &str) -> Result<bool, PairingError>;
    fn touch(&self, device_id: &str) -> Result<(), PairingError>;
}

// Default: JsonDeviceStore ($OZZIE_PATH/devices.json)

// --- Pending pairing requests (both flows share this) ---

pub struct PendingRequest {
    pub request_id: String,
    pub kind: PendingKind,
    pub requested_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

pub enum PendingKind {
    Device {
        client_type: String,
        label: Option<String>,
    },
    Chat {
        identity: Identity,
        display_name: String,
        platform_message: String, // raw message for context
    },
}

pub trait PendingPairings: Send + Sync {
    fn insert(&self, req: PendingRequest);
    fn list(&self) -> Vec<PendingRequest>;
    fn take(&self, request_id: &str) -> Option<PendingRequest>; // consumes on approval
    fn purge_expired(&self);
}

// Default: MemoryPendingPairings (in-memory with TTL, no persistence needed)
```

---

## 7. Event Payload Enrichment

Current `PairingRequest` / `PairingApproved` have no fields. Required:

```rust
// crates/ozzie-core/src/events/types.rs
enum Event {
    #[serde(rename = "pairing.request")]
    PairingRequest {
        request_id: String,
        kind: String,           // "device" | "chat"
        // Device fields
        client_type: Option<String>,
        label: Option<String>,
        // Chat fields
        platform: Option<String>,
        server_id: Option<String>,
        channel_id: Option<String>,
        user_id: Option<String>,
        display_name: Option<String>,
    },

    #[serde(rename = "pairing.approved")]
    PairingApproved {
        request_id: String,
        kind: String,             // "device" | "chat"
        approved_by: String,      // "cli" | "tui" | "discord:<channel_id>" | ...
        policy_name: Option<String>, // chat pairing only
        device_id: Option<String>,   // device pairing only
    },

    #[serde(rename = "pairing.rejected")]
    PairingRejected {
        request_id: String,
        rejected_by: String,
    },
}
```

---

## 8. WebSocket Protocol Additions

New methods in `ozzie-protocol`:

```
// Client → Gateway (unauthenticated, device pairing)
"pair_request"   { client_type: String, label: Option<String> }

// Gateway → Client (on approval)
"pair_response"  { token: String, device_id: String }

// Admin client → Gateway (list pending requests)
"pairing_list"   {}  →  { requests: Vec<PendingRequestSummary> }

// Admin client → Gateway (approve)
"pairing_approve" { request_id: String, policy: Option<String> }
```

HTTP alternative for clients that need to poll:

```
POST   /api/pair                    → 202 { request_id }
GET    /api/pair/{request_id}       → 200 { token } | 202 pending | 403 rejected
GET    /api/pairings                → 200 { devices: [...], chats: [...] }
DELETE /api/pairings/devices/{id}   → 204
DELETE /api/pairings/chats/{key}    → 204
```

---

## 9. CLI Commands

```
ozzie pairing requests              List pending pairing requests (device + chat)
ozzie pairing approve <id>          Approve a request (device: issues token, chat: stores policy)
ozzie pairing approve <id> --policy support   Approve chat with specific policy
ozzie pairing reject <id>           Reject a request

ozzie pairing devices               List paired devices
ozzie pairing devices revoke <id>   Revoke a device

ozzie pairing chats                 List chat pairings (identity → policy)
ozzie pairing chats add             Add a chat pairing manually
ozzie pairing chats remove          Remove a chat pairing
```

---

## 10. Session Policy Enforcement

When a session is created from a chat connector message, or an admin client
that has a specific policy:

```rust
// crates/ozzie-runtime/src/session.rs
pub struct Session {
    // Existing fields...

    // New
    pub policy_name: Option<String>,   // resolved policy, None = admin (full access)
    pub device_id: Option<String>,     // which device created this session
}
```

`EventRunner` and `TaskRunner` check the resolved policy before executing:

- `allowed_tools` — filter tool list passed to ReactLoop
- `denied_tools` — remove from tool list
- `approval_mode: "none"` — bypass dangerous tool approval for this session
- `client_facing` — inject Persona in system prompt
- `max_concurrent` — enforced by SessionManager (not yet implemented)

---

## 11. Implementation Status

```
Phase 1 — Foundations
──────────────────────────────────────────────────────────────────
[x] PairingStorage trait + JsonPairingStore (rename PairingStore)
[x] DeviceStorage trait + JsonDeviceStore
[x] PendingPairings trait + MemoryPendingPairings
[x] DeviceRecord struct
[x] Enrich PairingRequest/PairingApproved/PairingRejected events

Phase 2 — Chat connector pairing
──────────────────────────────────────────────────────────────────
[x] Extend IncomingMessage (command, command_args, roles, is_dm)
[x] PairingManager in ozzie-runtime
[x] PairingManager.resolve_policy(identity, roles) — explicit + role fallback
[x] ozzie pairing CLI (requests/approve/reject/chats)
[x] Wire PairingManager into ConnectorManager (EventBusSender)
[x] ConnectorMessage event carries roles for policy resolution
[x] EventRunner.handle_connector_message uses roles in resolve_policy

Phase 3 — Device client pairing
──────────────────────────────────────────────────────────────────
[x] CredentialStore trait + FileCredentialStore in ozzie-client
[x] DeviceAuth authenticator (replaces single-token LocalAuth for multi-device)
[x] POST /api/pair + GET /api/pair/{id} gateway endpoints
[x] OzzieClient.acquire_token_cli — full device pairing flow for CLI
[x] ozzie ask/events/pairing CLI commands use acquire_token_cli
[ ] pair_request/pair_response WS frames in ozzie-protocol (future)
[ ] ozzie pairing devices CLI (future)

Phase 4 — Policy enforcement
──────────────────────────────────────────────────────────────────
[x] Session.policy_name field
[ ] EventRunner: filter tools by resolved policy (future)
[ ] ozzie pairing approve wires DeviceAuth for remote TUI (future)

Phase 5 — Discord implementation
──────────────────────────────────────────────────────────────────
[x] Discord connector: slash commands registered (pair, status)
[x] Discord connector: resolve_roles via serenity get_member (roles in IncomingMessage)
[x] Role→policy config (connectors.discord.role_policies → PairingManager.new_with_roles)
[x] ConnectorReply event variant — bus-based routing back to connectors
[x] ConnectorManager.start_reply_listener — subscribes connector.reply, routes to connector
[x] EventRunner.finalize_response — publishes ConnectorReply when session has connector metadata
[x] /pair command handler: ACK + PairingRequest event (DM only)
[x] /status command handler: reports pairing status + policy to user
```

---

## 12. Open Questions

1. **`PairingRequest` event variant vs flat fields** — use `#[serde(tag)]` enum for
   kind discrimination, or flat struct with Option fields? Flat is simpler for the TUI
   to display; tagged enum is more type-safe.

2. **Pending request TTL** — 15 minutes seems reasonable. Configurable in config?

3. **Device token rotation** — should devices rotate tokens periodically, or is a
   long-lived token acceptable for a personal agent? Could add `ozzie pairing refresh`.

4. **Offline approval** — if admin is offline when a chat pairing request arrives,
   the request expires. Auto-reject after TTL, or notify on next admin login?

5. **Ed25519 vs token** — the device pairing flow above uses a bearer token issued by
   the gateway. A future upgrade could use Ed25519 keypairs (challenge-response) for
   stronger security without shared secrets. The `DeviceRecord` has a `pubkey_fingerprint`
   field reserved for this. Phase 1 uses tokens, Phase N upgrades to Ed25519.

6. **Multiple admins** — `ozzie pairing approve` assumes one admin. If multiple admins
   are paired (e.g., two TUI clients with admin policy), any of them can approve.
   This is the intended behavior.
