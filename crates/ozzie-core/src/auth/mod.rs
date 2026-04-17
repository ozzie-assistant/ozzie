mod traits;

// Only the port (trait + error) stays in core.
// Implementations (LocalAuth, DeviceAuth, CompositeAuth) live in ozzie-gateway.
pub use traits::{AuthError, Authenticator, extract_bearer};
