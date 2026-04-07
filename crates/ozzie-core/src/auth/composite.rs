use super::traits::{AuthError, Authenticator};

/// Tries a list of authenticators in order, succeeding on the first match.
///
/// Used to support both the single-token `LocalAuth` (for `ozzie ask` and
/// other CLI tools that read `.token` directly) and `DeviceAuth` (for paired
/// TUI/device clients that hold a per-device UUID token).
pub struct CompositeAuth {
    authenticators: Vec<std::sync::Arc<dyn Authenticator>>,
}

impl CompositeAuth {
    pub fn new(authenticators: Vec<std::sync::Arc<dyn Authenticator>>) -> Self {
        Self { authenticators }
    }
}

#[async_trait::async_trait]
impl Authenticator for CompositeAuth {
    async fn authenticate(&self, token: &str) -> Result<String, AuthError> {
        for auth in &self.authenticators {
            if let Ok(identity) = auth.authenticate(token).await {
                return Ok(identity);
            }
        }
        Err(AuthError::Unauthorized("authentication failed".to_string()))
    }
}
