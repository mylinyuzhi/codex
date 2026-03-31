//! Auto memory extraction agent.
//!
//! Fire-and-forget background agent that extracts persistent memories
//! (user, feedback, project, reference) from conversation. Runs after
//! each agent turn when `Feature::MemoryExtraction` is enabled.
//!
//! This is distinct from `SessionMemoryExtractionAgent` which summarizes
//! conversation for compaction -- this agent creates persistent memory
//! entries in the auto memory directory.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

use tokio::sync::Mutex;
use tracing::debug;

/// Result of checking for trailing extraction after completion.
pub struct TrailingExtraction {
    pub variant: ExtractionPromptVariant,
    pub message_count: i32,
}

/// Extraction prompt variant selector.
#[derive(Debug, Clone, Copy)]
pub enum ExtractionPromptVariant {
    /// Standard single-directory memory.
    Standard,
    /// Typed format with frontmatter structure.
    Typed,
    /// Team mode with user/team distinction.
    Team,
    /// Typed team mode with both typed format and user/team.
    TypedTeam,
}

impl ExtractionPromptVariant {
    /// Select the appropriate variant based on feature flags.
    pub fn select(team_memory_enabled: bool, typed_format: bool) -> Self {
        match (team_memory_enabled, typed_format) {
            (false, false) => Self::Standard,
            (false, true) => Self::Typed,
            (true, false) => Self::Team,
            (true, true) => Self::TypedTeam,
        }
    }
}

/// Stashed extraction context for coalescing.
struct StashedContext {
    message_count: i32,
    prompt_variant: ExtractionPromptVariant,
}

/// Auto memory extraction coordinator.
///
/// Manages the lifecycle of extraction subagent runs, including
/// guard checks, coalescing, and direct-write detection.
pub struct AutoMemoryExtractionCoordinator {
    /// Whether extraction is currently in progress.
    in_progress: AtomicBool,
    /// Stashed context for trailing run (coalescing).
    stashed: Mutex<Option<StashedContext>>,
    /// Memory directory for direct-write detection.
    memory_dir: PathBuf,
}

impl AutoMemoryExtractionCoordinator {
    pub fn new(memory_dir: PathBuf) -> Self {
        Self {
            in_progress: AtomicBool::new(false),
            stashed: Mutex::new(None),
            memory_dir,
        }
    }

    pub fn new_arc(memory_dir: PathBuf) -> Arc<Self> {
        Arc::new(Self::new(memory_dir))
    }

    /// Check all guard conditions for extraction.
    ///
    /// Returns false if extraction should be skipped.
    pub fn should_extract(&self, is_subagent: bool, memory_enabled: bool) -> bool {
        if is_subagent {
            return false;
        }
        if !memory_enabled {
            return false;
        }
        true
    }

    /// Attempt to start an extraction run.
    ///
    /// Returns `Some(variant)` if extraction should proceed,
    /// `None` if coalesced (stashed for trailing run).
    pub async fn try_start(
        &self,
        message_count: i32,
        prompt_variant: ExtractionPromptVariant,
    ) -> Option<ExtractionPromptVariant> {
        if self.in_progress.load(Ordering::Acquire) {
            // Coalesce: stash for trailing run
            debug!("Memory extraction in progress -- stashing for trailing run");
            *self.stashed.lock().await = Some(StashedContext {
                message_count,
                prompt_variant,
            });
            return None;
        }

        self.in_progress.store(true, Ordering::Release);
        Some(prompt_variant)
    }

    /// Mark current extraction as complete and check for trailing run.
    ///
    /// Returns `Some(TrailingExtraction)` if a trailing extraction should be started,
    /// including the message_count from the stashed context.
    pub async fn complete(&self) -> Option<TrailingExtraction> {
        self.in_progress.store(false, Ordering::Release);

        let stashed = self.stashed.lock().await.take();
        if let Some(ctx) = stashed {
            debug!(
                message_count = ctx.message_count,
                "Running trailing extraction for stashed context"
            );
            self.in_progress.store(true, Ordering::Release);
            Some(TrailingExtraction {
                variant: ctx.prompt_variant,
                message_count: ctx.message_count,
            })
        } else {
            None
        }
    }

    /// Mark current extraction as failed.
    ///
    /// Clears stashed context to avoid retrying with stale data.
    pub async fn mark_failed(&self) {
        self.in_progress.store(false, Ordering::Release);
        *self.stashed.lock().await = None;
    }

    /// Check if any tool calls in the messages wrote to the memory directory.
    ///
    /// If the conversation already wrote memory files directly, extraction
    /// is skipped to avoid double-writing.
    pub fn has_direct_memory_writes(&self, write_paths: &[PathBuf]) -> bool {
        write_paths
            .iter()
            .any(|p| cocode_auto_memory::is_auto_memory_path(p, &self.memory_dir))
    }
}

/// Build the extraction subagent prompt for the given variant.
///
/// Delegates to the variant-specific prompt builders in
/// `cocode_auto_memory::prompt`, which provide full guidance on
/// memory types, save triggers, organization, and formatting.
pub fn build_extraction_prompt(variant: ExtractionPromptVariant, message_count: i32) -> String {
    match variant {
        ExtractionPromptVariant::Standard => {
            cocode_auto_memory::prompt::build_extraction_prompt_standard(message_count)
        }
        ExtractionPromptVariant::Typed => {
            cocode_auto_memory::prompt::build_extraction_prompt_typed(message_count)
        }
        ExtractionPromptVariant::Team => {
            cocode_auto_memory::prompt::build_extraction_prompt_team(message_count)
        }
        ExtractionPromptVariant::TypedTeam => {
            cocode_auto_memory::prompt::build_extraction_prompt_typed_team(message_count)
        }
    }
}
