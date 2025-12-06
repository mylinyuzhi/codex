//! Code chunking using text-splitter.
//!
//! Uses CodeSplitter (tree-sitter AST-aware) for supported languages,
//! MarkdownChunker for markdown files, with TextSplitter fallback for others.
//!
//! CodeSplitter benefits:
//! - 3-5x better semantic chunk quality
//! - Respects function/class boundaries
//! - Never splits mid-statement
//!
//! Reference: Tabby's `tabby-index/src/code/intelligence.rs`

use crate::chunking::markdown::MarkdownChunker;
use crate::chunking::markdown::is_markdown_file;
use crate::chunking::validation::ChunkValidator;
use crate::error::Result;
use crate::types::ChunkSpan;
use text_splitter::CodeSplitter;
use text_splitter::TextSplitter;
use tree_sitter::Language;

/// Code chunking service.
///
/// Uses CodeSplitter (tree-sitter AST-aware) for supported languages,
/// MarkdownChunker for markdown files, with TextSplitter fallback for others.
/// Optional token validation ensures chunks fit within embedding model limits.
pub struct CodeChunkerService {
    max_chunk_size: usize,
    chunk_overlap: usize,
    validator: Option<ChunkValidator>,
}

impl CodeChunkerService {
    /// Create a new code chunker service.
    pub fn new(max_chunk_size: usize) -> Self {
        Self {
            max_chunk_size,
            chunk_overlap: 0,
            validator: None,
        }
    }

    /// Create a new code chunker service with overlap.
    pub fn with_overlap(max_chunk_size: usize, chunk_overlap: usize) -> Self {
        Self {
            max_chunk_size,
            chunk_overlap,
            validator: None,
        }
    }

    /// Create a new code chunker service with token validation.
    ///
    /// Chunks will be validated and split if they exceed the token limit.
    pub fn with_validation(max_chunk_size: usize, max_tokens: usize) -> Self {
        Self {
            max_chunk_size,
            chunk_overlap: 0,
            validator: Some(ChunkValidator::with_max_tokens(max_tokens)),
        }
    }

    /// Create a new code chunker service with overlap and token validation.
    pub fn with_overlap_and_validation(
        max_chunk_size: usize,
        chunk_overlap: usize,
        max_tokens: usize,
    ) -> Self {
        Self {
            max_chunk_size,
            chunk_overlap,
            validator: Some(ChunkValidator::with_max_tokens(max_tokens)),
        }
    }

    /// Chunk a source file.
    ///
    /// For markdown files, uses MarkdownChunker which respects header hierarchy.
    /// For supported languages (rust, go, python, java), uses CodeSplitter
    /// which is tree-sitter based and respects syntax boundaries.
    /// Falls back to TextSplitter for unsupported languages.
    ///
    /// If chunk_overlap is configured, adjacent chunks will have overlapping
    /// content at their boundaries for better semantic continuity.
    pub fn chunk(&self, content: &str, language: &str) -> Result<Vec<ChunkSpan>> {
        // Use MarkdownChunker for markdown files
        if is_markdown_file(language) {
            tracing::trace!(language = %language, "Using MarkdownChunker");
            let md_chunker = MarkdownChunker::new(self.max_chunk_size);
            let chunks = md_chunker.chunk(content);
            // Apply token validation if configured
            return Ok(self.maybe_validate(chunks));
        }

        let mut chunks = if let Some(ts_lang) = get_tree_sitter_language(language) {
            if let Ok(splitter) = CodeSplitter::new(ts_lang, self.max_chunk_size) {
                let raw_chunks: Vec<(usize, &str)> = splitter.chunk_indices(content).collect();
                tracing::trace!(
                    language = %language,
                    chunks = raw_chunks.len(),
                    "CodeSplitter chunked file"
                );
                raw_chunks
                    .into_iter()
                    .map(|(offset, chunk)| self.to_chunk_span(content, offset, chunk))
                    .collect()
            } else {
                self.chunk_with_text_splitter(content, language)
            }
        } else {
            self.chunk_with_text_splitter(content, language)
        };

        // Apply overlap if configured and there are multiple chunks
        if self.chunk_overlap > 0 && chunks.len() > 1 {
            self.apply_overlap(&mut chunks);
        }

        // Apply token validation if configured
        Ok(self.maybe_validate(chunks))
    }

