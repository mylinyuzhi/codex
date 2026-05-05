//! Prompt-type slash commands.
//!
//! TS source: `types/command.ts PromptCommand` + commands like
//! `commands/security-review.ts`, `commands/insights.ts`, `commands/brief.ts`,
//! `commands/commit-push-pr.ts` that all return
//! `[{type:'text', text: PROMPT_BODY}]` from `getPromptForCommand`.
//!
//! These commands don't run code locally — they push a prompt back into the
//! agent loop, which then drives subsequent tool calls.

use async_trait::async_trait;

use crate::CommandHandler;
use crate::CommandResult;
use crate::PromptPart;

/// Handler that returns a static prompt text wrapped in
/// `CommandResult::Prompt`. Optionally appends user-provided arguments.
pub struct StaticPromptHandler {
    pub name: &'static str,
    pub progress_message: &'static str,
    pub body: &'static str,
    /// When true, append `## Task\n\n<args>` to the body when args are given
    /// (matches TS pattern in security-review/insights/etc.).
    pub append_task: bool,
}

impl StaticPromptHandler {
    pub const fn new(
        name: &'static str,
        progress_message: &'static str,
        body: &'static str,
    ) -> Self {
        Self {
            name,
            progress_message,
            body,
            append_task: false,
        }
    }

    pub const fn with_task_append(
        name: &'static str,
        progress_message: &'static str,
        body: &'static str,
    ) -> Self {
        Self {
            name,
            progress_message,
            body,
            append_task: true,
        }
    }
}

#[async_trait]
impl CommandHandler for StaticPromptHandler {
    async fn execute_command(&self, args: &str) -> anyhow::Result<CommandResult> {
        let mut text = self.body.to_string();
        if self.append_task && !args.trim().is_empty() {
            text.push_str("\n\n## Task\n\n");
            text.push_str(args);
        }
        Ok(CommandResult::Prompt {
            progress_message: self.progress_message.to_string(),
            parts: vec![PromptPart::Text { text }],
        })
    }

    fn handler_name(&self) -> &str {
        self.name
    }
}
