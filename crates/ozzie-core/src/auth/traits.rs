/// Authenticates incoming requests.
#[async_trait::async_trait]
pub trait Authenticator: Send + Sync {
    /// Validates an auth token and returns a device ID.
    async fn authenticate(&self, token: &str) -> Result<String, AuthError>;
}

#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("unauthorized: {0}")]
    Unauthorized(String),
    #[error("{0}")]
    Other(String),
}

/// Extracts a bearer token from an Authorization header value.
pub fn extract_bearer(header: &str) -> Option<&str> {
    header.strip_prefix("Bearer ")
}

/// Constant-time byte comparison to prevent timing attacks.
pub(crate) fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_bearer_token() {
        assert_eq!(extract_bearer("Bearer abc123"), Some("abc123"));
        assert_eq!(extract_bearer("Basic abc123"), None);
        assert_eq!(extract_bearer(""), None);
    }

    #[test]
    fn constant_time_eq_works() {
        assert!(constant_time_eq(b"hello", b"hello"));
        assert!(!constant_time_eq(b"hello", b"world"));
        assert!(!constant_time_eq(b"short", b"longer"));
    }
}
