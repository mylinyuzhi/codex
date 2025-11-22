//! Reranking module for search results.
//!
//! Provides post-retrieval reranking to improve search relevance.
//! Currently implements rule-based reranking; model-based reranking
//! (e.g., HuggingFace TEI) can be added later.

pub mod rule_based;

pub use rule_based::RuleBasedReranker;
pub use rule_based::RuleBasedRerankerConfig;

use crate::types::SearchResult;

/// Reranker trait for post-retrieval score adjustment.
///
/// Implementations can use rules, models, or hybrid approaches
/// to reorder search results for better relevance.
pub trait Reranker: Send + Sync {
    /// Rerank search results based on query context.
    ///
    /// Modifies scores in place and re-sorts the results.
    fn rerank(&self, query: &str, results: &mut [SearchResult]);
}
