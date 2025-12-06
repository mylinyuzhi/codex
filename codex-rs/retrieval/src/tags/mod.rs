//! Tag extraction module.
//!
//! Uses tree-sitter-tags to extract function, class, and method definitions.

pub mod extractor;
pub mod languages;

pub use extractor::CodeTag;
pub use extractor::TagExtractor;
pub use extractor::TagKind;
pub use languages::SupportedLanguage;
