//! Auto memory session state.
//!
//! Thread-safe state container that holds the resolved config and
//! the loaded MEMORY.md index. Refreshed from disk each turn.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

use tokio::sync::RwLock;
use tracing::debug;
use tracing::warn;

use crate::config::ResolvedAutoMemoryConfig;
use crate::memory_file::MemoryIndex;

/// Thread-safe auto memory state for a session.
///
/// Shared via `Arc` between the agent loop, system reminder generators,
/// and the tool permission pipeline.
#[derive(Debug)]
pub struct AutoMemoryState {
    /// Resolved configuration.
    pub config: ResolvedAutoMemoryConfig,
    /// Loaded MEMORY.md index (refreshed each turn).
    index: RwLock<Option<MemoryIndex>>,
    /// Whether the memory directory has been successfully created.
    dir_created: AtomicBool,
    /// Team memory MEMORY.md index.
    team_index: RwLock<Option<MemoryIndex>>,
    /// Whether team memory directory has been created.
    team_dir_created: AtomicBool,
}

impl AutoMemoryState {
    /// Create a new auto memory state.
    pub fn new(config: ResolvedAutoMemoryConfig) -> Self {
        Self {
            config,
            index: RwLock::new(None),
            dir_created: AtomicBool::new(false),
            team_index: RwLock::new(None),
            team_dir_created: AtomicBool::new(false),
        }
    }

    /// Create a new state wrapped in `Arc` (common usage pattern).
    pub fn new_arc(config: ResolvedAutoMemoryConfig) -> Arc<Self> {
        Arc::new(Self::new(config))
    }

    /// Refresh the MEMORY.md index from disk.
    ///
    /// Called at the start of each agent loop turn to ensure the model
    /// always sees the latest content. Also refreshes team MEMORY.md
    /// when team memory is enabled, loading both in parallel.
    #[tracing::instrument(skip(self), fields(dir = %self.config.directory.display()))]
    pub async fn refresh(&self) {
        if !self.config.enabled {
            return;
        }

        // Ensure directories exist (sequential, only on first successful call).
        self.ensure_dirs().await;

        // Load both indexes in parallel.
        let (user_result, team_result) = tokio::join!(
            self.load_index(&self.config.directory, self.config.max_lines),
            async {
                if !self.config.team_memory_enabled
                    || !self.team_dir_created.load(Ordering::Relaxed)
                {
                    return None;
                }
                self.load_index(&self.config.team_memory_directory, self.config.max_lines)
                    .await
            },
        );

        if let Some(index) = user_result {
            *self.index.write().await = index;
        }
        if let Some(index) = team_result {
            *self.team_index.write().await = index;
        }
    }

    /// Ensure user and team memory directories exist on first call.
    async fn ensure_dirs(&self) {
        if !self.dir_created.load(Ordering::Relaxed) {
            if let Err(e) = crate::directory::ensure_memory_dir_exists(&self.config.directory).await
            {
                warn!(error = %e, "Failed to ensure memory directory exists");
                return;
            }
            self.dir_created.store(true, Ordering::Relaxed);
        }

        if self.config.team_memory_enabled && !self.team_dir_created.load(Ordering::Relaxed) {
            if let Err(e) =
                crate::directory::ensure_memory_dir_exists(&self.config.team_memory_directory).await
            {
                warn!(error = %e, "Failed to ensure team memory directory exists");
                return;
            }
            self.team_dir_created.store(true, Ordering::Relaxed);
        }
    }

    /// Load a MEMORY.md index from a directory via `spawn_blocking`.
    ///
    /// Returns `Some(index)` on success (including `None` when the file
    /// does not exist), or `None` on error (already logged).
    async fn load_index(
        &self,
        dir: &std::path::Path,
        max_lines: i32,
    ) -> Option<Option<MemoryIndex>> {
        let dir = dir.to_path_buf();
        let result = tokio::task::spawn_blocking(move || {
            crate::memory_file::load_memory_index(&dir, max_lines)
        })
        .await;

        match result {
            Ok(Ok(index)) => {
                debug!(has_index = index.is_some(), "Refreshed MEMORY.md");
                Some(index)
            }
            Ok(Err(e)) => {
                warn!(error = %e, "Failed to refresh MEMORY.md");
                None
            }
            Err(e) => {
                warn!(error = %e, "spawn_blocking panicked during MEMORY.md refresh");
                None
            }
        }
    }

    /// Get the current MEMORY.md index.
    pub async fn index(&self) -> Option<MemoryIndex> {
        self.index.read().await.clone()
    }

    /// Get the current team MEMORY.md index.
    pub async fn team_index(&self) -> Option<MemoryIndex> {
        self.team_index.read().await.clone()
    }

    /// Check if auto memory is enabled.
    pub fn is_enabled(&self) -> bool {
        self.config.enabled
    }

    /// Get the memory directory path as a string.
    pub fn memory_dir_str(&self) -> String {
        self.config.directory.display().to_string()
    }

    /// Get the team memory directory path as a string.
    pub fn team_memory_dir_str(&self) -> String {
        self.config.team_memory_directory.display().to_string()
    }
}

#[cfg(test)]
#[path = "state.test.rs"]
mod tests;
