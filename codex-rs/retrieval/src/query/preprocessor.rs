//! Query preprocessing.
//!
//! Based on Continue's BaseRetrievalPipeline.ts getCleanedTrigrams implementation.

use std::collections::HashSet;

use once_cell::sync::Lazy;

use crate::config::SearchConfig;

/// Query preprocessor.
///
/// Handles tokenization, stop word removal, and stemming.
pub struct QueryPreprocessor {
    stop_words: HashSet<String>,
    config: SearchConfig,
}

/// Processed query with tokens and n-grams.
#[derive(Debug, Clone)]
pub struct ProcessedQuery {
    /// Original query text
    pub original: String,
    /// Processed tokens
    pub tokens: Vec<String>,
    /// N-grams (if enabled)
    pub ngrams: Vec<String>,
}

impl QueryPreprocessor {
    /// Create a new query preprocessor.
    pub fn new(config: SearchConfig) -> Self {
        Self {
            stop_words: default_stop_words(),
            config,
        }
    }

    /// Process a query.
    ///
    /// Steps:
    /// 1. Normalize whitespace
    /// 2. Tokenize
    /// 3. Remove stop words
    /// 4. Stem tokens (if enabled)
    /// 5. Deduplicate
    /// 6. Generate n-grams (if enabled)
    pub fn process(&self, query: &str) -> ProcessedQuery {
        // Step 1: Normalize whitespace
        let normalized = normalize_whitespace(query);

        // Step 2: Tokenize
        let tokens = tokenize(&normalized);

        // Step 3: Remove stop words
        let filtered: Vec<_> = tokens
            .into_iter()
            .filter(|t| !self.stop_words.contains(&t.to_lowercase()))
            .collect();

        // Step 4: Stem tokens (if enabled)
        let stemmed = if self.config.enable_stemming {
            stem_tokens(&filtered)
        } else {
            filtered
        };

        // Step 5: Deduplicate
        let unique: Vec<_> = stemmed
            .into_iter()
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();

        // Step 6: Generate n-grams (if enabled)
        let ngrams = if self.config.enable_ngrams {
            generate_ngrams(&unique.join(" "), self.config.ngram_size)
        } else {
            Vec::new()
        };

        ProcessedQuery {
            original: query.to_string(),
            tokens: unique,
            ngrams,
        }
    }
}

/// Normalize whitespace (collapse multiple spaces).
fn normalize_whitespace(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Tokenize a string.
fn tokenize(s: &str) -> Vec<String> {
    s.split(|c: char| c.is_whitespace() || ".,;:!?()[]{}\"'".contains(c))
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect()
}

/// Stem tokens using rust-stemmers.
fn stem_tokens(tokens: &[String]) -> Vec<String> {
    use rust_stemmers::Algorithm;
    use rust_stemmers::Stemmer;

    let en_stemmer = Stemmer::create(Algorithm::English);

    tokens
        .iter()
        .map(|t| {
            // Only stem ASCII alphabetic tokens
            if t.chars().all(|c| c.is_ascii_alphabetic()) {
                en_stemmer.stem(t).to_string()
            } else {
                t.clone()
            }
        })
        .collect()
}

/// Generate n-grams from text.
fn generate_ngrams(text: &str, n: i32) -> Vec<String> {
    let words: Vec<&str> = text.split_whitespace().collect();
    if words.len() < n as usize {
        return vec![text.to_string()];
    }

    words.windows(n as usize).map(|w| w.join(" ")).collect()
}

/// Default stop words (English and Chinese).
fn default_stop_words() -> HashSet<String> {
    STOP_WORDS.iter().map(|s| s.to_string()).collect()
}

/// Static stop words list.
static STOP_WORDS: Lazy<Vec<&str>> = Lazy::new(|| {
    vec![
        // English stop words
        "the",
        "a",
        "an",
        "is",
        "are",
        "was",
        "were",
        "be",
        "been",
        "being",
        "have",
        "has",
        "had",
        "do",
        "does",
        "did",
        "will",
        "would",
        "could",
        "should",
        "may",
        "might",
        "can",
        "this",
        "that",
        "these",
        "those",
        "i",
        "you",
        "he",
        "she",
        "it",
        "we",
        "they",
        "what",
        "which",
        "who",
        "whom",
        "how",
        "when",
        "where",
        "why",
        "all",
        "each",
        "every",
        "both",
        "few",
        "more",
        "most",
        "other",
        "some",
        "such",
        "no",
        "not",
        "only",
        "same",
        "so",
        "than",
        "too",
        "very",
        "just",
        "but",
        "and",
        "or",
        "if",
        "because",
        "as",
        "until",
        "while",
        "of",
        "at",
        "by",
        "for",
        "with",
        "about",
        "against",
        "between",
        "into",
        "through",
        "during",
        "before",
        "after",
        "above",
        "below",
        "to",
        "from",
        "up",
        "down",
        "in",
        "out",
        "on",
        "off",
        "over",
        "under",
        // Chinese stop words
        "的",
        "了",
        "和",
        "是",
        "就",
        "都",
        "而",
        "及",
        "与",
        "着",
        "或",
        "一个",
        "没有",
        "我们",
        "你们",
        "他们",
        "它们",
        "这个",
        "那个",
        "这些",
        "那些",
        "什么",
        "怎么",
        "如何",
        "为什么",
    ]
});

/// Check if text contains Chinese characters.
pub fn contains_chinese(text: &str) -> bool {
    text.chars().any(|c| matches!(c, '\u{4e00}'..='\u{9fff}'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_whitespace() {
        assert_eq!(normalize_whitespace("hello   world"), "hello world");
        assert_eq!(normalize_whitespace("  leading"), "leading");
        assert_eq!(normalize_whitespace("trailing  "), "trailing");
    }

    #[test]
    fn test_tokenize() {
        let tokens = tokenize("hello, world! How are you?");
        assert_eq!(tokens, vec!["hello", "world", "How", "are", "you"]);
    }

    #[test]
    fn test_contains_chinese() {
        assert!(contains_chinese("用户认证"));
        assert!(contains_chinese("hello 世界"));
        assert!(!contains_chinese("hello world"));
    }

    #[test]
    fn test_process_query() {
        let config = SearchConfig::default();
        let preprocessor = QueryPreprocessor::new(config);
        let result = preprocessor.process("how to authenticate users");

        // "how" and "to" should be removed as stop words
        assert!(!result.tokens.contains(&"how".to_string()));
        assert!(!result.tokens.contains(&"to".to_string()));
        // "authenticate" and "users" should remain (possibly stemmed)
        assert!(result.tokens.iter().any(|t| t.contains("authent")));
        assert!(result.tokens.iter().any(|t| t.contains("user")));
    }
}
