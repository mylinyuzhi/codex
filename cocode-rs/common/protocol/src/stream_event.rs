//! Raw streaming delta events from the model API.
//!
//! These events require stateful accumulation (via `StreamAccumulator`)
//! before they can be converted into [`ServerNotification`]s.

use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;

use crate::ToolResultContent;

/// Raw streaming events emitted by the model API.
///
/// Each variant represents a stateful delta that needs accumulation
/// before it becomes a completed protocol item.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StreamEvent {
    /// Incremental text content from the model.
    TextDelta {
        /// Turn identifier.
        turn_id: String,
        /// The text delta.
        delta: String,
    },
    /// Incremental thinking/reasoning content from the model.
    ThinkingDelta {
        /// Turn identifier.
        turn_id: String,
        /// The thinking delta.
        delta: String,
    },
    /// A tool use has been queued for execution.
    ToolUseQueued {
        /// Call identifier.
        call_id: String,
        /// Tool name.
        name: String,
        /// Tool input (JSON).
        input: Value,
    },
    /// A tool has started executing.
    ToolUseStarted {
        /// Call identifier.
        call_id: String,
        /// Tool name.
        name: String,
        /// Batch ID for parallel execution grouping.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        batch_id: Option<String>,
    },
    /// A tool has completed execution.
    ToolUseCompleted {
        /// Call identifier.
        call_id: String,
        /// Tool output.
        output: ToolResultContent,
        /// Whether the tool returned an error.
        is_error: bool,
    },
    /// An MCP tool call has begun.
    McpToolCallBegin {
        /// Server name.
        server: String,
        /// Tool name.
        tool: String,
        /// Call identifier.
        call_id: String,
    },
    /// An MCP tool call has ended.
    McpToolCallEnd {
        /// Server name.
        server: String,
        /// Tool name.
        tool: String,
        /// Call identifier.
        call_id: String,
        /// Whether it was an error.
        is_error: bool,
    },
}
