use std::collections::HashMap;

/// Okapi BM25 scorer for document retrieval.
///
/// Standard parameters: k1=1.2, b=0.75.
pub struct BM25 {
    k1: f64,
    b: f64,
    /// Indexed documents for scoring.
    docs: Vec<Vec<String>>,
    /// Inverse document frequency per term.
    idf: HashMap<String, f64>,
    /// Average document length.
    avg_dl: f64,
}

impl BM25 {
    pub fn new(k1: f64, b: f64) -> Self {
        Self {
            k1,
            b,
            docs: Vec::new(),
            idf: HashMap::new(),
            avg_dl: 0.0,
        }
    }

    /// Creates a BM25 scorer with standard defaults (k1=1.2, b=0.75).
    pub fn standard() -> Self {
        Self::new(1.2, 0.75)
    }

    /// Indexes a set of documents.
    pub fn build(&mut self, documents: &[String]) {
        let n = documents.len() as f64;
        self.docs = documents.iter().map(|d| tokenize(d)).collect();

        if self.docs.is_empty() {
            return;
        }

        self.avg_dl = self.docs.iter().map(|d| d.len() as f64).sum::<f64>() / n;

        // Compute IDF
        let mut df: HashMap<String, usize> = HashMap::new();
        for doc in &self.docs {
            let unique: std::collections::HashSet<&String> = doc.iter().collect();
            for term in unique {
                *df.entry(term.clone()).or_insert(0) += 1;
            }
        }

        self.idf = df
            .into_iter()
            .map(|(term, freq)| {
                let idf = ((n - freq as f64 + 0.5) / (freq as f64 + 0.5) + 1.0).ln();
                (term, idf.max(0.0))
            })
            .collect();
    }

    /// Scores a single document against a query.
    pub fn score(&self, query: &str, doc_index: usize) -> f64 {
        if doc_index >= self.docs.len() {
            return 0.0;
        }

        let query_terms = tokenize(query);
        let doc = &self.docs[doc_index];
        let dl = doc.len() as f64;

        // Count term frequencies in this document
        let mut tf: HashMap<&String, usize> = HashMap::new();
        for term in doc {
            *tf.entry(term).or_insert(0) += 1;
        }

        let mut score = 0.0;
        for term in &query_terms {
            let idf = self.idf.get(term).copied().unwrap_or(0.0);
            let freq = tf.get(term).copied().unwrap_or(0) as f64;

            let numerator = freq * (self.k1 + 1.0);
            let denominator = freq + self.k1 * (1.0 - self.b + self.b * (dl / self.avg_dl));

            score += idf * (numerator / denominator);
        }

        score
    }

    /// Scores a query string directly against raw text.
    pub fn score_text(&self, query: &str, text: &str) -> f64 {
        // Build a temporary single-doc index
        let mut temp = BM25::new(self.k1, self.b);
        temp.build(&[text.to_string()]);
        temp.score(query, 0)
    }

    /// Returns the number of indexed documents.
    pub fn doc_count(&self) -> usize {
        self.docs.len()
    }
}

/// Simple whitespace + punctuation tokenizer, lowercase.
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
    fn tokenize_basic() {
        let tokens = tokenize("Hello, World! This is a test.");
        assert!(tokens.contains(&"hello".to_string()));
        assert!(tokens.contains(&"world".to_string()));
        assert!(tokens.contains(&"test".to_string()));
        // Single-char words filtered
        assert!(!tokens.contains(&"a".to_string()));
    }

    #[test]
    fn bm25_scoring() {
        let mut bm = BM25::standard();
        bm.build(&[
            "rust programming language".to_string(),
            "python programming language".to_string(),
            "cooking recipes for dinner".to_string(),
        ]);

        let s1 = bm.score("rust programming", 0);
        let s2 = bm.score("rust programming", 1);
        let s3 = bm.score("rust programming", 2);

        // Rust doc should score highest for "rust programming"
        assert!(s1 > s2, "rust doc ({s1}) should score higher than python ({s2})");
        assert!(s1 > s3, "rust doc ({s1}) should score higher than cooking ({s3})");
    }

    #[test]
    fn bm25_empty() {
        let bm = BM25::standard();
        assert_eq!(bm.doc_count(), 0);
        assert_eq!(bm.score("query", 0), 0.0);
    }

    #[test]
    fn bm25_score_text() {
        let bm = BM25::standard();
        let score = bm.score_text("rust programming", "rust is a systems programming language");
        assert!(score > 0.0);
    }
}
