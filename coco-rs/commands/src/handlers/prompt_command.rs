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
    async fn execute_command(&self, args: &str) -> crate::Result<CommandResult> {
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

/// Handler that pre-resolves `!`<shell-cmd>`` markers in the prompt body
/// before sending to the model. Mirrors TS
/// `utils/promptShellExecution.ts::executeShellCommandsInPrompt`.
///
/// Each `!`cmd`` token (backticks around the command) is replaced with the
/// captured stdout of running `cmd` through `bash -c`. A failing command
/// is replaced with its stderr prefixed by `(error: ...)` so the model
/// still sees something useful and can continue.
///
/// Used by `/security-review` and any other Prompt command whose TS source
/// originally went through `executeShellCommandsInPrompt`.
pub struct ShellExpandingPromptHandler {
    pub name: &'static str,
    pub progress_message: &'static str,
    pub body: &'static str,
    /// When true, append `## Task\n\n<args>` (matches TS append-task pattern).
    pub append_task: bool,
}

impl ShellExpandingPromptHandler {
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
}

#[async_trait]
impl CommandHandler for ShellExpandingPromptHandler {
    async fn execute_command(&self, args: &str) -> crate::Result<CommandResult> {
        let mut text = expand_shell_markers(self.body).await;
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

/// Expand TS-style `!\`cmd\`` markers in `body` by running each command
/// through `bash -c` and substituting captured stdout. Errors are
/// inlined as `(error: ...)` so the prompt still produces something
/// the model can act on.
async fn expand_shell_markers(body: &str) -> String {
    let mut out = String::with_capacity(body.len());
    let mut rest = body;
    while let Some(start) = rest.find("!`") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        match after.find('`') {
            Some(end) => {
                let cmd = &after[..end];
                let stdout = run_shell(cmd).await;
                out.push_str(&stdout);
                rest = &after[end + 1..];
            }
            None => {
                // Unterminated marker — leave as-is.
                out.push_str(&rest[start..]);
                rest = "";
                break;
            }
        }
    }
    out.push_str(rest);
    out
}

async fn run_shell(cmd: &str) -> String {
    match tokio::process::Command::new("bash")
        .arg("-c")
        .arg(cmd)
        .output()
        .await
    {
        Ok(output) if output.status.success() => String::from_utf8_lossy(&output.stdout)
            .trim_end()
            .to_string(),
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr)
                .trim_end()
                .to_string();
            format!("(error: {stderr})")
        }
        Err(e) => format!("(error: {e})"),
    }
}

#[cfg(test)]
#[path = "prompt_command.test.rs"]
mod tests;
