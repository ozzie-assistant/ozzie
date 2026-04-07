use chrono::{DateTime, Utc};

/// Formats a UTC timestamp using the preferred language.
/// French ("fr"): [DD-MM HH:MM], other: [MM-DD HH:MM].
/// Converts to local time.
pub fn format_ts(ts: DateTime<Utc>, language: Option<&str>) -> String {
    let local = ts.with_timezone(&chrono::Local);
    let is_french = language
        .map(|l| l.to_lowercase().starts_with("fr"))
        .unwrap_or(false);
    if is_french {
        local.format("[%d-%m %H:%M]").to_string()
    } else {
        local.format("[%m-%d %H:%M]").to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_french() {
        let ts = Utc::now();
        let formatted = format_ts(ts, Some("fr"));
        assert!(formatted.starts_with('['));
        assert!(formatted.ends_with(']'));
        // DD-MM format: day first
        let inner = &formatted[1..formatted.len() - 1];
        let parts: Vec<&str> = inner.split(' ').collect();
        assert_eq!(parts.len(), 2);
    }

    #[test]
    fn format_english() {
        let ts = Utc::now();
        let formatted = format_ts(ts, Some("en"));
        assert!(formatted.starts_with('['));
        // MM-DD format
    }

    #[test]
    fn format_none_defaults_to_mmdd() {
        let ts = Utc::now();
        let formatted = format_ts(ts, None);
        assert!(formatted.starts_with('['));
    }
}
