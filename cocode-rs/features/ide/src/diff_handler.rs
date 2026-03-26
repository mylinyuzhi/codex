//! IDE diff handler.
//!
//! Routes file edit previews to the IDE's diff view with three-way resolution
//! (accept, reject, close). Falls back to direct file write when the IDE is
//! disconnected or the file type is unsupported (e.g. `.ipynb`).

use std::path::Path;
use std::sync::Arc;

use rand::Rng;
use tracing::debug;
use tracing::warn;

use crate::mcp_bridge::DiffResolution;
use crate::mcp_bridge::IdeMcpBridge;

/// Result of an IDE diff operation.
#[derive(Debug, Clone)]
pub enum DiffResult {
    /// User accepted the edit (content may be user-modified).
    Accepted { content: String },
    /// User explicitly rejected the edit.
    Rejected,
    /// IDE not available or file type unsupported; fall back to direct write.
    Fallback,
}

/// Handles routing file edits to the IDE's diff preview.
#[derive(Debug)]
pub struct IdeDiffHandler {
    bridge: Arc<IdeMcpBridge>,
}

impl IdeDiffHandler {
    pub fn new(bridge: Arc<IdeMcpBridge>) -> Self {
        Self { bridge }
    }

    /// Handle a file edit by opening a diff in the IDE.
    ///
    /// Returns `DiffResult::Fallback` if the IDE is disconnected, the file
    /// type is unsupported (`.ipynb`), or the diff tool call fails.
    pub async fn handle_edit(
        &self,
        file_path: &Path,
        old_content: &str,
        new_content: &str,
    ) -> DiffResult {
        // Skip .ipynb files (too complex for IDE diff)
        if file_path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("ipynb"))
        {
            debug!(
                "Skipping IDE diff for notebook file: {}",
                file_path.display()
            );
            return DiffResult::Fallback;
        }

        // No change — skip diff
        if old_content == new_content {
            return DiffResult::Fallback;
        }

        // Check connection
        if !self.bridge.is_connected().await {
            return DiffResult::Fallback;
        }

        // Generate unique tab name
        let tab_name = generate_tab_name(file_path);

        match self
            .bridge
            .open_diff(file_path, file_path, new_content, &tab_name)
            .await
        {
            Ok(DiffResolution::FileSaved { content }) => {
                debug!(
                    "IDE diff accepted (user-edited) for {}",
                    file_path.display()
                );
                DiffResult::Accepted { content }
            }
            Ok(DiffResolution::TabClosed) => {
                debug!("IDE diff tab closed for {}", file_path.display());
                DiffResult::Accepted {
                    content: new_content.to_string(),
                }
            }
            Ok(DiffResolution::DiffRejected) => {
                debug!("IDE diff rejected for {}", file_path.display());
                DiffResult::Rejected
            }
            Err(e) => {
                warn!("IDE diff failed for {}: {e}", file_path.display());
                DiffResult::Fallback
            }
        }
    }
}

/// Generate a unique diff tab name matching Claude Code's format.
///
/// Format: `✻ [Claude Code] {filename} ({random_id}) ⧉`
fn generate_tab_name(file_path: &Path) -> String {
    let filename = file_path.file_name().unwrap_or_default().to_string_lossy();

    let random_id: String = rand::rng()
        .sample_iter(&rand::distr::Alphanumeric)
        .take(6)
        .map(char::from)
        .collect();

    format!("\u{273B} [Claude Code] {filename} ({random_id}) \u{29C9}")
}

#[cfg(test)]
#[path = "diff_handler.test.rs"]
mod tests;
