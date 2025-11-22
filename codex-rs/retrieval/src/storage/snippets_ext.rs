//! Extended snippet storage with FTS5 search and symbol query parsing.
//!
//! Provides:
//! - `SymbolQuery` - Parse queries like `type:function name:parse`
//! - FTS5-based full-text search on symbol names, signatures, and docs

use std::sync::Arc;

use crate::error::Result;
use crate::storage::SqliteStore;
use crate::storage::snippets::StoredSnippet;
use crate::tags::TagKind;

/// Parsed symbol query.
///
/// Supports syntax like:
/// - `type:function` - Filter by symbol type
/// - `name:parse` - Filter by name pattern
/// - `file:src/main.rs` or `path:src/main.rs` - Filter by file path
/// - `type:method name:get` - Combined filters
/// - `parse error` - Free-text search in signature/docs
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SymbolQuery {
    /// Symbol name pattern (without wildcards)
    pub name: Option<String>,
    /// Symbol type filter
    pub kind: Option<TagKind>,
    /// File path pattern
    pub filepath: Option<String>,
    /// Free-text search in signature/docs
    pub text: Option<String>,
}

impl SymbolQuery {
    /// Parse a query string.
    ///
    /// Syntax:
    /// - `type:function` - Filter by TagKind
    /// - `name:parse` - Filter by name (wildcards stripped)
    /// - `file:src/main.rs` or `path:src/main.rs` - Filter by filepath
    /// - Remaining terms become free-text search
    ///
    /// # Examples
    ///
    /// ```ignore
    /// let q = SymbolQuery::parse("type:function name:*parse*");
    /// assert_eq!(q.kind, Some(TagKind::Function));
    /// assert_eq!(q.name, Some("parse".to_string()));
    /// ```
    pub fn parse(query: &str) -> Self {
        let mut result = Self::default();
        let mut remaining = Vec::new();

        for part in query.split_whitespace() {
            if let Some(stripped) = part.strip_prefix("type:") {
                result.kind = Some(TagKind::from_syntax_type(stripped));
            } else if let Some(stripped) = part.strip_prefix("name:") {
                // Strip wildcards for SQL LIKE pattern
                result.name = Some(stripped.replace('*', ""));
            } else if let Some(stripped) = part
                .strip_prefix("file:")
                .or_else(|| part.strip_prefix("path:"))
            {
                result.filepath = Some(stripped.to_string());
            } else {
                remaining.push(part);
            }
        }

        if !remaining.is_empty() {
            result.text = Some(remaining.join(" "));
        }

        result
    }

    /// Create a query for a specific file.
    pub fn for_file(filepath: &str) -> Self {
        Self {
            filepath: Some(filepath.to_string()),
            ..Default::default()
        }
    }

    /// Check if this is a symbol-specific query (has type or name filter).
    pub fn is_symbol_query(&self) -> bool {
        self.name.is_some() || self.kind.is_some()
    }

    /// Check if query is empty (no filters or text).
    pub fn is_empty(&self) -> bool {
        self.name.is_none() && self.kind.is_none() && self.text.is_none() && self.filepath.is_none()
    }
}

/// Extended snippet storage with FTS5 search.
pub struct SnippetStorageExt {
    db: Arc<SqliteStore>,
}

impl SnippetStorageExt {
    /// Create a new extended snippet storage.
    pub fn new(db: Arc<SqliteStore>) -> Self {
        Self { db }
    }

