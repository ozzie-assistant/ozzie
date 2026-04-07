use std::collections::HashMap;

/// Common words excluded from keyword extraction (English + French).
const STOP_WORDS: &[&str] = &[
    // English
    "the", "a", "an", "and", "or", "but", "in", "on", "at", "to", "for", "of", "is", "it",
    "that", "this", "with", "from", "by", "as", "be", "was", "are", "were", "been", "has",
    "have", "had", "not", "no", "do", "does", "did", "will", "would", "can", "could", "should",
    "may", "might",
    // French
    "le", "la", "les", "un", "une", "des", "du", "de", "et", "ou", "en", "dans", "sur", "pour",
    "par", "est", "sont", "pas", "que", "qui", "ce", "se", "ne", "je", "tu", "il", "elle",
    "nous", "vous", "ils",
];

/// Extracts the top `max` keywords from text by frequency.
/// Ties are broken alphabetically for determinism.
pub fn extract_keywords(text: &str, max: usize) -> Vec<String> {
    let stop: std::collections::HashSet<&str> = STOP_WORDS.iter().copied().collect();

    let mut freq: HashMap<String, usize> = HashMap::new();
    for token in tokenize(text) {
        if !stop.contains(token.as_str()) {
            *freq.entry(token).or_insert(0) += 1;
        }
    }

    let mut pairs: Vec<(String, usize)> = freq.into_iter().collect();
    pairs.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));

    pairs.into_iter().take(max).map(|(w, _)| w).collect()
}

/// Splits text into lowercase tokens, keeping only 2+ char alphanumeric words.
fn tokenize(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty() && s.len() >= 2)
        .map(|s| s.to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_extraction() {
        let text = "rust programming language rust systems programming";
        let kw = extract_keywords(text, 3);
        // "programming" and "rust" both freq 2, alphabetical tiebreak
        assert_eq!(kw[0], "programming");
        assert_eq!(kw[1], "rust");
        assert_eq!(kw.len(), 3);
    }

    #[test]
    fn filters_stop_words() {
        let text = "the quick brown fox is a very fast animal";
        let kw = extract_keywords(text, 10);
        assert!(!kw.contains(&"the".to_string()));
        assert!(!kw.contains(&"is".to_string()));
        assert!(!kw.contains(&"a".to_string()));
        assert!(kw.contains(&"quick".to_string()));
    }

    #[test]
    fn french_stop_words() {
        let text = "le chat est sur la table dans le jardin";
        let kw = extract_keywords(text, 5);
        assert!(!kw.contains(&"le".to_string()));
        assert!(!kw.contains(&"est".to_string()));
        assert!(kw.contains(&"chat".to_string()));
        assert!(kw.contains(&"table".to_string()));
    }

    #[test]
    fn empty_text() {
        let kw = extract_keywords("", 5);
        assert!(kw.is_empty());
    }

    #[test]
    fn respects_max() {
        let text = "one two three four five six seven eight nine ten";
        let kw = extract_keywords(text, 3);
        assert_eq!(kw.len(), 3);
    }

    #[test]
    fn deterministic_tiebreak() {
        // All words have frequency 1, should be sorted alphabetically
        let text = "zebra apple mango";
        let kw = extract_keywords(text, 10);
        assert_eq!(kw, vec!["apple", "mango", "zebra"]);
    }
}
