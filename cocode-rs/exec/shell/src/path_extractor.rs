//! Path extraction from command output.
//!
//! This module provides a trait for extracting file paths from shell command output,
//! enabling fast model pre-reading of files that commands read or modify.
//!
//! ## Usage
//!
//! The `PathExtractor` trait allows pluggable extraction strategies:
//! - `NoOpExtractor`: Default no-op implementation (no extraction)
//! - Custom implementations can use LLM models for intelligent extraction
//!
//! ## Example
//!
//! ```no_run
//! use cocode_shell::path_extractor::{PathExtractor, NoOpExtractor, PathExtractionResult};
//! use std::path::Path;
//!
//! # async fn example() -> anyhow::Result<()> {
//! let extractor = NoOpExtractor;
//!
//! // Check if extraction is enabled
//! if extractor.is_enabled() {
//!     let result = extractor.extract_paths(
//!         "git status",
//!         " M src/main.rs\n M src/lib.rs",
//!         Path::new("/project"),
//!     ).await?;
//!
//!     for path in result.paths {
//!         println!("Found: {}", path.display());
//!     }
//! }
//! # Ok(())
//! # }
//! ```

use std::future::Future;
use std::path::Path;
use std::path::PathBuf;
use std::pin::Pin;

/// Maximum output length for path extraction (matches Claude Code).
///
/// Longer outputs are truncated to this length before being sent to
/// the extraction model for efficiency.
pub const MAX_EXTRACTION_OUTPUT_CHARS: usize = 2000;

/// Result of path extraction from command output.
#[derive(Clone, Debug, Default)]
pub struct PathExtractionResult {
    /// Extracted file paths that exist on the filesystem.
    pub paths: Vec<PathBuf>,
    /// Duration of extraction in milliseconds.
    pub extraction_ms: i64,
}

impl PathExtractionResult {
    /// Creates a new result with the given paths.
    pub fn new(paths: Vec<PathBuf>, extraction_ms: i64) -> Self {
        Self {
            paths,
            extraction_ms,
        }
    }

    /// Creates an empty result with zero duration.
    pub fn empty() -> Self {
        Self::default()
    }
}

/// Boxed future type for dyn-compatible async trait methods.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Trait for extracting file paths from command output.
///
/// Implementations can use various strategies to extract paths:
/// - Pattern matching / regex
/// - LLM-based extraction (e.g., using a fast model like Haiku)
/// - Static analysis
///
/// The trait is async to support LLM-based implementations that need
/// to make API calls.
///
/// This trait is dyn-compatible, allowing it to be used as `Arc<dyn PathExtractor>`.
pub trait PathExtractor: Send + Sync {
    /// Extracts file paths from command output.
    ///
    /// # Arguments
    ///
    /// * `command` - The shell command that was executed
    /// * `output` - The command's stdout (may be truncated for extraction)
    /// * `cwd` - The working directory where the command was executed
    ///
    /// # Returns
    ///
    /// A result containing extracted paths that exist on the filesystem.
    /// Non-existent paths are filtered out.
    fn extract_paths<'a>(
        &'a self,
        command: &'a str,
        output: &'a str,
        cwd: &'a Path,
    ) -> BoxFuture<'a, anyhow::Result<PathExtractionResult>>;

    /// Returns true if this extractor is enabled and should be used.
    ///
    /// Used to skip extraction entirely when not configured.
    fn is_enabled(&self) -> bool;
}

/// No-op path extractor (default fallback).
///
/// This extractor does nothing and returns empty results.
/// Used when no fast model is configured or extraction is disabled.
#[derive(Clone, Debug, Default)]
pub struct NoOpExtractor;

impl PathExtractor for NoOpExtractor {
    fn extract_paths<'a>(
        &'a self,
        _command: &'a str,
        _output: &'a str,
        _cwd: &'a Path,
    ) -> BoxFuture<'a, anyhow::Result<PathExtractionResult>> {
        Box::pin(async { Ok(PathExtractionResult::empty()) })
    }

    fn is_enabled(&self) -> bool {
        false
    }
}

/// Truncates output to the maximum extraction length.
///
/// This is used to limit the amount of text sent to the extraction model
/// for efficiency, matching Claude Code's behavior.
pub fn truncate_for_extraction(output: &str) -> &str {
    if output.len() <= MAX_EXTRACTION_OUTPUT_CHARS {
        output
    } else {
        // Find a safe UTF-8 boundary
        let mut end = MAX_EXTRACTION_OUTPUT_CHARS;
        while end > 0 && !output.is_char_boundary(end) {
            end -= 1;
        }
        &output[..end]
    }
}

/// Filters paths to only include those that exist as files.
///
/// Also resolves relative paths against the provided working directory.
pub fn filter_existing_files(paths: Vec<PathBuf>, cwd: &Path) -> Vec<PathBuf> {
    paths
        .into_iter()
        .filter_map(|p| {
            let absolute = if p.is_absolute() { p } else { cwd.join(&p) };

            // Only include files that exist (not directories)
            if absolute.is_file() {
                Some(absolute)
            } else {
                None
            }
        })
        .collect()
}

#[cfg(test)]
#[path = "path_extractor.test.rs"]
mod tests;
