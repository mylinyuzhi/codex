//! Storage module.
//!
//! Provides SQLite and LanceDB storage backends.
//!
//! LanceDB is used for vector storage and search with extended metadata.
//! SQLite is used for lock/checkpoint management and query caching.

pub mod lancedb;
pub mod snippets;
pub mod snippets_ext;
pub mod sqlite;

pub use lancedb::FileMetadata;
pub use lancedb::LanceDbStore;
pub use snippets::SnippetStorage;
pub use snippets::StoredSnippet;
pub use snippets_ext::SnippetStorageExt;
pub use snippets_ext::SymbolQuery;
pub use sqlite::SqliteStore;
