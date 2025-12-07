//! Snippet storage for extracted tags.
//!
//! Stores and retrieves code snippets (symbols) in SQLite.

use std::sync::Arc;

use crate::error::Result;
use crate::storage::SqliteStore;
use crate::tags::CodeTag;
use crate::tags::TagKind;

/// Stored code snippet (symbol).
#[derive(Debug, Clone)]
pub struct StoredSnippet {
    /// Database ID
    pub id: i64,
    /// Workspace identifier
    pub workspace: String,
    /// File path
    pub filepath: String,
    /// Symbol name
    pub name: String,
    /// Syntax type (function, class, etc.)
    pub syntax_type: String,
    /// Start line (0-indexed)
    pub start_line: i32,
    /// End line (0-indexed)
    pub end_line: i32,
    /// Optional signature
    pub signature: Option<String>,
    /// Optional documentation
    pub docs: Option<String>,
    /// Content hash for deduplication
    pub content_hash: String,
}

/// Snippet storage service.
pub struct SnippetStorage {
    db: Arc<SqliteStore>,
}

impl SnippetStorage {
    /// Create a new snippet storage service.
    pub fn new(db: Arc<SqliteStore>) -> Self {
        Self { db }
    }

    /// Store a batch of tags as snippets.
    pub async fn store_tags(
        &self,
        workspace: &str,
        filepath: &str,
        tags: &[CodeTag],
        content_hash: &str,
    ) -> Result<i32> {
        let ws = workspace.to_string();
        let fp = filepath.to_string();
        let hash = content_hash.to_string();

        // Prepare data for insertion
        let snippets: Vec<_> = tags
            .iter()
            .map(|tag| {
                (
                    tag.name.clone(),
                    tag.kind.as_str().to_string(),
                    tag.start_line,
                    tag.end_line,
                    tag.signature.clone(),
                    tag.docs.clone(),
                )
            })
            .collect();

        self.db
            .transaction(move |conn| {
                // Delete existing snippets for this file
                conn.execute(
                    "DELETE FROM snippets WHERE workspace = ? AND filepath = ?",
                    rusqlite::params![ws, fp],
                )?;

                let mut count = 0;
                for (name, syntax_type, start_line, end_line, signature, docs) in snippets {
                    conn.execute(
                        "INSERT INTO snippets (workspace, filepath, name, syntax_type, start_line, end_line, signature, docs, content_hash)
                         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
                        rusqlite::params![ws, fp, name, syntax_type, start_line, end_line, signature, docs, hash],
                    )?;
                    count += 1;
                }

                Ok(count)
            })
            .await
    }

