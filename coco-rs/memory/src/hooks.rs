//! Agent loop integration for memory extraction.
//!
//! TS: query/stopHooks.ts — executeExtractMemories, hasMemoryWritesSince.
//! TS: services/extractMemories/extractMemories.ts — initExtractMemories,
//!     drainPendingExtraction.
//!
//! The extraction hook fires at turn-end when the model produces a final
//! response (no tool calls). It runs as a background task, sharing the
//! prompt cache with the main conversation.

use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;

use crate::config::MemoryConfig;
use crate::scan;

/// State for the extraction hook, maintained across turns.
///
/// TS: services/extractMemories/extractMemories.ts — closure-scoped state in
/// `initExtractMemories()`.
#[derive(Debug)]
pub struct ExtractionHookState {
    /// UUID of the last message that was processed for extraction.
    pub last_cursor: Option<String>,
    /// Whether an extraction is currently in progress.
    pub in_progress: bool,
    /// Turn counter for throttle gate.
    pub turn_count: i32,
    /// Turns since last extraction (for throttle).
    pub turns_since_last_extraction: i32,
    /// Stashed context for trailing run: if a new turn ends while
    /// extraction is in-progress, the context is stashed here and run
    /// recursively after the current extraction finishes.
    pub pending_trailing_run: bool,
    /// Count of model-visible messages since last cursor.
    pub new_message_count: i32,
    /// Memory directory path.
    pub memory_dir: PathBuf,
    /// Configuration.
    pub config: MemoryConfig,
}

impl ExtractionHookState {
    pub fn new(memory_dir: PathBuf, config: MemoryConfig) -> Self {
        Self {
            last_cursor: None,
            in_progress: false,
            turn_count: 0,
            turns_since_last_extraction: 0,
            pending_trailing_run: false,
            new_message_count: 0,
            memory_dir,
            config,
        }
    }
}

/// Thread-safe extraction hook handle.
pub type ExtractionHook = Arc<Mutex<ExtractionHookState>>;

/// Create a new extraction hook.
pub fn init_extraction_hook(memory_dir: PathBuf, config: MemoryConfig) -> ExtractionHook {
    Arc::new(Mutex::new(ExtractionHookState::new(memory_dir, config)))
}

/// Check whether extraction should fire for this turn.
///
/// TS: executeExtractMemoriesImpl gate sequence.
///
/// Conditions:
/// 1. Auto-memory and extraction are enabled
/// 2. No extraction currently in progress (if so, stash as trailing run)
/// 3. Throttle gate passes (turns_since_last >= throttle)
/// 4. Main agent didn't already write memories this turn
pub fn should_extract(hook: &ExtractionHook, has_memory_writes: bool) -> bool {
    let mut state = match hook.lock() {
        Ok(s) => s,
        Err(_) => return false,
    };

    if !state.config.enabled || !state.config.extraction_enabled {
        return false;
    }

    if state.in_progress {
        // Stash for trailing run when current extraction finishes
        state.pending_trailing_run = true;
        return false;
    }

    if has_memory_writes {
        return false;
    }

    // Throttle gate (TS: turnsSinceLastExtraction < throttle → skip)
    state.turns_since_last_extraction += 1;
    if state.config.extraction_throttle > 0
        && state.turns_since_last_extraction < state.config.extraction_throttle
    {
        return false;
    }
    state.turns_since_last_extraction = 0;

    true
}

/// Mark extraction as started and advance the cursor.
pub fn begin_extraction(hook: &ExtractionHook, message_id: &str) {
    if let Ok(mut state) = hook.lock() {
        state.in_progress = true;
        state.last_cursor = Some(message_id.to_string());
    }
}

/// Mark extraction as completed. Returns whether a trailing run is pending.
///
/// TS: On completion, checks pendingContext and runs recursively.
pub fn end_extraction(hook: &ExtractionHook) -> bool {
    if let Ok(mut state) = hook.lock() {
        state.in_progress = false;
        let trailing = state.pending_trailing_run;
        state.pending_trailing_run = false;
        return trailing;
    }
    false
}

/// Advance the turn counter (called at end of each agent turn).
pub fn advance_turn(hook: &ExtractionHook) {
    if let Ok(mut state) = hook.lock() {
        state.turn_count += 1;
    }
}

/// Update the new message count since the last extraction cursor.
///
/// TS: countModelVisibleMessagesSince — counts user/assistant messages.
pub fn set_new_message_count(hook: &ExtractionHook, count: i32) {
    if let Ok(mut state) = hook.lock() {
        state.new_message_count = count;
    }
}

/// Get the new message count for the extraction prompt.
pub fn get_new_message_count(hook: &ExtractionHook) -> i32 {
    hook.lock().map_or(0, |s| s.new_message_count)
}

/// Check if an extraction is currently in progress.
pub fn is_extracting(hook: &ExtractionHook) -> bool {
    hook.lock().is_ok_and(|s| s.in_progress)
}

/// Get the last cursor UUID.
pub fn get_last_cursor(hook: &ExtractionHook) -> Option<String> {
    hook.lock().ok().and_then(|s| s.last_cursor.clone())
}

/// Check if a file write targets the memory directory.
///
/// Used by `hasMemoryWritesSince` to detect if the main agent already
/// wrote memories, making background extraction unnecessary.
pub fn is_memory_write(path: &Path, memory_dir: &Path) -> bool {
    crate::security::is_within_memory_dir(path, memory_dir)
}

/// Build the extraction context: scan existing memories and format manifest.
///
/// Called before spawning the extraction agent to pre-inject existing
/// memory state into the extraction prompt (avoids wasting a read turn).
pub fn build_extraction_context(memory_dir: &Path) -> ExtractionContext {
    let scanned = scan::scan_memory_files(memory_dir);
    let manifest = scan::format_memory_manifest(&scanned);
    let file_count = scanned.len() as i32;

    ExtractionContext {
        manifest,
        file_count,
    }
}

/// Pre-computed extraction context for the extraction agent.
#[derive(Debug, Clone)]
pub struct ExtractionContext {
    /// Formatted manifest of existing memories.
    pub manifest: String,
    /// Number of existing memory files.
    pub file_count: i32,
}

/// Tool permission decisions for the extraction agent.
///
/// TS: createAutoMemCanUseTool — whitelist of allowed tools.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtractionToolDecision {
    /// Tool is allowed without restriction.
    Allow,
    /// Tool is allowed only for paths within the memory directory.
    AllowMemdirOnly,
    /// Tool is allowed in read-only mode.
    AllowReadOnly,
    /// Tool is denied.
    Deny,
}

/// Maximum turns for the extraction agent.
pub const EXTRACTION_MAX_TURNS: i32 = 5;

#[cfg(test)]
#[path = "hooks.test.rs"]
mod tests;
