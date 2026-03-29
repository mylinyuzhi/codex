//! TUI-only events dropped by SDK and app-server consumers.
//!
//! These events drive overlay displays, progress indicators, and other
//! interactive UI elements that have no meaning outside the terminal UI.

use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;

use crate::ApprovalRequest;
use crate::event_types::AbortReason;
use crate::event_types::MarketplaceSummaryInfo;
use crate::event_types::OutputStyleItem;
use crate::event_types::PluginSummaryInfo;
use crate::event_types::RewindCheckpointItem;
use crate::event_types::RewindDiffStats;
use crate::event_types::SandboxAccessType;
use crate::event_types::ToolProgressInfo;

/// Events consumed exclusively by the TUI layer.
///
/// SDK and app-server consumers should ignore these events entirely.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TuiEvent {
    /// User approval is required to proceed.
    ApprovalRequired {
        /// The approval request.
        request: ApprovalRequest,
    },
    /// The model is asking the user a question.
    QuestionAsked {
        /// Unique request identifier.
        request_id: String,
        /// The questions to display (JSON array).
        questions: Value,
    },
    /// An MCP server is requesting user input via elicitation.
    ElicitationRequested {
        /// Unique request identifier.
        request_id: String,
        /// Name of the MCP server requesting input.
        server_name: String,
        /// Human-readable message to display.
        message: String,
        /// Elicitation mode: "form" or "url".
        mode: String,
        /// JSON Schema for form mode.
        schema: Option<Value>,
        /// URL for URL mode.
        url: Option<String>,
    },
    /// Sandbox permission approval required.
    SandboxApprovalRequired {
        /// The approval request.
        request: ApprovalRequest,
        /// Type of sandbox access requested.
        access_type: SandboxAccessType,
    },
    /// Plugin data ready for UI display.
    PluginDataReady {
        /// Installed plugin summaries.
        installed: Vec<PluginSummaryInfo>,
        /// Known marketplace summaries.
        marketplaces: Vec<MarketplaceSummaryInfo>,
    },
    /// Output styles data ready for the picker overlay.
    OutputStylesReady {
        /// Available output style items.
        styles: Vec<OutputStyleItem>,
    },
    /// Rewind checkpoints ready for the selector overlay.
    RewindCheckpointsReady {
        /// Available checkpoints in chronological order.
        checkpoints: Vec<RewindCheckpointItem>,
    },
    /// Diff stats ready for a specific rewind checkpoint.
    DiffStatsReady {
        /// The turn number these stats apply to.
        turn_number: i32,
        /// The computed diff stats.
        stats: RewindDiffStats,
    },
    /// Auto-compaction circuit breaker opened after consecutive failures.
    CompactionCircuitBreakerOpen {
        /// Number of consecutive failures that triggered the breaker.
        consecutive_failures: i32,
    },
    /// Micro-compaction was applied to tool results.
    MicroCompactionApplied {
        /// Number of results compacted.
        removed_results: i32,
        /// Tokens saved by compaction.
        tokens_saved: i32,
    },
    /// Session memory compaction was applied.
    SessionMemoryCompactApplied {
        /// Tokens saved.
        saved_tokens: i32,
        /// Tokens in the summary.
        summary_tokens: i32,
    },
    /// Background session memory extraction has started.
    SessionMemoryExtractionStarted {
        /// Current token count in the conversation.
        current_tokens: i32,
        /// Number of tool calls since the last extraction.
        tool_calls_since: i32,
    },
    /// Background session memory extraction has completed.
    SessionMemoryExtractionCompleted {
        /// Tokens in the new summary.
        summary_tokens: i32,
        /// ID of the last message that was summarized.
        last_summarized_id: String,
        /// Total number of messages that were summarized.
        messages_summarized: i32,
    },
    /// Background session memory extraction failed.
    SessionMemoryExtractionFailed {
        /// Error message.
        error: String,
        /// Number of attempts made.
        attempts: i32,
    },
    /// Speculative execution has been rolled back.
    SpeculativeRolledBack {
        /// Speculation batch identifier.
        speculation_id: String,
        /// Reason for rollback.
        reason: String,
        /// Tool calls that were rolled back.
        rolled_back_calls: Vec<String>,
    },
    /// A cron job was disabled by the circuit breaker.
    CronJobDisabled {
        /// Job identifier.
        job_id: String,
        /// Number of consecutive failures.
        consecutive_failures: i32,
    },
    /// One-shot tasks were missed during downtime.
    CronJobsMissed {
        /// Number of missed tasks.
        count: i32,
        /// Summary of missed tasks.
        summary: String,
    },
    /// Tool call delta (partial tool call JSON).
    ToolCallDelta {
        /// Call identifier.
        call_id: String,
        /// The tool call delta.
        delta: String,
    },
    /// Progress update from a tool.
    ToolProgress {
        /// Call identifier.
        call_id: String,
        /// Progress information.
        progress: ToolProgressInfo,
    },
    /// Tool execution was aborted.
    ToolExecutionAborted {
        /// Reason for abortion.
        reason: AbortReason,
    },
}
