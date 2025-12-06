//! Index manager for batch indexing operations.
//!
//! Coordinates file walking, change detection, and incremental updates.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

use tokio::sync::mpsc;

/// Rebuild mode for indexing operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RebuildMode {
    /// Incremental: only process changed files (default)
    #[default]
    Incremental,
    /// Clean: delete all index data, then rebuild from scratch
    Clean,
}

use crate::chunking::CodeChunkerService;
use crate::chunking::SmartCollapser;
use crate::config::RetrievalConfig;
use crate::error::Result;
use crate::indexing::IndexLockGuard;
use crate::indexing::change_detector::ChangeDetector;
use crate::indexing::change_detector::ChangeStatus;
use crate::indexing::change_detector::get_mtime;
use crate::indexing::change_detector::hash_file;
use crate::indexing::progress::IndexProgress;
use crate::indexing::walker::FileWalker;
use crate::storage::SnippetStorage;
use crate::storage::SqliteStore;
use crate::tags::SupportedLanguage;
use crate::tags::TagExtractor;
use crate::types::detect_language;

/// Index manager for coordinating indexing operations.
#[allow(dead_code)]
pub struct IndexManager {
    config: RetrievalConfig,
    db: Arc<SqliteStore>,
    change_detector: ChangeDetector,
    snippet_storage: SnippetStorage,
    chunker: CodeChunkerService,
}

impl IndexManager {
    /// Create a new index manager.
    pub fn new(config: RetrievalConfig, db: Arc<SqliteStore>) -> Self {
        let change_detector = ChangeDetector::new(db.clone());
        let snippet_storage = SnippetStorage::new(db.clone());
        let chunker = CodeChunkerService::with_overlap(
            config.chunking.max_chunk_size as usize,
            config.chunking.chunk_overlap as usize,
        );

        Self {
            config,
            db,
            change_detector,
            snippet_storage,
            chunker,
        }
    }

    /// Index a workspace directory.
    ///
    /// Returns a stream of progress updates.
    pub async fn index_workspace(
        &mut self,
        workspace: &str,
        root: &Path,
        branch: Option<&str>,
    ) -> Result<mpsc::Receiver<IndexProgress>> {
        let (tx, rx) = mpsc::channel(100);

        // Acquire lock
        let lock = IndexLockGuard::try_acquire(
            self.db.clone(),
            workspace,
            std::time::Duration::from_secs(self.config.indexing.lock_timeout_secs as u64),
        )
        .await?;

        // Clone what we need for the async task
        let workspace = workspace.to_string();
        let root = root.to_path_buf();
        let branch = branch.map(|s| s.to_string());
        let config = self.config.clone();
        let change_detector = ChangeDetector::new(self.db.clone());
        let snippet_storage = SnippetStorage::new(self.db.clone());
        let chunker = CodeChunkerService::with_overlap(
            config.chunking.max_chunk_size as usize,
            config.chunking.chunk_overlap as usize,
        );

        tokio::spawn(async move {
            let result = Self::run_indexing(
                &workspace,
                &root,
                branch.as_deref(),
                &config,
                &change_detector,
                &snippet_storage,
                &chunker,
                &lock,
                tx.clone(),
            )
            .await;

            if let Err(e) = result {
                let _ = tx
                    .send(IndexProgress::failed(format!("Indexing failed: {e}")))
                    .await;
            }
        });

        Ok(rx)
    }

