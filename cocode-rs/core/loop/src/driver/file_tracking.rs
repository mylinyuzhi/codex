//! FileTracker management methods for the agent loop.

use cocode_system_reminder::MentionReadRecord;
use cocode_tools_api::FileReadState;
use cocode_tools_api::FileTracker;
use tracing::debug;

use crate::compaction::FileRestoration;

use super::AgentLoop;

impl AgentLoop {
    /// Build a per-turn derived `FileTracker` view for system reminder generation.
    ///
    /// Snapshots the shared tracker state, then releases the lock so generators
    /// can run without holding it.
    ///
    /// # Claude Code Alignment
    ///
    /// CODEX's per-turn derived tracker view pattern: snapshot → release lock →
    /// pass view to generators → bridge mention reads back afterward.
    pub(crate) async fn build_reminder_tracker_view(&self) -> FileTracker {
        let snapshot = {
            let tools_tracker = self.shared_tools_file_tracker.lock().await;
            tools_tracker.read_files_snapshot()
        };
        // Lock is released here
        let tracker = FileTracker::new();
        tracker.replace_snapshot(snapshot);
        tracker
    }

    /// Apply mention read records from system reminder generation to the shared tracker.
    ///
    /// After `generate_all()` completes, generators may have pushed `MentionReadRecord`
    /// entries into the shared buffer. This method drains those records and applies
    /// them to the canonical shared tools FileTracker.
    pub(crate) async fn apply_mention_read_records(&self, records: &[MentionReadRecord]) {
        if records.is_empty() {
            return;
        }
        let tracker = self.shared_tools_file_tracker.lock().await;
        for record in records {
            let state = match record.read_kind {
                cocode_protocol::FileReadKind::FullContent => FileReadState::complete_with_turn(
                    record.content.clone(),
                    record.last_modified,
                    record.read_turn,
                ),
                cocode_protocol::FileReadKind::PartialContent => FileReadState::partial_with_turn(
                    record.offset.unwrap_or(0),
                    record.limit.unwrap_or(0),
                    record.last_modified,
                    record.read_turn,
                ),
                cocode_protocol::FileReadKind::MetadataOnly => {
                    FileReadState::metadata_only(record.last_modified, record.read_turn)
                }
            };
            tracker.record_read_with_state(record.path.clone(), state);
        }
        debug!(
            count = records.len(),
            "Applied mention read records to FileTracker"
        );
    }

    /// Rebuild FileTracker from restored file context after compaction.
    ///
    /// After compaction restores files, the FileTracker must be rebuilt to match
    /// the restored context. This replaces ALL tracker entries with entries
    /// derived from the restored files.
    ///
    /// # Claude Code Alignment
    ///
    /// Claude Code clears readFileState entirely during compaction and rebuilds
    /// from restored files only.
    pub(crate) async fn rebuild_trackers_from_restored_files(&self, files: &[FileRestoration]) {
        let mut entries = Vec::with_capacity(files.len());
        for file in files {
            let file_mtime = std::fs::metadata(&file.path)
                .ok()
                .and_then(|m| m.modified().ok());
            entries.push((
                file.path.clone(),
                FileReadState::complete_with_turn(
                    file.content.clone(),
                    file_mtime,
                    self.turn_number,
                ),
            ));
        }
        let tracker = self.shared_tools_file_tracker.lock().await;
        tracker.replace_snapshot(entries);
        debug!(
            files_count = files.len(),
            "Rebuilt FileTracker from restored files"
        );
    }

    /// Restore FileTracker state for rewind.
    ///
    /// When a rewind occurs, the FileTracker needs to be restored to match
    /// the state at the target turn. This extracts all file reads from
    /// historical tool calls up to that turn and rebuilds the tracker state.
    ///
    /// # Claude Code Alignment
    ///
    /// This matches Claude Code v2.1.38's rewind file state restoration:
    /// - Extract file reads from ContextModifier::FileRead in tool calls
    /// - Clear current FileTracker state
    /// - Rebuild state from historical reads
    pub(crate) async fn restore_file_tracker_for_rewind(&mut self, to_turn: i32) {
        // Extract file reads from history up to the target turn
        let extractions = self.message_history.extract_file_reads_up_to_turn(to_turn);

        if extractions.is_empty() {
            debug!(to_turn, "No file reads to restore for rewind");
            return;
        }

        // Clear current FileTracker state and rebuild
        let tracker = self.shared_tools_file_tracker.lock().await;
        tracker.clear_reads();

        // Convert mtime from ms if provided
        let convert_mtime = |ms: Option<i64>| -> Option<std::time::SystemTime> {
            ms.and_then(|ms| {
                std::time::UNIX_EPOCH.checked_add(std::time::Duration::from_millis(ms as u64))
            })
        };

        for extraction in extractions {
            let file_mtime = convert_mtime(extraction.file_mtime_ms);

            let state = match extraction.kind {
                cocode_protocol::FileReadKind::FullContent => {
                    if let Some(content) = extraction.content {
                        cocode_tools_api::FileReadState::complete_with_turn(
                            content,
                            file_mtime,
                            extraction.read_turn,
                        )
                    } else {
                        // Content was compacted, just track metadata
                        cocode_tools_api::FileReadState::metadata_only(
                            file_mtime,
                            extraction.read_turn,
                        )
                    }
                }
                cocode_protocol::FileReadKind::PartialContent => {
                    cocode_tools_api::FileReadState::partial_with_turn(
                        extraction.offset.unwrap_or(0),
                        extraction.limit.unwrap_or(0),
                        file_mtime,
                        extraction.read_turn,
                    )
                }
                cocode_protocol::FileReadKind::MetadataOnly => {
                    cocode_tools_api::FileReadState::metadata_only(file_mtime, extraction.read_turn)
                }
            };

            tracker.track_read(extraction.path.clone(), state);
        }

        debug!(
            to_turn,
            restored_count = tracker.read_count(),
            "Restored FileTracker state for rewind"
        );
    }
}
