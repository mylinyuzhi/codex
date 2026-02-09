use serde::Deserialize;
use serde::Serialize;

/// Describes why the agent loop stopped.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StopReason {
    /// The loop exhausted its maximum turn budget.
    MaxTurnsReached,

    /// The model emitted an explicit stop signal (end_turn, stop, etc.).
    ModelStopSignal,

    /// The user cancelled the loop (e.g. Ctrl-C).
    UserInterrupted,

    /// The loop terminated due to an error.
    Error {
        /// Human-readable error description.
        message: String,
    },

    /// The loop exited plan mode.
    PlanModeExit {
        /// Whether the plan was approved by the user.
        approved: bool,
    },

    /// A hook requested the loop to stop.
    HookStopped,
}

/// Aggregate result of a completed agent loop run.
#[derive(Debug, Clone)]
pub struct LoopResult {
    /// The reason the loop stopped.
    pub stop_reason: StopReason,

    /// Total number of turns completed.
    pub turns_completed: i32,

    /// Cumulative input tokens consumed across all turns.
    pub total_input_tokens: i32,

    /// Cumulative output tokens generated across all turns.
    pub total_output_tokens: i32,

    /// Final text response from the model (last assistant message text).
    pub final_text: String,

    /// All content blocks from the last response.
    pub last_response_content: Vec<hyper_sdk::ContentBlock>,
}

impl LoopResult {
    /// Create a result for model stop signal.
    pub fn completed(
        turns: i32,
        input_tokens: i32,
        output_tokens: i32,
        text: String,
        content: Vec<hyper_sdk::ContentBlock>,
    ) -> Self {
        Self {
            stop_reason: StopReason::ModelStopSignal,
            turns_completed: turns,
            total_input_tokens: input_tokens,
            total_output_tokens: output_tokens,
            final_text: text,
            last_response_content: content,
        }
    }

    /// Create a result for max turns reached.
    pub fn max_turns_reached(turns: i32, input_tokens: i32, output_tokens: i32) -> Self {
        Self {
            stop_reason: StopReason::MaxTurnsReached,
            turns_completed: turns,
            total_input_tokens: input_tokens,
            total_output_tokens: output_tokens,
            final_text: String::new(),
            last_response_content: Vec::new(),
        }
    }

    /// Create a result for hook stop.
    pub fn hook_stopped(turns: i32, input_tokens: i32, output_tokens: i32) -> Self {
        Self {
            stop_reason: StopReason::HookStopped,
            turns_completed: turns,
            total_input_tokens: input_tokens,
            total_output_tokens: output_tokens,
            final_text: String::new(),
            last_response_content: Vec::new(),
        }
    }

    /// Create a result for user interruption.
    pub fn interrupted(turns: i32, input_tokens: i32, output_tokens: i32) -> Self {
        Self {
            stop_reason: StopReason::UserInterrupted,
            turns_completed: turns,
            total_input_tokens: input_tokens,
            total_output_tokens: output_tokens,
            final_text: String::new(),
            last_response_content: Vec::new(),
        }
    }

    /// Create a result for an error.
    pub fn error(turns: i32, input_tokens: i32, output_tokens: i32, message: String) -> Self {
        Self {
            stop_reason: StopReason::Error { message },
            turns_completed: turns,
            total_input_tokens: input_tokens,
            total_output_tokens: output_tokens,
            final_text: String::new(),
            last_response_content: Vec::new(),
        }
    }

    /// Create a result for plan mode exit.
    pub fn plan_mode_exit(
        turns: i32,
        input_tokens: i32,
        output_tokens: i32,
        approved: bool,
        content: Vec<hyper_sdk::ContentBlock>,
    ) -> Self {
        // Extract text from content blocks
        let text: String = content
            .iter()
            .filter_map(|b| match b {
                hyper_sdk::ContentBlock::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        Self {
            stop_reason: StopReason::PlanModeExit { approved },
            turns_completed: turns,
            total_input_tokens: input_tokens,
            total_output_tokens: output_tokens,
            final_text: text,
            last_response_content: content,
        }
    }
}

#[cfg(test)]
#[path = "result.test.rs"]
mod tests;
