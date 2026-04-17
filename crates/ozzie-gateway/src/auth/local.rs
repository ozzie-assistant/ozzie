use super::traits::{AuthError, Authenticator, constant_time_eq};

/// Token-based local authenticator.
///
/// Stores a plaintext token in memory and validates against it
/// using constant-time comparison.
pub struct LocalAuth {
    token: String,
}

impl LocalAuth {
    /// Creates a new local authenticator with the given token.
    pub fn new(token: impl Into<String>) -> Self {
        Self {
            token: token.into(),
        }
    }

    /// Generates a random hex token and creates the authenticator.
    pub fn generate() -> Self {
        use rand::Rng;
        let mut rng = rand::rng();
        let bytes: Vec<u8> = (0..32).map(|_| rng.random::<u8>()).collect();
        let token = hex::encode(&bytes);
        Self::new(token)
    }

    /// Returns the token for storage.
    pub fn token(&self) -> &str {
        &self.token
    }
}

#[async_trait::async_trait]
impl Authenticator for LocalAuth {
    async fn authenticate(&self, token: &str) -> Result<String, AuthError> {
        // Constant-time comparison
        if constant_time_eq(token.as_bytes(), self.token.as_bytes()) {
            Ok("local".to_string())
        } else {
            Err(AuthError::Unauthorized("invalid token".to_string()))
        }
    }
}

/// Insecure authenticator that accepts everything (dev mode).
pub struct InsecureAuth;

#[async_trait::async_trait]
impl Authenticator for InsecureAuth {
    async fn authenticate(&self, _token: &str) -> Result<String, AuthError> {
        Ok("insecure".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn local_auth_valid() {
        let auth = LocalAuth::new("secret_token");
        let result = auth.authenticate("secret_token").await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "local");
    }

    #[tokio::test]
    async fn local_auth_invalid() {
        let auth = LocalAuth::new("secret_token");
        let result = auth.authenticate("wrong_token").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn insecure_auth_always_ok() {
        let auth = InsecureAuth;
        let result = auth.authenticate("anything").await;
        assert!(result.is_ok());
    }

    #[test]
    fn generate_token() {
        let auth = LocalAuth::generate();
        assert_eq!(auth.token().len(), 64); // 32 bytes = 64 hex chars
    }
}
