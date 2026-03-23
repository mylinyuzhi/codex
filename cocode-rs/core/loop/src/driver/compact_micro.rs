//! Micro-compaction: no-LLM-call tool result cleanup.

use cocode_protocol::LoopEvent;

use tracing::debug;

use super::AgentLoop;

impl AgentLoop {
    /// Run micro-compaction (no LLM call).
    ///
    /// Clears old tool results from the message history when the context usage
    /// exceeds the warning threshold. Returns `(compacted_count, tokens_saved)`.
    pub(crate) async fn micro_compact(&mut self) -> (i32, i32) {
        // Check if micro-compact is enabled
        if !self.compact_config.is_micro_compact_enabled() {
            return (0, 0);
        }

        let tokens_before = self.message_history.estimate_tokens();
        let context_window = self.context.environment.context_window;

        // Use ThresholdStatus to check if we're above warning threshold
        let status = crate::compaction::ThresholdStatus::calculate(
            tokens_before,
            context_window,
            &self.compact_config,
        );

        if !status.is_above_warning_threshold {
            debug!(
                tokens_before,
                status = status.status_description(),
                "Below warning threshold, skipping micro-compact"
            );
            return (0, 0);
        }

        // Emit started event before compaction begins
        self.emit(LoopEvent::MicroCompactionStarted {
            candidates: 0, // Exact count will be in MicroCompactionApplied
            potential_savings: 0,
        })
        .await;

        // Apply micro-compaction using configured recent_tool_results_to_keep
        // Get paths from ContextModifier::FileRead for FileTracker cleanup
        let keep_count = self.compact_config.recent_tool_results_to_keep;
        let outcome = self.message_history.micro_compact_outcome(keep_count);

        // Clean up FileTracker entries for compacted reads using paths from modifiers
        // This is more accurate than tool_id mapping since it uses actual file paths
        if !outcome.cleared_read_paths.is_empty() {
            // Determine how many recent turns to preserve files from
            // This matches Claude Code's collectFilesToKeep behavior
            let keep_recent_turns = self.compact_config.micro_compact_keep_recent_turns;
            let files_to_keep =
                crate::compaction::collect_files_to_keep(&self.message_history, keep_recent_turns);

            let tracker = self.shared_tools_file_tracker.lock().await;

            // Collect paths to remove (excluding preserved files)
            let paths_to_remove: Vec<_> = outcome
                .cleared_read_paths
                .iter()
                .filter(|p| !files_to_keep.contains(*p))
                .cloned()
                .collect();

            if !paths_to_remove.is_empty() {
                tracker.remove_paths(&paths_to_remove);
            }

            debug!(
                cleared_paths = outcome.cleared_read_paths.len(),
                removed_paths = paths_to_remove.len(),
                files_preserved = files_to_keep.len(),
                "Cleaned up FileTracker entries for compacted reads (preserved recent files)"
            );
        }

        // Calculate tokens saved
        let tokens_after = self.message_history.estimate_tokens();
        let tokens_saved = tokens_before - tokens_after;

        debug!(
            removed = outcome.compacted_count,
            tokens_before, tokens_after, tokens_saved, "Micro-compaction complete"
        );

        (outcome.compacted_count, tokens_saved)
    }
}
