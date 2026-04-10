pub mod auth;
pub mod handler;
pub mod hub;
pub mod mcp;
pub mod memory_api;
pub mod pair_device;
pub mod profile_api;
pub mod pairing;
pub mod protocol;
pub mod server;

pub use auth::{auth_middleware, DeviceId};
pub use hub::{Hub, HubHandler};
pub use pair_device::DeviceApprovalCache;
pub use protocol::Frame;
pub use server::{AppState, GatewayError, Server, ServerConfig};