    /// Run the indexing process.
    async fn run_indexing(
        workspace: &str,
        root: &Path,
        branch: Option<&str>,
        config: &RetrievalConfig,
        change_detector: &ChangeDetector,
        snippet_storage: &SnippetStorage,
        chunker: &CodeChunkerService,
        lock: &IndexLockGuard,
        tx: mpsc::Sender<IndexProgress>,
    ) -> Result<()> {
        // Phase 1: Walk files
        let _ = tx.send(IndexProgress::loading("Scanning files...")).await;

        let walker = FileWalker::new(config.indexing.max_file_size_mb);
        let files = walker.walk(root)?;
        let total_files = files.len();

        let _ = tx
            .send(IndexProgress::indexing(
                0.0,
                format!("Found {total_files} files"),
            ))
            .await;

        // Phase 2: Compute hashes for all files
        let _ = tx
            .send(IndexProgress::indexing(0.05, "Computing file hashes..."))
            .await;

        let mut current_files = HashMap::new();
        for file in &files {
            if let Ok(hash) = hash_file(file) {
                let rel_path = file
                    .strip_prefix(root)
                    .unwrap_or(file)
                    .to_string_lossy()
                    .to_string();
                current_files.insert(rel_path, hash);
            }
        }

        // Phase 3: Detect changes
        let _ = tx
            .send(IndexProgress::indexing(0.1, "Detecting changes..."))
            .await;

        let changes = change_detector
            .detect_changes(workspace, branch, &current_files)
            .await?;

        let added = changes
            .iter()
            .filter(|c| c.status == ChangeStatus::Added)
            .count();
        let modified = changes
            .iter()
            .filter(|c| c.status == ChangeStatus::Modified)
            .count();
        let deleted = changes
            .iter()
            .filter(|c| c.status == ChangeStatus::Deleted)
            .count();

        let _ = tx
            .send(IndexProgress::indexing(
                0.15,
                format!("Changes: {added} added, {modified} modified, {deleted} deleted"),
            ))
            .await;

        // Phase 4: Process changes in batches
        let batch_size = config.indexing.batch_size as usize;
        let files_to_process: Vec<_> = changes
            .iter()
            .filter(|c| c.status != ChangeStatus::Deleted)
            .collect();
        let total_to_process = files_to_process.len();

        let mut tag_extractor = TagExtractor::new();
        let mut processed = 0;
        let mut failed_files: Vec<String> = Vec::new();

        // Time-based lock refresh (every 15 seconds, lock timeout is 30 seconds)
        let mut last_refresh = Instant::now();
        const REFRESH_INTERVAL: Duration = Duration::from_secs(15);

        for batch in files_to_process.chunks(batch_size) {
            // Refresh lock based on time, not file count
            if last_refresh.elapsed() > REFRESH_INTERVAL {
                lock.refresh().await?;
                last_refresh = Instant::now();
            }

            for change in batch {
                let file_path = root.join(&change.filepath);

                // Read file content with proper error handling
                let content = match std::fs::read_to_string(&file_path) {
                    Ok(c) => c,
                    Err(e) => {
                        tracing::warn!(
                            filepath = %change.filepath,
                            error = %e,
                            "Failed to read file during indexing, removing from catalog"
                        );
                        failed_files.push(change.filepath.clone());
                        // Remove from catalog since file is not accessible
                        // This prevents orphaned entries from accumulating
                        let _ = change_detector
                            .remove_from_catalog(workspace, branch, &change.filepath)
                            .await;
                        continue;
                    }
                };

                // Extract tags if supported language
                if let Some(lang) = SupportedLanguage::from_path(&file_path) {
                    if let Ok(tags) = tag_extractor.extract(&content, lang) {
                        let hash = change.content_hash.as_deref().unwrap_or("");
                        let _ = snippet_storage
                            .store_tags(workspace, &change.filepath, &tags, hash)
                            .await;
                    }
                }

                // Update catalog
                let mtime = get_mtime(&file_path).unwrap_or(0);
                let language = detect_language(&file_path).unwrap_or_default();
                let chunks = chunker.chunk(&content, &language).unwrap_or_default();

                // Apply smart collapsing if enabled (reduces large chunks by collapsing nested blocks)
                let chunks = if config.chunking.enable_smart_collapse {
                    let collapser = SmartCollapser::new(config.chunking.max_chunk_size as usize);
                    collapser.collapse_all(&chunks)
                } else {
                    chunks
                };

                change_detector
                    .update_catalog(
                        workspace,
                        branch,
                        &change.filepath,
                        change.content_hash.as_deref().unwrap_or(""),
                        mtime,
                        chunks.len() as i32,
                        0,
                    )
                    .await?;

                processed += 1;
            }

            // Report progress
            let progress = 0.15 + (0.8 * processed as f32 / total_to_process.max(1) as f32);
            let _ = tx
                .send(IndexProgress::indexing(
                    progress,
                    format!("Indexed {processed}/{total_to_process} files"),
                ))
                .await;
        }

        // Phase 5: Handle deletions
        for change in changes.iter().filter(|c| c.status == ChangeStatus::Deleted) {
            change_detector
                .remove_from_catalog(workspace, branch, &change.filepath)
                .await?;
            snippet_storage
                .delete_by_filepath(workspace, &change.filepath)
                .await?;
        }

        // Report failed files if any
        if !failed_files.is_empty() {
            tracing::warn!(
                count = failed_files.len(),
                "Some files could not be indexed due to read errors"
            );
        }

        let status_msg = if failed_files.is_empty() {
            format!(
                "Indexed {processed} files ({added} added, {modified} modified, {deleted} deleted)"
            )
        } else {
            format!(
                "Indexed {processed} files ({added} added, {modified} modified, {deleted} deleted, {} failed)",
                failed_files.len()
            )
        };

        let _ = tx.send(IndexProgress::done(status_msg)).await;

        Ok(())
    }

