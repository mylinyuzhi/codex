//! Reranking model module.
//!
//! This module provides reranking model types organized by version.

pub mod v4;

// Re-export v4 types at this level
pub use v4::RerankedDocument;
pub use v4::RerankingModelV4;
pub use v4::RerankingModelV4CallOptions;
pub use v4::RerankingModelV4Result;