    /// Apply token validation if a validator is configured.
    fn maybe_validate(&self, chunks: Vec<ChunkSpan>) -> Vec<ChunkSpan> {
        match &self.validator {
            Some(validator) => {
                let original_count = chunks.len();
                let validated = validator.validate_chunks(chunks);
                if validated.len() != original_count {
                    tracing::debug!(
                        original = original_count,
                        validated = validated.len(),
                        "Token validation adjusted chunk count"
                    );
                }
                validated
            }
            None => chunks,
        }
    }

    fn chunk_with_text_splitter(&self, content: &str, language: &str) -> Vec<ChunkSpan> {
        tracing::trace!(
            language = %language,
            "Using TextSplitter fallback"
        );
        let splitter = TextSplitter::new(self.max_chunk_size);
        let chunks: Vec<(usize, &str)> = splitter.chunk_indices(content).collect();
        chunks
            .into_iter()
            .map(|(offset, chunk)| self.to_chunk_span(content, offset, chunk))
            .collect()
    }

    /// Apply overlap by prepending content from the previous chunk to each subsequent chunk.
    ///
    /// This improves semantic continuity across chunk boundaries by ensuring that
    /// context from the end of one chunk is available at the start of the next.
    ///
    /// Note: start_line and end_line are NOT adjusted because they represent the
    /// actual position in the source file. The overlap is purely for semantic
    /// context and should not affect position metadata used for code navigation.
    fn apply_overlap(&self, chunks: &mut Vec<ChunkSpan>) {
        for i in 1..chunks.len() {
            // Clone the previous content to avoid borrow conflicts
            let prev_content = chunks[i - 1].content.clone();
            if prev_content.len() > self.chunk_overlap {
                let overlap_start = prev_content.len() - self.chunk_overlap;
                // Find line boundary for clean overlap (don't split mid-line)
                let overlap = if let Some(pos) = prev_content[overlap_start..].find('\n') {
                    prev_content[overlap_start + pos + 1..].to_string()
                } else {
                    prev_content[overlap_start..].to_string()
                };
                if !overlap.is_empty() {
                    // Only prepend overlap content, do NOT modify line numbers
                    // start_line/end_line still represent the original chunk's position in the file
                    chunks[i].content = format!("{}{}", overlap, chunks[i].content);
                }
            }
        }
    }

    fn to_chunk_span(&self, full_content: &str, offset: usize, chunk: &str) -> ChunkSpan {
        // Calculate line numbers from byte offset (1-indexed)
        let start_line = full_content[..offset].lines().count() as i32 + 1;
        let chunk_lines = chunk.lines().count() as i32;
        let end_line = start_line + chunk_lines.saturating_sub(1);

        ChunkSpan {
            content: chunk.to_string(),
            start_line,
            end_line: end_line.max(start_line),
        }
    }
}

/// Get tree-sitter Language for supported languages.
///
/// Returns None for unsupported languages, triggering TextSplitter fallback.
/// Currently supports: rust, go, python, java (matching tree-sitter-* dependencies).
fn get_tree_sitter_language(lang: &str) -> Option<Language> {
    match lang {
        "rust" => Some(tree_sitter_rust::LANGUAGE.into()),
        "go" => Some(tree_sitter_go::LANGUAGE.into()),
        "python" => Some(tree_sitter_python::LANGUAGE.into()),
        "java" => Some(tree_sitter_java::LANGUAGE.into()),
        _ => None,
    }
}

