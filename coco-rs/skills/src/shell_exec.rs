//! Shell command execution in skill prompts.
//!
//! TS: executeShellCommandsInPrompt() in promptShellExecution.ts --
//! executes `$(shell commands)` embedded in skill markdown content
//! before sending to the model.
//!
//! Supports inline `$(command)` patterns -- replaced with stdout.

use std::time::Duration;

/// Default timeout for shell commands in skill prompts.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

/// Execute shell commands embedded in skill prompt content.
///
/// Replaces `$(command)` patterns with command stdout.
///
/// Returns the content with shell commands replaced by their output.
///
/// # Security
/// - MCP-sourced skills skip shell execution entirely (TS parity)
/// - Commands inherit the current working directory
/// - Timeout prevents hanging commands
pub async fn execute_shell_in_prompt(content: &str, skip_shell: bool) -> String {
    if skip_shell {
        return content.to_string();
    }

    // Pattern: $(command) -- inline shell execution
    // Find all $(…) patterns, being careful about nesting
    replace_inline_shell(content).await
}

/// Replace `$(command)` patterns with their stdout.
async fn replace_inline_shell(content: &str) -> String {
    let mut result = String::with_capacity(content.len());
    let mut chars = content.char_indices().peekable();

    while let Some((i, ch)) = chars.next() {
        if ch == '$'
            && let Some(&(_, '(')) = chars.peek()
        {
            chars.next(); // consume '('
            // Find matching closing paren (respecting nesting)
            let mut depth = 1;
            let start = i + 2;
            let mut end = start;
            for (j, c) in chars.by_ref() {
                match c {
                    '(' => depth += 1,
                    ')' => {
                        depth -= 1;
                        if depth == 0 {
                            end = j;
                            break;
                        }
                    }
                    _ => {}
                }
            }
            if depth == 0 {
                let command = &content[start..end];
                let output = run_shell_command(command).await;
                result.push_str(&output);
                continue;
            }
            // Unmatched paren -- output literally
            result.push_str(&content[i..=start]);
            continue;
        }
        result.push(ch);
    }

    result
}

/// Run a shell command and return its trimmed stdout.
async fn run_shell_command(command: &str) -> String {
    let output = tokio::time::timeout(
        DEFAULT_TIMEOUT,
        tokio::process::Command::new("sh")
            .arg("-c")
            .arg(command)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .output(),
    )
    .await;

    match output {
        Ok(Ok(out)) if out.status.success() => {
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        }
        Ok(Ok(out)) => {
            tracing::warn!(
                "shell command in skill prompt failed (exit {}): {command}",
                out.status.code().unwrap_or(-1)
            );
            String::new()
        }
        Ok(Err(e)) => {
            tracing::warn!("failed to spawn shell command in skill prompt: {e}");
            String::new()
        }
        Err(_) => {
            tracing::warn!("shell command in skill prompt timed out: {command}");
            String::new()
        }
    }
}

#[cfg(test)]
#[path = "shell_exec.test.rs"]
mod tests;