    /// Rebuild the index with the specified mode.
    ///
    /// - `Incremental`: Only process changed files (default behavior)
    /// - `Clean`: Delete all index data and rebuild from scratch
    pub async fn rebuild(
        &mut self,
        workspace: &str,
        root: &Path,
        branch: Option<&str>,
        mode: RebuildMode,
    ) -> Result<mpsc::Receiver<IndexProgress>> {
        if mode == RebuildMode::Clean {
            self.clean(workspace).await?;
        }
        self.index_workspace(workspace, root, branch).await
    }

    /// Clean all index data for a workspace.
    ///
    /// Deletes all catalog entries and snippet data for the workspace.
    pub async fn clean(&mut self, workspace: &str) -> Result<()> {
        let ws = workspace.to_string();

        // Delete from catalog
        self.db
            .query(move |conn| {
                conn.execute("DELETE FROM catalog WHERE workspace = ?", [&ws])?;
                Ok(())
            })
            .await?;

        // Delete snippets
        self.snippet_storage.delete_by_workspace(workspace).await?;

        tracing::info!(workspace = workspace, "Cleaned all index data");
        Ok(())
    }

    /// Get index statistics for a workspace.
    pub async fn get_stats(&self, workspace: &str) -> Result<IndexStats> {
        let ws = workspace.to_string();

        let (file_count, chunk_count, last_indexed) = self
            .db
            .query(move |conn| {
                let file_count: i64 = conn
                    .query_row(
                        "SELECT COUNT(*) FROM catalog WHERE workspace = ?",
                        [&ws],
                        |row| row.get(0),
                    )
                    .unwrap_or(0);

                let chunk_count: i64 = conn
                    .query_row(
                        "SELECT COALESCE(SUM(chunks_count), 0) FROM catalog WHERE workspace = ?",
                        [&ws],
                        |row| row.get(0),
                    )
                    .unwrap_or(0);

                let last_indexed: Option<i64> = conn
                    .query_row(
                        "SELECT MAX(indexed_at) FROM catalog WHERE workspace = ?",
                        [&ws],
                        |row| row.get(0),
                    )
                    .ok()
                    .flatten();

                Ok((file_count, chunk_count, last_indexed))
            })
            .await?;

        Ok(IndexStats {
            file_count,
            chunk_count,
            last_indexed,
        })
    }
}

/// Index statistics for a workspace.
#[derive(Debug, Clone, Default)]
pub struct IndexStats {
    /// Number of indexed files
    pub file_count: i64,
    /// Total number of chunks
    pub chunk_count: i64,
    /// Unix timestamp of last indexing operation
    pub last_indexed: Option<i64>,
}

/// Git utilities for branch detection.
pub mod git {
    use std::path::Path;
    use std::process::Command;

    /// Get the current git branch name.
    pub fn current_branch(repo_path: &Path) -> Option<String> {
        let output = Command::new("git")
            .args(["rev-parse", "--abbrev-ref", "HEAD"])
            .current_dir(repo_path)
            .output()
            .ok()?;

        if output.status.success() {
            Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
        } else {
            None
        }
    }

    /// Get the current git commit hash.
    pub fn current_commit(repo_path: &Path) -> Option<String> {
        let output = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(repo_path)
            .output()
            .ok()?;

        if output.status.success() {
            Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
        } else {
            None
        }
    }

    /// Check if a path is inside a git repository.
    pub fn is_git_repo(path: &Path) -> bool {
        Command::new("git")
            .args(["rev-parse", "--git-dir"])
            .current_dir(path)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    /// Get list of changed files since a commit.
    pub fn changed_files_since(repo_path: &Path, commit: &str) -> Option<Vec<String>> {
        let output = Command::new("git")
            .args(["diff", "--name-only", commit, "HEAD"])
            .current_dir(repo_path)
            .output()
            .ok()?;

        if output.status.success() {
            let files = String::from_utf8_lossy(&output.stdout)
                .lines()
                .map(|s| s.to_string())
                .collect();
            Some(files)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_git_is_git_repo() {
        // Current directory should be a git repo (codex-rs)
        let current = std::env::current_dir().unwrap();
        // This might fail in CI, so we just test the function doesn't crash
        let _ = git::is_git_repo(&current);
    }

    #[test]
    fn test_git_current_branch() {
        let current = std::env::current_dir().unwrap();
        // Just test the function doesn't crash
        let _ = git::current_branch(&current);
    }
}
