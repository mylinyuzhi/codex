//! Code chunking module.
//!
//! Uses CodeSplitter (tree-sitter AST-aware) for supported languages,
//! MarkdownChunker for markdown files, with TextSplitter fallback for others.
//! Token validation ensures chunks fit within embedding model limits.

pub mod collapser;
pub mod markdown;
pub mod splitter;
pub mod validation;

pub use collapser::SmartCollapser;
pub use markdown::MarkdownChunker;
pub use markdown::is_markdown_file;
pub use splitter::CODE_SPLITTER_LANGUAGES;
pub use splitter::CodeChunkerService;
pub use splitter::supported_languages_info;
pub use validation::ChunkValidator;
pub use validation::DEFAULT_MAX_CHUNK_TOKENS;