    /// Search snippets using FTS5 with optional filters.
    ///
    /// Uses the `snippets_fts` virtual table for full-text search on
    /// name, signature, and docs fields.
    pub async fn search_fts(
        &self,
        workspace: &str,
        query: &SymbolQuery,
        limit: i32,
    ) -> Result<Vec<StoredSnippet>> {
        let ws = workspace.to_string();
        let name_pattern = query.name.clone();
        let kind_filter = query.kind.map(|k| k.as_str().to_string());
        let filepath_filter = query.filepath.clone();
        let text_query = query.text.clone();
        let lim = limit;

        self.db
            .query(move |conn| {
                // Build dynamic SQL with parameters
                let use_fts = text_query.is_some();

                let pq = if use_fts {
                    // Join with FTS5 for text search
                    build_fts_query(
                        &ws,
                        &name_pattern,
                        &kind_filter,
                        &filepath_filter,
                        &text_query,
                        lim,
                    )
                } else {
                    // Simple query without FTS
                    build_simple_query(&ws, &name_pattern, &kind_filter, &filepath_filter, lim)
                };

                let mut stmt = conn.prepare(&pq.sql)?;

                // Convert params to rusqlite parameter references
                let param_refs: Vec<&dyn rusqlite::ToSql> = pq
                    .params
                    .iter()
                    .map(|s| s as &dyn rusqlite::ToSql)
                    .collect();

                let rows = stmt.query_map(param_refs.as_slice(), |row| {
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

    /// Search snippets by name only (simple search).
    pub async fn search_by_name(
        &self,
        workspace: &str,
        name: &str,
        limit: i32,
    ) -> Result<Vec<StoredSnippet>> {
        let query = SymbolQuery {
            name: Some(name.to_string()),
            ..Default::default()
        };
        self.search_fts(workspace, &query, limit).await
    }

    /// Search snippets by type only.
    pub async fn search_by_type(
        &self,
        workspace: &str,
        kind: TagKind,
        limit: i32,
    ) -> Result<Vec<StoredSnippet>> {
        let query = SymbolQuery {
            kind: Some(kind),
            ..Default::default()
        };
        self.search_fts(workspace, &query, limit).await
    }

    /// Search snippets by file path.
    pub async fn search_by_filepath(
        &self,
        workspace: &str,
        filepath: &str,
        limit: i32,
    ) -> Result<Vec<StoredSnippet>> {
        let query = SymbolQuery::for_file(filepath);
        self.search_fts(workspace, &query, limit).await
    }
}

/// Query result with SQL and parameters for parameterized execution.
struct ParameterizedQuery {
    sql: String,
    params: Vec<String>,
}

/// Build a query using FTS5 MATCH for text search.
fn build_fts_query(
    workspace: &str,
    name_pattern: &Option<String>,
    kind_filter: &Option<String>,
    filepath_filter: &Option<String>,
    text_query: &Option<String>,
    limit: i32,
) -> ParameterizedQuery {
    let mut conditions = Vec::new();
    let mut params = Vec::new();
    let mut param_idx = 1;

    // Workspace filter (always present)
    conditions.push(format!("s.workspace = ?{}", param_idx));
    params.push(workspace.to_string());
    param_idx += 1;

    // FTS5 match condition
    if let Some(text) = text_query {
        conditions.push(format!("snippets_fts MATCH ?{}", param_idx));
        // Wrap in quotes for FTS5 phrase search
        params.push(format!("\"{}\"", text.replace('"', "\"\"")));
        param_idx += 1;
    }

    // Name pattern (LIKE)
    if let Some(name) = name_pattern {
        conditions.push(format!("s.name LIKE ?{}", param_idx));
        params.push(format!("%{}%", name));
        param_idx += 1;
    }

    // Type filter
    if let Some(kind) = kind_filter {
        conditions.push(format!("s.syntax_type = ?{}", param_idx));
        params.push(kind.clone());
        param_idx += 1;
    }

    // Filepath filter (exact or LIKE match)
    if let Some(filepath) = filepath_filter {
        // Support both exact match and pattern match
        if filepath.contains('*') || filepath.contains('%') {
            conditions.push(format!("s.filepath LIKE ?{}", param_idx));
            params.push(filepath.replace('*', "%"));
        } else {
            conditions.push(format!("s.filepath = ?{}", param_idx));
            params.push(filepath.clone());
        }
        // param_idx += 1; // not needed, last param
    }

    let sql = format!(
        "SELECT s.id, s.workspace, s.filepath, s.name, s.syntax_type,
                s.start_line, s.end_line, s.signature, s.docs, s.content_hash
         FROM snippets s
         JOIN snippets_fts ON snippets_fts.rowid = s.id
         WHERE {}
         ORDER BY s.name
         LIMIT {}",
        conditions.join(" AND "),
        limit
    );

    ParameterizedQuery { sql, params }
}

/// Build a simple query without FTS5.
fn build_simple_query(
    workspace: &str,
    name_pattern: &Option<String>,
    kind_filter: &Option<String>,
    filepath_filter: &Option<String>,
    limit: i32,
) -> ParameterizedQuery {
    let mut conditions = Vec::new();
    let mut params = Vec::new();
    let mut param_idx = 1;

    // Workspace filter (always present)
    conditions.push(format!("workspace = ?{}", param_idx));
    params.push(workspace.to_string());
    param_idx += 1;

    // Name pattern (LIKE)
    if let Some(name) = name_pattern {
        conditions.push(format!("name LIKE ?{}", param_idx));
        params.push(format!("%{}%", name));
        param_idx += 1;
    }

    // Type filter
    if let Some(kind) = kind_filter {
        conditions.push(format!("syntax_type = ?{}", param_idx));
        params.push(kind.clone());
        param_idx += 1;
    }

    // Filepath filter (exact or LIKE match)
    if let Some(filepath) = filepath_filter {
        if filepath.contains('*') || filepath.contains('%') {
            conditions.push(format!("filepath LIKE ?{}", param_idx));
            params.push(filepath.replace('*', "%"));
        } else {
            conditions.push(format!("filepath = ?{}", param_idx));
            params.push(filepath.clone());
        }
        // param_idx += 1; // not needed, last param
    }

    let sql = format!(
        "SELECT id, workspace, filepath, name, syntax_type,
                start_line, end_line, signature, docs, content_hash
         FROM snippets
         WHERE {}
         ORDER BY name
         LIMIT {}",
        conditions.join(" AND "),
        limit
    );

    ParameterizedQuery { sql, params }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_type_only() {
        let q = SymbolQuery::parse("type:function");
        assert_eq!(q.kind, Some(TagKind::Function));
        assert_eq!(q.name, None);
        assert_eq!(q.text, None);
        assert!(q.is_symbol_query());
    }

    #[test]
    fn test_parse_name_only() {
        let q = SymbolQuery::parse("name:parse");
        assert_eq!(q.kind, None);
        assert_eq!(q.name, Some("parse".to_string()));
        assert_eq!(q.text, None);
        assert!(q.is_symbol_query());
    }

    #[test]
    fn test_parse_name_with_wildcards() {
        let q = SymbolQuery::parse("name:*parse*");
        assert_eq!(q.name, Some("parse".to_string()));
    }

    #[test]
    fn test_parse_combined() {
        let q = SymbolQuery::parse("type:method name:get");
        assert_eq!(q.kind, Some(TagKind::Method));
        assert_eq!(q.name, Some("get".to_string()));
        assert_eq!(q.text, None);
    }

    #[test]
    fn test_parse_with_text() {
        let q = SymbolQuery::parse("type:function parse error");
        assert_eq!(q.kind, Some(TagKind::Function));
        assert_eq!(q.name, None);
        assert_eq!(q.text, Some("parse error".to_string()));
    }

    #[test]
    fn test_parse_text_only() {
        let q = SymbolQuery::parse("parse error handling");
        assert_eq!(q.kind, None);
        assert_eq!(q.name, None);
        assert_eq!(q.text, Some("parse error handling".to_string()));
        assert!(!q.is_symbol_query());
    }

    #[test]
    fn test_parse_empty() {
        let q = SymbolQuery::parse("");
        assert!(q.is_empty());
        assert!(!q.is_symbol_query());
    }

    #[test]
    fn test_parse_filepath() {
        let q = SymbolQuery::parse("file:src/main.rs type:function");
        assert_eq!(q.filepath, Some("src/main.rs".to_string()));
        assert_eq!(q.kind, Some(TagKind::Function));
        assert_eq!(q.name, None);
    }

    #[test]
    fn test_parse_path_alias() {
        // path: should work the same as file:
        let q = SymbolQuery::parse("path:src/main.rs type:function");
        assert_eq!(q.filepath, Some("src/main.rs".to_string()));
        assert_eq!(q.kind, Some(TagKind::Function));
    }

    #[test]
    fn test_for_file() {
        let q = SymbolQuery::for_file("src/lib.rs");
        assert_eq!(q.filepath, Some("src/lib.rs".to_string()));
        assert!(q.name.is_none());
        assert!(q.kind.is_none());
    }

    #[test]
    fn test_build_simple_query_parameterized() {
        let pq = build_simple_query(
            "ws",
            &Some("parse".to_string()),
            &Some("function".to_string()),
            &None,
            10,
        );
        // Check SQL uses placeholders
        assert!(pq.sql.contains("workspace = ?1"));
        assert!(pq.sql.contains("name LIKE ?2"));
        assert!(pq.sql.contains("syntax_type = ?3"));
        assert!(pq.sql.contains("LIMIT 10"));
        // Check params
        assert_eq!(pq.params.len(), 3);
        assert_eq!(pq.params[0], "ws");
        assert_eq!(pq.params[1], "%parse%");
        assert_eq!(pq.params[2], "function");
    }

    #[test]
    fn test_build_simple_query_with_filepath() {
        let pq = build_simple_query("ws", &None, &None, &Some("src/main.rs".to_string()), 10);
        assert!(pq.sql.contains("workspace = ?1"));
        assert!(pq.sql.contains("filepath = ?2"));
        assert_eq!(pq.params.len(), 2);
        assert_eq!(pq.params[0], "ws");
        assert_eq!(pq.params[1], "src/main.rs");
    }

    #[test]
    fn test_build_simple_query_with_filepath_pattern() {
        let pq = build_simple_query("ws", &None, &None, &Some("src/*.rs".to_string()), 10);
        assert!(pq.sql.contains("filepath LIKE ?2"));
        assert_eq!(pq.params[1], "src/%.rs");
    }

    #[test]
    fn test_build_fts_query_parameterized() {
        let pq = build_fts_query(
            "ws",
            &None,
            &Some("function".to_string()),
            &None,
            &Some("error handling".to_string()),
            20,
        );
        // Check SQL uses placeholders
        assert!(pq.sql.contains("s.workspace = ?1"));
        assert!(pq.sql.contains("snippets_fts MATCH ?2"));
        assert!(pq.sql.contains("s.syntax_type = ?3"));
        assert!(pq.sql.contains("LIMIT 20"));
        // Check params
        assert_eq!(pq.params.len(), 3);
        assert_eq!(pq.params[0], "ws");
        assert_eq!(pq.params[1], "\"error handling\""); // FTS5 phrase search
        assert_eq!(pq.params[2], "function");
    }

    #[test]
    fn test_fts_escapes_quotes() {
        let pq = build_fts_query(
            "ws",
            &None,
            &None,
            &None,
            &Some("test \"quoted\" value".to_string()),
            10,
        );
        // Quotes should be escaped for FTS5
        assert_eq!(pq.params[1], "\"test \"\"quoted\"\" value\"");
    }

    #[test]
    fn test_build_fts_query_with_filepath() {
        let pq = build_fts_query(
            "ws",
            &None,
            &None,
            &Some("src/lib.rs".to_string()),
            &Some("parse".to_string()),
            10,
        );
        assert!(pq.sql.contains("snippets_fts MATCH ?2"));
        assert!(pq.sql.contains("s.filepath = ?3"));
        assert_eq!(pq.params[2], "src/lib.rs");
    }
}
