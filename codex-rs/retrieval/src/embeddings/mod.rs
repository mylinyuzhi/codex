//! Embedding providers for vector search.
//!
//! Provides implementations of the `EmbeddingProvider` trait for various
//! embedding services.

pub mod cache;
pub mod openai;
pub mod queue;

pub use cache::EmbeddingCache;
pub use openai::OpenAIEmbeddings;
pub use queue::EmbeddingQueue;
