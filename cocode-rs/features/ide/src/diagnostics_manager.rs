//! IDE diagnostics baseline/delta manager.
//!
//! Captures a baseline of IDE LSP diagnostics before file edits and computes
//! the delta (new issues) after the edit completes. This is separate from the
//! built-in LSP diagnostics in `cocode-lsp`, which handles diagnostics from
//! language servers managed by cocode itself.

use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::RwLock;
use tracing::debug;
use tracing::warn;

use crate::mcp_bridge::IdeDiagnosticRaw;
use crate::mcp_bridge::IdeMcpBridge;

/// Severity of a diagnostic issue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DiagnosticSeverity {
    Error,
    Warning,
    Information,
    Hint,
}

impl DiagnosticSeverity {
    /// Create from LSP severity integer (1=Error, 2=Warning, 3=Info, 4=Hint).
    pub fn from_lsp(value: i32) -> Self {
        match value {
            1 => Self::Error,
            2 => Self::Warning,
            3 => Self::Information,
            4 => Self::Hint,
            _ => Self::Hint,
        }
    }

    /// Symbol for display in system reminders (matching Claude Code).
    pub fn symbol(self) -> &'static str {
        match self {
            Self::Error => "\u{2717}",       // ✗
            Self::Warning => "\u{26A0}",     // ⚠
            Self::Information => "\u{2139}", // ℹ
            Self::Hint => "\u{2605}",        // ★
        }
    }

    /// Human-readable name.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warning => "warning",
            Self::Information => "info",
            Self::Hint => "hint",
        }
    }
}

/// A normalized IDE diagnostic used for baseline/delta comparison.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct IdeDiagnostic {
    pub message: String,
    pub severity: DiagnosticSeverity,
    pub source: Option<String>,
    pub code: Option<String>,
    pub range_start_line: i32,
    pub range_start_char: i32,
    pub range_end_line: i32,
    pub range_end_char: i32,
}

impl IdeDiagnostic {
    /// Convert from a raw IDE diagnostic.
    pub(crate) fn from_raw(raw: &IdeDiagnosticRaw) -> Self {
        Self {
            message: raw.message.clone(),
            severity: DiagnosticSeverity::from_lsp(raw.severity),
            source: raw.source.clone(),
            code: raw.code.as_ref().map(|c| match c {
                serde_json::Value::String(s) => s.clone(),
                other => other.to_string(),
            }),
            range_start_line: raw.range.start.line,
            range_start_char: raw.range.start.character,
            range_end_line: raw.range.end.line,
            range_end_char: raw.range.end.character,
        }
    }
}

/// Manages diagnostic baselines and deltas for IDE integration.
#[derive(Debug, Clone)]
pub struct IdeDiagnosticsManager {
    baseline: Arc<RwLock<HashMap<PathBuf, Vec<IdeDiagnostic>>>>,
}

impl Default for IdeDiagnosticsManager {
    fn default() -> Self {
        Self::new()
    }
}

impl IdeDiagnosticsManager {
    pub fn new() -> Self {
        Self {
            baseline: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Capture the diagnostic baseline for a file before editing.
    ///
    /// Calls the IDE's `getDiagnostics` tool and stores the result.
    pub async fn capture_baseline(&self, file_path: &Path, bridge: &IdeMcpBridge) {
        let uri = format!("file://{}", file_path.display());

        match bridge.get_diagnostics(Some(&uri)).await {
            Ok(raw_diagnostics) => {
                let diagnostics: Vec<IdeDiagnostic> = raw_diagnostics
                    .iter()
                    .map(IdeDiagnostic::from_raw)
                    .collect();

                debug!(
                    "Captured diagnostic baseline for {}: {} issues",
                    file_path.display(),
                    diagnostics.len()
                );

                let mut baseline = self.baseline.write().await;
                baseline.insert(file_path.to_path_buf(), diagnostics);
            }
            Err(e) => {
                warn!(
                    "Failed to capture diagnostic baseline for {}: {e}",
                    file_path.display()
                );
            }
        }
    }

    /// Compute the delta (new diagnostics) after an edit.
    ///
    /// For each file with a captured baseline, fetches current diagnostics
    /// from the IDE and returns only new issues not present in the baseline.
    /// Updates the baseline with the current state for next comparison.
    pub async fn compute_delta(&self, bridge: &IdeMcpBridge) -> Vec<(PathBuf, Vec<IdeDiagnostic>)> {
        // Snapshot baseline under lock, then release before doing network I/O
        let snapshot: Vec<(PathBuf, Vec<IdeDiagnostic>)> = {
            let baseline = self.baseline.read().await;
            baseline
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect()
        };

        let mut deltas = Vec::new();
        let mut updated_baselines = Vec::new();

        for (path, baseline_diags) in &snapshot {
            let uri = format!("file://{}", path.display());
            let current_for_file = match bridge.get_diagnostics(Some(&uri)).await {
                Ok(diags) => diags
                    .iter()
                    .map(IdeDiagnostic::from_raw)
                    .collect::<Vec<_>>(),
                Err(e) => {
                    debug!("Skipping diagnostics for {}: {e}", path.display());
                    continue;
                }
            };

            let new_diags: Vec<IdeDiagnostic> = current_for_file
                .iter()
                .filter(|d| !baseline_diags.contains(d))
                .cloned()
                .collect();

            if !new_diags.is_empty() {
                debug!(
                    "Found {} new diagnostics for {}",
                    new_diags.len(),
                    path.display()
                );
                deltas.push((path.clone(), new_diags));
            }

            updated_baselines.push((path.clone(), current_for_file));
        }

        // Update baseline with current state for next comparison
        let mut baseline = self.baseline.write().await;
        for (path, diagnostics) in updated_baselines {
            baseline.insert(path, diagnostics);
        }

        deltas
    }

    /// Clear the baseline for all files.
    pub async fn clear_baseline(&self) {
        let mut baseline = self.baseline.write().await;
        baseline.clear();
    }

    /// Check if we have a baseline for the given file.
    pub async fn has_baseline(&self, file_path: &Path) -> bool {
        let baseline = self.baseline.read().await;
        baseline.contains_key(file_path)
    }
}

#[cfg(test)]
#[path = "diagnostics_manager.test.rs"]
mod tests;
