//! SQLite storage layer.
//!
//! Provides async-safe SQLite operations using spawn_blocking.

use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;

use rusqlite::Connection;
use tokio::task::spawn_blocking;

use crate::error::Result;
use crate::error::RetrievalErr;

/// Async-safe SQLite store.
///
/// rusqlite::Connection is not Send + Sync, so we wrap it in Arc<Mutex<>>.
pub struct SqliteStore {
    conn: Arc<Mutex<Connection>>,
    path: PathBuf,
}

impl SqliteStore {
    /// Open or create a SQLite database.
    pub fn open(path: &Path) -> Result<Self> {
        let path_buf = path.to_path_buf();
        let conn = Connection::open(path).map_err(|e| RetrievalErr::SqliteError {
            path: path_buf.clone(),
            cause: e.to_string(),
        })?;
        Self::init_schema(&conn, &path_buf)?;

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            path: path_buf,
        })
    }

    /// Get the database path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    fn init_schema(conn: &Connection, path: &PathBuf) -> Result<()> {
        conn.execute_batch(SCHEMA)
            .map_err(|e| RetrievalErr::SqliteError {
                path: path.clone(),
                cause: format!("schema init failed: {e}"),
            })?;
        Ok(())
    }

    /// Execute a query asynchronously.
    pub async fn query<F, T>(&self, f: F) -> Result<T>
    where
        F: FnOnce(&Connection) -> Result<T> + Send + 'static,
        T: Send + 'static,
    {
        let conn = self.conn.clone();
        let path = self.path.clone();

        spawn_blocking(move || {
            // Recover from mutex poisoning with state validation
            let guard = conn.lock().unwrap_or_else(|poisoned| {
                tracing::warn!(path = %path.display(), "Mutex poisoned, recovering");
                let inner = poisoned.into_inner();

                // Validate connection state after poisoning recovery
                // If not in autocommit mode, an interrupted transaction may be in progress
                if !inner.is_autocommit() {
                    tracing::warn!(
                        path = %path.display(),
                        "Connection not in autocommit after poisoning recovery, attempting rollback"
                    );
                    // Attempt to rollback any pending transaction and log result
                    match inner.execute("ROLLBACK", []) {
                        Ok(_) => {
                            tracing::info!(
                                path = %path.display(),
                                "Successfully rolled back pending transaction after mutex poisoning"
                            );
                        }
                        Err(e) => {
                            tracing::error!(
                                path = %path.display(),
                                error = %e,
                                "ROLLBACK failed after mutex poisoning recovery - connection may be in inconsistent state"
                            );
                        }
                    }
                }
                inner
            });
            f(&guard)
        })
        .await
        .map_err(|e| RetrievalErr::SqliteError {
            path: self.path.clone(),
            cause: format!("spawn_blocking failed: {e}"),
        })?
    }

    /// Execute a transaction asynchronously.
    pub async fn transaction<F, T>(&self, f: F) -> Result<T>
    where
        F: FnOnce(&Connection) -> Result<T> + Send + 'static,
        T: Send + 'static,
    {
        let conn = self.conn.clone();
        let path = self.path.clone();

        spawn_blocking(move || {
            // Recover from mutex poisoning with state validation
            let mut guard = conn.lock().unwrap_or_else(|poisoned| {
                tracing::warn!(path = %path.display(), "Mutex poisoned in transaction, recovering");
                let inner = poisoned.into_inner();

                // Validate connection state after poisoning recovery
                if !inner.is_autocommit() {
                    tracing::warn!(
                        path = %path.display(),
                        "Connection not in autocommit after poisoning recovery, attempting rollback"
                    );
                    // Attempt to rollback any pending transaction and log result
                    match inner.execute("ROLLBACK", []) {
                        Ok(_) => {
                            tracing::info!(
                                path = %path.display(),
                                "Successfully rolled back pending transaction after mutex poisoning"
                            );
                        }
                        Err(e) => {
                            tracing::error!(
                                path = %path.display(),
                                error = %e,
                                "ROLLBACK failed after mutex poisoning recovery - connection may be in inconsistent state"
                            );
                        }
                    }
                }
                inner
            });

            let tx = guard.transaction().map_err(|e| RetrievalErr::SqliteError {
                path: path.clone(),
                cause: format!("transaction start failed: {e}"),
            })?;
            let result = f(&tx)?;
            tx.commit().map_err(|e| RetrievalErr::SqliteError {
                path: path.clone(),
                cause: format!("transaction commit failed: {e}"),
            })?;
            Ok(result)
        })
        .await
        .map_err(|e| RetrievalErr::SqliteError {
            path: self.path.clone(),
            cause: format!("spawn_blocking failed: {e}"),
        })?
    }
}

/// SQLite schema for retrieval metadata.
///
/// Simplified schema without branch tracking - tweakcc updates based on
/// file content changes only.
const SCHEMA: &str = r#"
-- Schema version tracking
CREATE TABLE IF NOT EXISTS schema_version (
    version INTEGER PRIMARY KEY,
    applied_at INTEGER NOT NULL
);
INSERT OR IGNORE INTO schema_version (version, applied_at) VALUES (2, strftime('%s', 'now'));

