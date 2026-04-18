// Re-export the port from core — single source of truth.
pub use ozzie_core::auth::{AuthError, Authenticator, extract_bearer};

/// Constant-time byte comparison to prevent timing attacks.
///
/// Both length mismatch and content mismatch take the same code path
/// to avoid leaking the token length via timing.
pub(crate) fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    let mut diff = a.len() ^ b.len();
    for i in 0..a.len().min(b.len()) {
        diff |= (a[i] ^ b[i]) as usize;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equal_slices() {
        assert!(constant_time_eq(b"hello", b"hello"));
    }

    #[test]
    fn different_content() {
        assert!(!constant_time_eq(b"hello", b"world"));
    }

    #[test]
    fn different_length() {
        assert!(!constant_time_eq(b"short", b"longer"));
    }
}
