/// Estimates tokens from text (4 chars ≈ 1 token).
pub fn estimate_tokens(text: &str) -> usize {
    text.len().div_ceil(4)
}

/// Truncates text to fit within the given token budget.
pub fn trim_to_tokens(text: &str, max_tokens: usize) -> &str {
    if max_tokens == 0 {
        return "";
    }
    let max_chars = max_tokens * 4;
    if text.len() <= max_chars {
        return text;
    }
    // Find a valid UTF-8 boundary
    let mut end = max_chars;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

/// Splits messages into chunks of `chunk_size`.
pub fn chunk_messages(messages: &[String], chunk_size: usize) -> Vec<Vec<String>> {
    messages.chunks(chunk_size).map(|c| c.to_vec()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_estimation() {
        assert_eq!(estimate_tokens(""), 0);
        assert_eq!(estimate_tokens("hi"), 1);
        assert_eq!(estimate_tokens("hello world"), 3); // 11 chars → ~3 tokens
    }

    #[test]
    fn trim_basic() {
        let text = "a".repeat(100);
        let trimmed = trim_to_tokens(&text, 10); // 10 tokens = 40 chars
        assert_eq!(trimmed.len(), 40);
    }

    #[test]
    fn trim_zero() {
        assert_eq!(trim_to_tokens("hello", 0), "");
    }

    #[test]
    fn trim_no_truncation() {
        assert_eq!(trim_to_tokens("hi", 100), "hi");
    }

    #[test]
    fn chunking() {
        let msgs: Vec<String> = (0..10).map(|i| format!("msg{i}")).collect();
        let chunks = chunk_messages(&msgs, 4);
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0].len(), 4);
        assert_eq!(chunks[2].len(), 2);
    }
}