-- Index catalog (tweakcc update tracking)
-- Simplified: no branch column, unique by (workspace, filepath)
CREATE TABLE IF NOT EXISTS catalog (
    id INTEGER PRIMARY KEY,
    workspace TEXT NOT NULL,
    filepath TEXT NOT NULL,
    content_hash TEXT NOT NULL,
    mtime INTEGER NOT NULL,
    indexed_at INTEGER NOT NULL,
    chunks_count INTEGER DEFAULT 0,
    chunks_failed INTEGER DEFAULT 0,
    UNIQUE(workspace, filepath)
);

CREATE INDEX IF NOT EXISTS idx_catalog_workspace ON catalog(workspace);
CREATE INDEX IF NOT EXISTS idx_catalog_hash ON catalog(content_hash);
CREATE INDEX IF NOT EXISTS idx_catalog_filepath ON catalog(filepath);

-- Code snippets (tree-sitter-tags extracted symbols)
CREATE TABLE IF NOT EXISTS snippets (
    id INTEGER PRIMARY KEY,
    workspace TEXT NOT NULL,
    filepath TEXT NOT NULL,
    name TEXT NOT NULL,
    syntax_type TEXT NOT NULL,
    start_line INTEGER NOT NULL,
    end_line INTEGER NOT NULL,
    signature TEXT,
    docs TEXT,
    content_hash TEXT NOT NULL,
    UNIQUE(workspace, filepath, name, start_line)
);

CREATE INDEX IF NOT EXISTS idx_snippets_workspace ON snippets(workspace);
CREATE INDEX IF NOT EXISTS idx_snippets_name ON snippets(name);
CREATE INDEX IF NOT EXISTS idx_snippets_type ON snippets(syntax_type);

-- Index lock (multi-process coordination)
CREATE TABLE IF NOT EXISTS index_lock (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    holder_id TEXT NOT NULL,
    workspace TEXT NOT NULL,
    locked_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL
);

-- Checkpoint for resume (optional)
CREATE TABLE IF NOT EXISTS checkpoint (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    workspace TEXT NOT NULL,
    phase TEXT NOT NULL,
    total_files INTEGER NOT NULL,
    processed_files INTEGER NOT NULL,
    last_file TEXT,
    started_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

-- FTS5 virtual table for fast symbol search
CREATE VIRTUAL TABLE IF NOT EXISTS snippets_fts USING fts5(
    name,
    signature,
    docs,
    content=snippets,
    content_rowid=id
);

-- Triggers to keep FTS5 in sync with snippets table
CREATE TRIGGER IF NOT EXISTS snippets_ai AFTER INSERT ON snippets BEGIN
    INSERT INTO snippets_fts(rowid, name, signature, docs)
    VALUES (new.id, new.name, new.signature, new.docs);
END;

CREATE TRIGGER IF NOT EXISTS snippets_ad AFTER DELETE ON snippets BEGIN
    INSERT INTO snippets_fts(snippets_fts, rowid, name, signature, docs)
    VALUES ('delete', old.id, old.name, old.signature, old.docs);
END;

CREATE TRIGGER IF NOT EXISTS snippets_au AFTER UPDATE ON snippets BEGIN
    INSERT INTO snippets_fts(snippets_fts, rowid, name, signature, docs)
    VALUES ('delete', old.id, old.name, old.signature, old.docs);
    INSERT INTO snippets_fts(rowid, name, signature, docs)
    VALUES (new.id, new.name, new.signature, new.docs);
END;

-- Repo map tag cache (definitions and references for PageRank graph)
CREATE TABLE IF NOT EXISTS repomap_tags (
    id INTEGER PRIMARY KEY,
    workspace TEXT NOT NULL,
    filepath TEXT NOT NULL,
    mtime INTEGER NOT NULL,
    name TEXT NOT NULL,
    kind TEXT NOT NULL,  -- 'def' or 'ref'
    line INTEGER NOT NULL,
    UNIQUE(workspace, filepath, name, kind, line)
);

CREATE INDEX IF NOT EXISTS idx_repomap_tags_file ON repomap_tags(workspace, filepath);
CREATE INDEX IF NOT EXISTS idx_repomap_tags_name ON repomap_tags(name);
CREATE INDEX IF NOT EXISTS idx_repomap_tags_kind ON repomap_tags(kind);
"#;

/// Extension trait for optional query results.
pub trait OptionalExt<T> {
    /// Convert QueryReturnedNoRows to None. Loses path context on other errors.
    fn optional(self) -> Result<Option<T>>;

    /// Convert QueryReturnedNoRows to None with path context for other errors.
    fn optional_with_path(self, path: &Path) -> Result<Option<T>>;
}

impl<T> OptionalExt<T> for std::result::Result<T, rusqlite::Error> {
    fn optional(self) -> Result<Option<T>> {
        match self {
            Ok(v) => Ok(Some(v)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    fn optional_with_path(self, path: &Path) -> Result<Option<T>> {
        match self {
            Ok(v) => Ok(Some(v)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(RetrievalErr::sqlite_error(path, e)),
        }
    }
}
