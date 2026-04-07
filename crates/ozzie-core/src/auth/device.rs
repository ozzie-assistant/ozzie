use super::traits::{AuthError, Authenticator};

/// Multi-device authenticator backed by `DeviceStorage`.
///
/// Validates the bearer token against stored device records and updates
/// `last_seen` on every successful authentication.
pub struct DeviceAuth {
    devices: std::sync::Arc<dyn crate::domain::DeviceStorage>,
}

impl DeviceAuth {
    pub fn new(devices: std::sync::Arc<dyn crate::domain::DeviceStorage>) -> Self {
        Self { devices }
    }
}

#[async_trait::async_trait]
impl Authenticator for DeviceAuth {
    async fn authenticate(&self, token: &str) -> Result<String, AuthError> {
        match self.devices.verify_token(token) {
            Some(record) => {
                let _ = self.devices.touch(&record.device_id);
                Ok(record.device_id)
            }
            None => Err(AuthError::Unauthorized("unknown device token".to_string())),
        }
    }
}
