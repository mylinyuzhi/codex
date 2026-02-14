//! Commands sent from TUI to core agent.
//!
//! These commands represent user actions that need to be communicated
//! to the core agent loop for processing.

use cocode_protocol::ApprovalDecision;
use cocode_protocol::RoleSelection;
use cocode_protocol::SubmissionId;
use cocode_protocol::ThinkingLevel;
use hyper_sdk::ContentBlock;

/// Commands sent from the TUI to the core agent.
///
/// These commands allow the TUI to communicate user intentions
/// to the core agent loop, which will process them accordingly.
#[derive(Debug, Clone)]
pub enum UserCommand {
    /// Submit user input to the agent.
    SubmitInput {
        /// Content blocks (text, images) to send to the agent.
        content: Vec<ContentBlock>,
        /// Original display text (with pills) for chat history.
        display_text: String,
    },

    /// Interrupt the current operation.
    ///
    /// This is typically triggered by Ctrl+C.
    Interrupt,

    /// Set plan mode state.
    SetPlanMode {
        /// Whether plan mode should be active.
        active: bool,
    },

    /// Set the thinking level.
    SetThinkingLevel {
        /// The new thinking level.
        level: ThinkingLevel,
    },

    /// Set the model to use.
    SetModel {
        /// The complete model selection (provider + model + optional thinking level).
        selection: RoleSelection,
    },

    /// Respond to a permission/approval request.
    ApprovalResponse {
        /// The request ID being responded to.
        request_id: String,
        /// The user's three-way decision.
        decision: ApprovalDecision,
    },

    /// Execute a skill command.
    ExecuteSkill {
        /// The skill name (e.g., "commit").
        name: String,
        /// Arguments passed to the skill.
        args: String,
    },

    /// Queue a command for steering injection (Enter during streaming).
    ///
    /// The command is consumed once in the agent loop and injected as a
    /// steering system-reminder that asks the model to address the message.
    QueueCommand {
        /// The prompt to queue.
        prompt: String,
    },

    /// Background all running foreground tasks (Ctrl+B).
    ///
    /// Transitions all foreground subagents to background execution.
    BackgroundAllTasks,

    /// Clear all queued commands.
    ClearQueues,

    /// Set the output style.
    SetOutputStyle {
        /// Style name to activate, or `None` to disable.
        style: Option<String>,
    },

    /// Request graceful shutdown.
    Shutdown,
}

impl UserCommand {
    /// Create a submission with a correlation ID.
    ///
    /// Returns a tuple of (SubmissionId, UserCommand) where the SubmissionId
    /// can be used to correlate events back to this command.
    ///
    /// # Example
    ///
    /// ```
    /// use cocode_tui::UserCommand;
    /// use hyper_sdk::ContentBlock;
    ///
    /// let cmd = UserCommand::SubmitInput {
    ///     content: vec![ContentBlock::text("Hello")],
    ///     display_text: "Hello".to_string(),
    /// };
    /// let (id, cmd) = cmd.with_correlation_id();
    /// // `id` can now be used to track events related to this command
    /// ```
    pub fn with_correlation_id(self) -> (SubmissionId, Self) {
        (SubmissionId::new(), self)
    }

    /// Check if this command triggers a turn (requires correlation tracking).
    ///
    /// Commands that trigger turns should have their events correlated.
    pub fn triggers_turn(&self) -> bool {
        matches!(
            self,
            UserCommand::SubmitInput { .. }
                | UserCommand::ExecuteSkill { .. }
                | UserCommand::QueueCommand { .. }
        )
    }
}

impl std::fmt::Display for UserCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            UserCommand::SubmitInput { display_text, .. } => {
                let preview = if display_text.len() > 20 {
                    format!("{}...", &display_text[..20])
                } else {
                    display_text.clone()
                };
                write!(f, "SubmitInput({preview:?})")
            }
            UserCommand::Interrupt => write!(f, "Interrupt"),
            UserCommand::SetPlanMode { active } => write!(f, "SetPlanMode({active})"),
            UserCommand::SetThinkingLevel { level } => {
                write!(f, "SetThinkingLevel({:?})", level.effort)
            }
            UserCommand::SetModel { selection } => {
                write!(f, "SetModel({})", selection.model)
            }
            UserCommand::ApprovalResponse {
                request_id,
                decision,
            } => {
                write!(f, "ApprovalResponse({request_id}, {decision:?})")
            }
            UserCommand::ExecuteSkill { name, args } => {
                if args.is_empty() {
                    write!(f, "ExecuteSkill({name})")
                } else {
                    write!(f, "ExecuteSkill({name}, args={args})")
                }
            }
            UserCommand::QueueCommand { prompt } => {
                let preview = if prompt.len() > 20 {
                    format!("{}...", &prompt[..20])
                } else {
                    prompt.clone()
                };
                write!(f, "QueueCommand({preview:?})")
            }
            UserCommand::BackgroundAllTasks => write!(f, "BackgroundAllTasks"),
            UserCommand::ClearQueues => write!(f, "ClearQueues"),
            UserCommand::SetOutputStyle { style } => match style {
                Some(s) => write!(f, "SetOutputStyle({s})"),
                None => write!(f, "SetOutputStyle(off)"),
            },
            UserCommand::Shutdown => write!(f, "Shutdown"),
        }
    }
}

#[cfg(test)]
#[path = "command.test.rs"]
mod tests;