/// Languages with CodeSplitter (tree-sitter AST) support.
pub const CODE_SPLITTER_LANGUAGES: &[&str] = &["rust", "go", "python", "java"];

/// Check if a language is supported by CodeSplitter.
pub fn is_code_splitter_supported(lang: &str) -> bool {
    get_tree_sitter_language(lang).is_some()
}

/// Get formatted string of supported languages for logging.
pub fn supported_languages_info() -> String {
    format!(
        "CodeSplitter (AST-aware): {} | Others: TextSplitter fallback",
        CODE_SPLITTER_LANGUAGES.join(", ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chunk_code() {
        let code = r#"
fn main() {
    println!("Hello, world!");
}

fn add(a: i32, b: i32) -> i32 {
    a + b
}
"#;
        let chunker = CodeChunkerService::new(512);
        let chunks = chunker.chunk(code, "rust").expect("chunking failed");

        assert!(!chunks.is_empty());
        // Chunks should cover the entire content
        let total_content: String = chunks.iter().map(|c| c.content.as_str()).collect();
        assert_eq!(total_content.trim(), code.trim());
    }

    #[test]
    fn test_line_numbers() {
        let code = "line1\nline2\nline3\nline4\nline5";
        let chunker = CodeChunkerService::new(1000);
        let chunks = chunker.chunk(code, "text").expect("chunking failed");

        assert_eq!(chunks.len(), 1);
        // Line numbers are 1-indexed to match CodeChunk and IDE conventions
        assert_eq!(chunks[0].start_line, 1);
        assert_eq!(chunks[0].end_line, 5);
    }

    #[test]
    fn test_multiple_chunks() {
        let code = "a".repeat(100) + "\n" + &"b".repeat(100);
        let chunker = CodeChunkerService::new(50);
        let chunks = chunker.chunk(&code, "text").expect("chunking failed");

        assert!(chunks.len() > 1);
    }

    #[test]
    fn test_code_splitter_supported_languages() {
        assert!(is_code_splitter_supported("rust"));
        assert!(is_code_splitter_supported("go"));
        assert!(is_code_splitter_supported("python"));
        assert!(is_code_splitter_supported("java"));
        // Unsupported languages
        assert!(!is_code_splitter_supported("javascript"));
        assert!(!is_code_splitter_supported("typescript"));
        assert!(!is_code_splitter_supported("markdown"));
        assert!(!is_code_splitter_supported("unknown"));
    }

    #[test]
    fn test_code_splitter_rust() {
        // Test that CodeSplitter is used for Rust and produces semantic chunks
        let code = r#"fn hello() {
    println!("Hello");
}

fn world() {
    println!("World");
}

fn long_function() {
    let x = 1;
    let y = 2;
    let z = 3;
    println!("{} {} {}", x, y, z);
}
"#;
        let chunker = CodeChunkerService::new(100);
        let chunks = chunker.chunk(code, "rust").expect("chunking failed");

        // CodeSplitter should produce chunks
        assert!(!chunks.is_empty());
        // Each function should be in the output
        let total: String = chunks.iter().map(|c| c.content.as_str()).collect();
        assert!(total.contains("fn hello()"));
        assert!(total.contains("fn world()"));
        assert!(total.contains("fn long_function()"));
    }

    #[test]
    fn test_code_splitter_python() {
        let code = r#"def hello():
    print("Hello")

def world():
    print("World")

class Greeter:
    def greet(self, name):
        return f"Hello, {name}"
"#;
        let chunker = CodeChunkerService::new(100);
        let chunks = chunker.chunk(code, "python").expect("chunking failed");

        assert!(!chunks.is_empty());
        // Each definition should be in the output
        let total: String = chunks.iter().map(|c| c.content.as_str()).collect();
        assert!(total.contains("def hello()"));
        assert!(total.contains("def world()"));
        assert!(total.contains("class Greeter"));
    }

    #[test]
    fn test_code_splitter_go() {
        let code = r#"package main

func hello() {
    fmt.Println("Hello")
}

func world() {
    fmt.Println("World")
}
"#;
        let chunker = CodeChunkerService::new(100);
        let chunks = chunker.chunk(code, "go").expect("chunking failed");

        assert!(!chunks.is_empty());
        // Each function should be in the output
        let total: String = chunks.iter().map(|c| c.content.as_str()).collect();
        assert!(total.contains("func hello()"));
        assert!(total.contains("func world()"));
    }

    #[test]
    fn test_text_splitter_fallback() {
        // Unsupported language should fall back to TextSplitter
        let code = "const x = 1;\nconst y = 2;\nconst z = 3;";
        let chunker = CodeChunkerService::new(1000);
        let chunks = chunker.chunk(code, "javascript").expect("chunking failed");

        assert!(!chunks.is_empty());
        let total: String = chunks.iter().map(|c| c.content.as_str()).collect();
        assert_eq!(total.trim(), code.trim());
    }

    #[test]
    fn test_chunk_overlap() {
        // Create content that will produce multiple chunks
        let lines: Vec<String> = (1..=20).map(|i| format!("line{i}")).collect();
        let code = lines.join("\n");

        // Without overlap
        let chunker_no_overlap = CodeChunkerService::new(50);
        let chunks_no_overlap = chunker_no_overlap
            .chunk(&code, "text")
            .expect("chunking failed");

        // With overlap (20 chars)
        let chunker_with_overlap = CodeChunkerService::with_overlap(50, 20);
        let chunks_with_overlap = chunker_with_overlap
            .chunk(&code, "text")
            .expect("chunking failed");

        // Both should have multiple chunks
        assert!(
            chunks_no_overlap.len() > 1,
            "Expected multiple chunks without overlap"
        );
        assert!(
            chunks_with_overlap.len() > 1,
            "Expected multiple chunks with overlap"
        );

        // With overlap enabled, subsequent chunks should contain content from previous chunk
        if chunks_with_overlap.len() >= 2 {
            // Check that overlap is applied - second chunk should have extra content
            assert!(
                chunks_with_overlap[1].content.len() >= chunks_no_overlap[1].content.len(),
                "Overlapped chunk should be at least as long as non-overlapped"
            );
        }
    }

    #[test]
    fn test_chunk_overlap_single_chunk() {
        // Small content that fits in one chunk - overlap should not affect it
        let code = "short content";
        let chunker = CodeChunkerService::with_overlap(1000, 50);
        let chunks = chunker.chunk(code, "text").expect("chunking failed");

        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].content.trim(), code.trim());
    }

    #[test]
    fn test_overlap_preserves_line_numbers() {
        // Create content that will produce multiple chunks
        let lines: Vec<String> = (1..=30).map(|i| format!("line{i}")).collect();
        let code = lines.join("\n");

        // Without overlap - get baseline line numbers
        let chunker_no_overlap = CodeChunkerService::new(50);
        let chunks_no_overlap = chunker_no_overlap
            .chunk(&code, "text")
            .expect("chunking failed");

        // With overlap
        let chunker_with_overlap = CodeChunkerService::with_overlap(50, 20);
        let chunks_with_overlap = chunker_with_overlap
            .chunk(&code, "text")
            .expect("chunking failed");

        // Line numbers should be the same regardless of overlap
        // (overlap is for semantic context, not position tracking)
        assert_eq!(chunks_no_overlap.len(), chunks_with_overlap.len());
        for (no_overlap, with_overlap) in chunks_no_overlap.iter().zip(chunks_with_overlap.iter()) {
            assert_eq!(
                no_overlap.start_line, with_overlap.start_line,
                "start_line should not change with overlap"
            );
            assert_eq!(
                no_overlap.end_line, with_overlap.end_line,
                "end_line should not change with overlap"
            );
        }
    }
}