    /// Search snippets by symbol name.
    pub async fn search_by_name(
        &self,
        workspace: &str,
        query: &str,
        limit: i32,
    ) -> Result<Vec<StoredSnippet>> {
        let ws = workspace.to_string();
        let q = format!("%{query}%");
        let lim = limit;

        self.db
            .query(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, workspace, filepath, name, syntax_type, start_line, end_line, signature, docs, content_hash
                     FROM snippets
                     WHERE workspace = ? AND name LIKE ?
                     ORDER BY name
                     LIMIT ?",
                )?;

                let rows = stmt.query_map(rusqlite::params![ws, q, lim], |row| {
                    Ok(StoredSnippet {
                        id: row.get(0)?,
                        workspace: row.get(1)?,
                        filepath: row.get(2)?,
                        name: row.get(3)?,
                        syntax_type: row.get(4)?,
                        start_line: row.get(5)?,
                        end_line: row.get(6)?,
                        signature: row.get(7)?,
                        docs: row.get(8)?,
                        content_hash: row.get(9)?,
                    })
                })?;

                let mut results = Vec::new();
                for row in rows {
                    results.push(row?);
                }
                Ok(results)
            })
            .await
    }

    /// Search snippets by syntax type.
    pub async fn search_by_type(
        &self,
        workspace: &str,
        syntax_type: TagKind,
        limit: i32,
    ) -> Result<Vec<StoredSnippet>> {
        let ws = workspace.to_string();
        let st = syntax_type.as_str().to_string();
        let lim = limit;

        self.db
            .query(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, workspace, filepath, name, syntax_type, start_line, end_line, signature, docs, content_hash
                     FROM snippets
                     WHERE workspace = ? AND syntax_type = ?
                     ORDER BY name
                     LIMIT ?",
                )?;

                let rows = stmt.query_map(rusqlite::params![ws, st, lim], |row| {
                    Ok(StoredSnippet {
                        id: row.get(0)?,
                        workspace: row.get(1)?,
                        filepath: row.get(2)?,
                        name: row.get(3)?,
                        syntax_type: row.get(4)?,
                        start_line: row.get(5)?,
                        end_line: row.get(6)?,
                        signature: row.get(7)?,
                        docs: row.get(8)?,
                        content_hash: row.get(9)?,
                    })
                })?;

                let mut results = Vec::new();
                for row in rows {
                    results.push(row?);
                }
                Ok(results)
            })
            .await
    }

    /// Get snippets for a specific file.
    pub async fn get_by_filepath(
        &self,
        workspace: &str,
        filepath: &str,
    ) -> Result<Vec<StoredSnippet>> {
        let ws = workspace.to_string();
        let fp = filepath.to_string();

        self.db
            .query(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, workspace, filepath, name, syntax_type, start_line, end_line, signature, docs, content_hash
                     FROM snippets
                     WHERE workspace = ? AND filepath = ?
                     ORDER BY start_line",
                )?;

                let rows = stmt.query_map(rusqlite::params![ws, fp], |row| {
                    Ok(StoredSnippet {
                        id: row.get(0)?,
                        workspace: row.get(1)?,
                        filepath: row.get(2)?,
                        name: row.get(3)?,
                        syntax_type: row.get(4)?,
                        start_line: row.get(5)?,
                        end_line: row.get(6)?,
                        signature: row.get(7)?,
                        docs: row.get(8)?,
                        content_hash: row.get(9)?,
                    })
                })?;

                let mut results = Vec::new();
                for row in rows {
                    results.push(row?);
                }
                Ok(results)
            })
            .await
    }

    /// Delete snippets for a file.
    pub async fn delete_by_filepath(&self, workspace: &str, filepath: &str) -> Result<i32> {
        let ws = workspace.to_string();
        let fp = filepath.to_string();

        self.db
            .query(move |conn| {
                let count = conn.execute(
                    "DELETE FROM snippets WHERE workspace = ? AND filepath = ?",
                    rusqlite::params![ws, fp],
                )?;
                Ok(count as i32)
            })
            .await
    }

    /// Delete all snippets for a workspace.
    pub async fn delete_by_workspace(&self, workspace: &str) -> Result<i32> {
        let ws = workspace.to_string();

        self.db
            .query(move |conn| {
                let count = conn.execute(
                    "DELETE FROM snippets WHERE workspace = ?",
                    rusqlite::params![ws],
                )?;
                Ok(count as i32)
            })
            .await
    }

    /// Count total snippets in workspace.
    pub async fn count(&self, workspace: &str) -> Result<i64> {
        let ws = workspace.to_string();

        self.db
            .query(move |conn| {
                let count: i64 = conn.query_row(
                    "SELECT COUNT(*) FROM snippets WHERE workspace = ?",
                    [&ws],
                    |row| row.get(0),
                )?;
                Ok(count)
            })
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    struct TestContext {
        _dir: TempDir,
        snippets: SnippetStorage,
    }

    fn setup() -> TestContext {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("test.db");
        let store = Arc::new(SqliteStore::open(&db_path).unwrap());
        let snippets = SnippetStorage::new(store);
        TestContext {
            _dir: dir,
            snippets,
        }
    }

    #[tokio::test]
    async fn test_store_and_retrieve_tags() {
        let ctx = setup();
        let snippets = &ctx.snippets;

        let tags = vec![
            CodeTag {
                name: "main".to_string(),
                kind: TagKind::Function,
                start_line: 0,
                end_line: 5,
                start_byte: 0,
                end_byte: 100,
                signature: Some("fn main()".to_string()),
                docs: Some("Entry point".to_string()),
                is_definition: true,
            },
            CodeTag {
                name: "Point".to_string(),
                kind: TagKind::Struct,
                start_line: 10,
                end_line: 15,
                start_byte: 200,
                end_byte: 300,
                signature: None,
                docs: None,
                is_definition: true,
            },
        ];

        // Store tags
        let count = snippets
            .store_tags("test_ws", "src/main.rs", &tags, "abc123")
            .await
            .unwrap();
        assert_eq!(count, 2);

        // Retrieve by name
        let results = snippets
            .search_by_name("test_ws", "main", 10)
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "main");
        assert_eq!(results[0].syntax_type, "function");

        // Retrieve by type
        let structs = snippets
            .search_by_type("test_ws", TagKind::Struct, 10)
            .await
            .unwrap();
        assert_eq!(structs.len(), 1);
        assert_eq!(structs[0].name, "Point");

        // Count
        let total = snippets.count("test_ws").await.unwrap();
        assert_eq!(total, 2);
    }

    #[tokio::test]
    async fn test_delete_snippets() {
        let ctx = setup();
        let snippets = &ctx.snippets;

        let tags = vec![CodeTag {
            name: "foo".to_string(),
            kind: TagKind::Function,
            start_line: 0,
            end_line: 5,
            start_byte: 0,
            end_byte: 100,
            signature: None,
            docs: None,
            is_definition: true,
        }];

        snippets
            .store_tags("ws", "file.rs", &tags, "hash")
            .await
            .unwrap();

        let deleted = snippets.delete_by_filepath("ws", "file.rs").await.unwrap();
        assert_eq!(deleted, 1);

        let count = snippets.count("ws").await.unwrap();
        assert_eq!(count, 0);
    }
}
