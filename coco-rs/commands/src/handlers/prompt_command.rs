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

/// How a Prompt-type command should incorporate the user-supplied
/// `args` into the static body.
///
/// Replaces the prior `bool append_task` flag — CLAUDE.md style guide
/// flags `bool` parameters when callsites would read as opaque
/// literals (`register_static_prompt(..., true)`). The enum makes the
/// behaviour explicit at every callsite.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArgsHandling {
    /// No args manipulation — body is emitted verbatim regardless of
    /// `args`. Used by static prompts that never expect args
    /// (`/statusline`).
    Static,
    /// Append `\n\n## Task\n\n<args>` when args are non-empty.
    /// Matches TS pattern in `/security-review`, `/insights`,
    /// `/pr-comments`.
    AppendUnderTask,
    /// Always emit `\n<prefix><args>` at the body's end. `args` may be
    /// empty — TS pattern in `/review`: ``PR number: ${args}`` is
    /// included even when no PR number was given, so the model sees
    /// an explicit empty value rather than the line being absent.
    AppendInline { prefix: &'static str },
}

/// Handler that returns a static prompt text wrapped in
/// `CommandResult::Prompt`. The supplied [`ArgsHandling`] decides how
/// `args` are folded into the body.
pub struct StaticPromptHandler {
    pub name: String,
    pub progress_message: String,
    pub body: String,
    pub args_handling: ArgsHandling,
}

impl StaticPromptHandler {
    pub fn new(
        name: impl Into<String>,
        progress_message: impl Into<String>,
        body: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            progress_message: progress_message.into(),
            body: body.into(),
            args_handling: ArgsHandling::Static,
        }
    }

    pub fn with_task_append(
        name: impl Into<String>,
        progress_message: impl Into<String>,
        body: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            progress_message: progress_message.into(),
            body: body.into(),
            args_handling: ArgsHandling::AppendUnderTask,
        }
    }

    pub fn with_inline_append(
        name: impl Into<String>,
        progress_message: impl Into<String>,
        body: impl Into<String>,
        prefix: &'static str,
    ) -> Self {
        Self {
            name: name.into(),
            progress_message: progress_message.into(),
            body: body.into(),
            args_handling: ArgsHandling::AppendInline { prefix },
        }
    }
}

#[async_trait]
impl CommandHandler for StaticPromptHandler {
    async fn execute_command(&self, args: &str) -> crate::Result<CommandResult> {
        let mut text = self.body.clone();
        match self.args_handling {
            ArgsHandling::Static => {}
            ArgsHandling::AppendUnderTask => {
                if !args.trim().is_empty() {
                    text.push_str("\n\n## Task\n\n");
                    text.push_str(args);
                }
            }
            ArgsHandling::AppendInline { prefix } => {
                // TS emits the prefix line unconditionally — even when
                // args is empty — so the model gets an explicit blank
                // value rather than an absent line.
                text.push('\n');
                text.push_str(prefix);
                text.push_str(args);
            }
        }
        Ok(CommandResult::Prompt {
            progress_message: self.progress_message.clone(),
            parts: vec![PromptPart::Text { text }],
        })
    }

    fn handler_name(&self) -> &str {
        &self.name
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
    pub name: String,
    pub progress_message: String,
    pub body: String,
    /// How `args` are folded into the body. See [`ArgsHandling`].
    pub args_handling: ArgsHandling,
}

impl ShellExpandingPromptHandler {
    pub fn new(
        name: impl Into<String>,
        progress_message: impl Into<String>,
        body: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            progress_message: progress_message.into(),
            body: body.into(),
            args_handling: ArgsHandling::Static,
        }
    }
}

#[async_trait]
impl CommandHandler for ShellExpandingPromptHandler {
    async fn execute_command(&self, args: &str) -> crate::Result<CommandResult> {
        let mut text = expand_shell_markers(&self.body).await;
        match self.args_handling {
            ArgsHandling::Static => {}
            ArgsHandling::AppendUnderTask => {
                if !args.trim().is_empty() {
                    text.push_str("\n\n## Task\n\n");
                    text.push_str(args);
                }
            }
            ArgsHandling::AppendInline { prefix } => {
                text.push('\n');
                text.push_str(prefix);
                text.push_str(args);
            }
        }
        Ok(CommandResult::Prompt {
            progress_message: self.progress_message.clone(),
            parts: vec![PromptPart::Text { text }],
        })
    }

    fn handler_name(&self) -> &str {
        &self.name
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
