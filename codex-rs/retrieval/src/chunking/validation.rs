//! Token-based chunk validation.
//!
//! Ensures chunks don't exceed embedding model token limits.
//! Uses tiktoken (cl100k_base) for OpenAI-compatible token counting.
//!
//! Reference: Continue's `core/indexing/chunk/chunk.ts:70`

use crate::types::ChunkSpan;
use tiktoken_rs::cl100k_base;

/// Default maximum tokens per chunk for embedding models.
pub const DEFAULT_MAX_CHUNK_TOKENS: usize = 512;

/// Chunk validator for token-based size limits.
pub struct ChunkValidator {
    bpe: tiktoken_rs::CoreBPE,
    max_tokens: usize,
}

impl ChunkValidator {
    /// Create a new chunk validator with default max tokens (512).
    pub fn new() -> Self {
        Self::with_max_tokens(DEFAULT_MAX_CHUNK_TOKENS)
    }

    /// Create a new chunk validator with custom max tokens.
    pub fn with_max_tokens(max_tokens: usize) -> Self {
        let bpe = cl100k_base().expect("Failed to load cl100k_base tokenizer");
        Self { bpe, max_tokens }
    }

    /// Count tokens in a text string.
    pub fn count_tokens(&self, text: &str) -> usize {
        self.bpe.encode_with_special_tokens(text).len()
    }

    /// Check if a chunk is within the token limit.
    pub fn is_valid(&self, chunk: &str) -> bool {
        self.count_tokens(chunk) <= self.max_tokens
    }

    /// Validate and filter chunks, splitting oversized ones.
    ///
    /// Returns chunks that are within the token limit.
    /// Oversized chunks are split at sentence/line boundaries.
    pub fn validate_chunks(&self, chunks: Vec<ChunkSpan>) -> Vec<ChunkSpan> {
        let mut validated = Vec::new();

        for chunk in chunks {
            let token_count = self.count_tokens(&chunk.content);

            if token_count <= self.max_tokens {
                validated.push(chunk);
            } else {
                // Split oversized chunk
                tracing::debug!(
                    tokens = token_count,
                    max = self.max_tokens,
                    start_line = chunk.start_line,
                    "Splitting oversized chunk"
                );
                let split_chunks = self.split_chunk(&chunk);
                validated.extend(split_chunks);
            }
        }

        validated
    }

    /// Split an oversized chunk into smaller valid chunks.
    fn split_chunk(&self, chunk: &ChunkSpan) -> Vec<ChunkSpan> {
        let lines: Vec<&str> = chunk.content.lines().collect();
        let mut result = Vec::new();
        let mut current_content = String::new();
        let mut current_start_line = chunk.start_line;
        let mut lines_in_current = 0;

        for (i, line) in lines.iter().enumerate() {
            let line_with_newline = if current_content.is_empty() {
                line.to_string()
            } else {
                format!("\n{}", line)
            };

            let test_content = format!("{}{}", current_content, line_with_newline);

            if self.count_tokens(&test_content) > self.max_tokens && !current_content.is_empty() {
                // Save current chunk
                result.push(ChunkSpan {
                    content: current_content,
                    start_line: current_start_line,
                    end_line: current_start_line + lines_in_current - 1,
                });

                // Start new chunk
                current_content = line.to_string();
                current_start_line = chunk.start_line + i as i32;
                lines_in_current = 1;
            } else {
                current_content = test_content;
                lines_in_current += 1;
            }
        }

        // Don't forget the last chunk
        if !current_content.is_empty() {
            result.push(ChunkSpan {
                content: current_content,
                start_line: current_start_line,
                end_line: current_start_line + lines_in_current.max(1) - 1,
            });
        }

        result
    }
}

impl Default for ChunkValidator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_count_tokens() {
        let validator = ChunkValidator::new();

        // Simple text
        let tokens = validator.count_tokens("Hello, world!");
        assert!(tokens > 0);
        assert!(tokens < 10);

        // Longer text should have more tokens
        let long_text = "The quick brown fox jumps over the lazy dog. ".repeat(10);
        let long_tokens = validator.count_tokens(&long_text);
        assert!(long_tokens > tokens);
    }

    #[test]
    fn test_is_valid() {
        let validator = ChunkValidator::with_max_tokens(10);

        assert!(validator.is_valid("Hello"));
        assert!(!validator.is_valid(&"word ".repeat(100)));
    }

    #[test]
    fn test_validate_chunks_small() {
        let validator = ChunkValidator::new();

        let chunks = vec![ChunkSpan {
            content: "Small chunk".to_string(),
            start_line: 1,
            end_line: 1,
        }];

        let validated = validator.validate_chunks(chunks);
        assert_eq!(validated.len(), 1);
        assert_eq!(validated[0].content, "Small chunk");
    }

    #[test]
    fn test_validate_chunks_splits_large() {
        let validator = ChunkValidator::with_max_tokens(50);

        // Create a chunk that's definitely too large
        let large_content = (1..=100)
            .map(|i| format!("Line {i}"))
            .collect::<Vec<_>>()
            .join("\n");

        let chunks = vec![ChunkSpan {
            content: large_content,
            start_line: 1,
            end_line: 100,
        }];

        let validated = validator.validate_chunks(chunks);

        // Should be split into multiple chunks
        assert!(validated.len() > 1);

        // Each chunk should be valid
        for chunk in &validated {
            assert!(
                validator.is_valid(&chunk.content),
                "Chunk should be within token limit"
            );
        }
    }

    #[test]
    fn test_split_preserves_line_numbers() {
        let validator = ChunkValidator::with_max_tokens(20);

        let content = "Line 1\nLine 2\nLine 3\nLine 4\nLine 5";
        let chunks = vec![ChunkSpan {
            content: content.to_string(),
            start_line: 10,
            end_line: 14,
        }];

        let validated = validator.validate_chunks(chunks);

        // First chunk should start at line 10
        assert_eq!(validated[0].start_line, 10);

        // Check line numbers are reasonable
        for chunk in &validated {
            assert!(chunk.start_line >= 10);
            assert!(chunk.end_line <= 14);
            assert!(chunk.start_line <= chunk.end_line);
        }
    }
}
