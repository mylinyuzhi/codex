//! Bash action implementation
//!
//! Executes external bash scripts as hook actions, following Claude Code's protocol.

use super::{HookAction, HookActionError};
use crate::context::HookContext;
use crate::decision::{HookDecision, HookEffect, HookResult, LogLevel};
use async_trait::async_trait;
use codex_protocol::hooks as protocol;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use tokio::time::timeout;

/// Bash action that executes external scripts
///
/// # Protocol
///
/// - **Input**: JSON (HookEventContext) via stdin
/// - **Output**: JSON (HookOutput) via stdout, OR exit code
/// - **Exit codes**:
///   - 0: Success (continue)
///   - 2: Block operation
///   - Other: Error
#[derive(Debug, Clone)]
pub struct BashAction {
    command: String,
    timeout_ms: u64,
}

impl BashAction {
    pub fn new(command: String, timeout_ms: u64) -> Self {
        Self {
            command,
            timeout_ms,
        }
    }
}

#[async_trait]
impl HookAction for BashAction {
    async fn execute(&self, ctx: &HookContext) -> Result<HookResult, HookActionError> {
        let duration = Duration::from_millis(self.timeout_ms);

        // Spawn bash process
        let mut child = Command::new("bash")
            .arg("-c")
            .arg(&self.command)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| HookActionError::ExecutionFailed(e.to_string()))?;

        // Send JSON input (Claude Code format)
        let input_json = serde_json::to_string(&ctx.event)
            .map_err(|e| HookActionError::ParseError(e.to_string()))?;

        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(input_json.as_bytes())
                .await
                .map_err(|e| HookActionError::ExecutionFailed(e.to_string()))?;
        }

        // Wait for completion with timeout
        let result = timeout(duration, child.wait_with_output())
            .await
            .map_err(|_| HookActionError::Timeout(self.timeout_ms))?
            .map_err(|e| HookActionError::ExecutionFailed(e.to_string()))?;

        // Parse output
        self.parse_output(result)
    }

    fn description(&self) -> String {
        format!("bash: {}", self.command)
    }

    fn is_parallelizable(&self) -> bool {
        true // Bash scripts are generally safe to run in parallel
    }
}

impl BashAction {
    fn parse_output(
        &self,
        output: std::process::Output,
    ) -> Result<HookResult, HookActionError> {
        let exit_code = output.status.code().unwrap_or(-1);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        match exit_code {
            0 => {
                // Success: try to parse JSON output
                let stdout_trimmed = stdout.trim();
                if !stdout_trimmed.is_empty() {
                    if let Ok(hook_output) = serde_json::from_str::<protocol::HookOutput>(stdout_trimmed) {
                        return Ok(self.convert_hook_output(hook_output));
                    }
                }

                // No JSON output, default to continue
                Ok(HookResult::continue_with(vec![HookEffect::Log {
                    level: LogLevel::Debug,
                    message: format!("Hook script output: {}", stdout_trimmed),
                }]))
            }
            2 => {
                // Exit code 2: block operation
                let reason = if !stderr.is_empty() {
                    stderr.trim().to_string()
                } else if !stdout.is_empty() {
                    stdout.trim().to_string()
                } else {
                    "Hook blocked execution".to_string()
                };
                Ok(HookResult::abort(reason))
            }
            _ => {
                // Other exit codes: error
                Err(HookActionError::ExecutionFailed(format!(
                    "Exit code {}: {}",
                    exit_code,
                    if !stderr.is_empty() {
                        stderr.trim()
                    } else {
                        "no error message"
                    }
                )))
            }
        }
    }

    fn convert_hook_output(&self, output: protocol::HookOutput) -> HookResult {
        // Convert Claude Code HookOutput to internal HookResult

        // Determine decision
        let decision = if !output.continue_execution {
            HookDecision::Abort {
                reason: output
                    .reason
                    .unwrap_or_else(|| "Hook blocked execution".to_string()),
            }
        } else {
            match output.decision {
                Some(protocol::HookDecision::Block) | Some(protocol::HookDecision::Deny) => {
                    HookDecision::Abort {
                        reason: output
                            .reason
                            .unwrap_or_else(|| "Blocked by hook".to_string()),
                    }
                }
                Some(protocol::HookDecision::Ask) => HookDecision::AskUser {
                    prompt: output
                        .reason
                        .unwrap_or_else(|| "Confirmation required".to_string()),
                },
                _ => HookDecision::Continue,
            }
        };

        // Convert effects
        let mut effects = vec![];

        if let Some(msg) = output.system_message {
            effects.push(HookEffect::Log {
                level: LogLevel::Info,
                message: msg,
            });
        }

        if let Some(ctx) = output.additional_context {
            effects.push(HookEffect::AddMetadata {
                key: "additional_context".to_string(),
                value: serde_json::Value::String(ctx),
            });
        }

        if let Some(specific) = output.hook_specific_output {
            effects.push(HookEffect::AddMetadata {
                key: "hook_specific_output".to_string(),
                value: specific,
            });
        }

        HookResult { decision, effects }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codex_protocol::hooks::{HookEventContext, HookEventData, HookEventName};

    fn make_test_context() -> HookContext {
        let event = HookEventContext {
            session_id: "test-123".to_string(),
            transcript_path: None,
            cwd: "/tmp".to_string(),
            hook_event_name: HookEventName::PreToolUse,
            timestamp: "2025-01-01T00:00:00Z".to_string(),
            event_data: HookEventData::PreToolUse {
                tool_name: "test_tool".to_string(),
                tool_input: serde_json::json!({"arg": "value"}),
            },
        };
        HookContext::new(event)
    }

    #[tokio::test]
    async fn test_bash_action_success() {
        let action = BashAction::new("echo 'test'".to_string(), 5000);
        let ctx = make_test_context();

        let result = action.execute(&ctx).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_bash_action_block() {
        let action = BashAction::new("exit 2".to_string(), 5000);
        let ctx = make_test_context();

        let result = action.execute(&ctx).await.unwrap();
        assert!(matches!(result.decision, HookDecision::Abort { .. }));
    }

    #[tokio::test]
    async fn test_bash_action_json_output() {
        let script = r#"echo '{"continue": false, "decision": "block", "reason": "Test block"}'"#;
        let action = BashAction::new(script.to_string(), 5000);
        let ctx = make_test_context();

        let result = action.execute(&ctx).await.unwrap();
        match result.decision {
            HookDecision::Abort { reason } => assert_eq!(reason, "Test block"),
            _ => panic!("Expected Abort decision"),
        }
    }

    #[tokio::test]
    async fn test_bash_action_timeout() {
        let action = BashAction::new("sleep 10".to_string(), 100);
        let ctx = make_test_context();

        let result = action.execute(&ctx).await;
        assert!(matches!(result, Err(HookActionError::Timeout(100))));
    }
}
