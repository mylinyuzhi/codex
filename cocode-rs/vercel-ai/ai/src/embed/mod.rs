//! Embedding generation module.
//!
//! This module provides `embed` and `embed_many` functions for generating
//! embeddings from text using embedding models.

#[allow(clippy::module_inception)]
mod embed;
mod embed_result;

pub use embed::EmbedManyOptions;
pub use embed::EmbedOptions;
pub use embed::embed;
pub use embed::embed_many;
pub use embed_result::EmbedManyResult;
pub use embed_result::EmbedResult;
