use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// Generates a unique ID with the given prefix.
///
/// Format: `{prefix}_{timestamp_hex}_{counter_hex}`.
/// Unique within a process lifetime.
pub fn generate_id(prefix: &str) -> String {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{prefix}_{ts:x}_{seq:04x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique() {
        let a = generate_id("mem");
        let b = generate_id("mem");
        assert_ne!(a, b);
        assert!(a.starts_with("mem_"));
    }
}
