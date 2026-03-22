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
}

impl AutoMemoryState {
    /// Create a new auto memory state.
    pub fn new(config: ResolvedAutoMemoryConfig) -> Self {
        Self {
            config,
            index: RwLock::new(None),
            dir_created: AtomicBool::new(false),
        }
    }

    /// Create a new state wrapped in `Arc` (common usage pattern).
    pub fn new_arc(config: ResolvedAutoMemoryConfig) -> Arc<Self> {
        Arc::new(Self::new(config))
    }

    /// Refresh the MEMORY.md index from disk.
    ///
    /// Called at the start of each agent loop turn to ensure the model
    /// always sees the latest content.
    #[tracing::instrument(skip(self), fields(dir = %self.config.directory.display()))]
    pub async fn refresh(&self) {
        if !self.config.enabled {
            return;
        }

        // Ensure the memory directory exists (only on first successful call).
        if !self.dir_created.load(Ordering::Relaxed) {
            if let Err(e) = crate::directory::ensure_memory_dir_exists(&self.config.directory).await
            {
                warn!(error = %e, "Failed to ensure memory directory exists");
                return;
            }
            self.dir_created.store(true, Ordering::Relaxed);
        }

        // Run sync file I/O off the async runtime to avoid blocking.
        let dir = self.config.directory.clone();
        let max_lines = self.config.max_lines;
        let result = tokio::task::spawn_blocking(move || {
            crate::memory_file::load_memory_index(&dir, max_lines)
        })
        .await;

        match result {
            Ok(Ok(index)) => {
                debug!(
                    has_index = index.is_some(),
                    dir = %self.config.directory.display(),
                    "Refreshed MEMORY.md"
                );
                *self.index.write().await = index;
            }
            Ok(Err(e)) => {
                warn!(error = %e, "Failed to refresh MEMORY.md");
            }
            Err(e) => {
                warn!(error = %e, "spawn_blocking panicked during MEMORY.md refresh");
            }
        }
    }

    /// Get the current MEMORY.md index.
    pub async fn index(&self) -> Option<MemoryIndex> {
        self.index.read().await.clone()
    }

    /// Check if auto memory is enabled.
    pub fn is_enabled(&self) -> bool {
        self.config.enabled
    }

    /// Get the memory directory path as a string.
    pub fn memory_dir_str(&self) -> String {
        self.config.directory.display().to_string()
    }
}

#[cfg(test)]
#[path = "state.test.rs"]
mod tests;
