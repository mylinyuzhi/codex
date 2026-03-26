//! Shell tool for executing commands via array format (direct exec, no shell).

use super::prompts;
use crate::context::ToolContext;
use crate::error::Result;
use crate::tool::Tool;
use async_trait::async_trait;
use cocode_protocol::ApprovalRequest;
use cocode_protocol::ConcurrencySafety;
use cocode_protocol::PermissionResult;
use cocode_protocol::ToolOutput;
use serde_json::Value;
use std::process::Stdio;
use tokio::process::Command;

/// Default timeout in seconds.
const DEFAULT_TIMEOUT_SECS: i64 = 120;
/// Maximum timeout in seconds.
const MAX_TIMEOUT_SECS: i64 = 600;

/// Tool for executing commands via array format (direct exec, no shell).
///
/// Unlike [`BashTool`], this tool takes a `Vec<String>` command array
/// and executes it directly without a shell interpreter. This is useful
/// for models that prefer structured command invocation.
pub struct ShellTool;

impl ShellTool {
    /// Create a new Shell tool.
    pub fn new() -> Self {
        Self
    }
}

impl Default for ShellTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for ShellTool {
    fn name(&self) -> &str {
        cocode_protocol::ToolName::Shell.as_str()
    }

    fn description(&self) -> &str {
        prompts::SHELL_DESCRIPTION
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Command as array: [program, arg1, arg2, ...]"
                },
                "timeout": {
                    "type": "integer",
                    "description": "Optional timeout in seconds (max 600)"
                },
                "dangerouslyDisableSandbox": {
                    "type": "boolean",
                    "description": "Set this to true to dangerously override sandbox mode and run commands without sandboxing.",
                    "default": false
                }
            },
            "required": ["command"]
        })
    }

    fn concurrency_safety(&self) -> ConcurrencySafety {
        ConcurrencySafety::Unsafe
    }

    fn is_read_only(&self) -> bool {
        false
    }

    fn max_result_size_chars(&self) -> i32 {
        30_000
    }

    async fn check_permission(&self, input: &Value, ctx: &ToolContext) -> PermissionResult {
        let args = match input.get("command").and_then(|v| v.as_array()) {
            Some(arr) => arr,
            None => return PermissionResult::Passthrough,
        };

        let command_str: String = args
            .iter()
            .filter_map(|v| v.as_str())
            .collect::<Vec<_>>()
            .join(" ");

        if command_str.is_empty() {
            return PermissionResult::Passthrough;
        }

        // Sandbox auto-allow: when sandbox is active and auto-allow enabled,
        // the sandbox itself becomes the security boundary.
        let bypass_requested =
            super::input_helpers::bool_or(input, "dangerouslyDisableSandbox", false);
        if let Some(ref state) = ctx.sandbox_state
            && state.auto_allow_enabled()
            && !bypass_requested
            && state.should_sandbox_command(&command_str, cocode_sandbox::SandboxBypass::No)
        {
            return PermissionResult::Allowed;
        }

        // Plan mode: only allow read-only commands.
        if ctx.is_plan_mode {
            if super::bash::is_plan_mode_allowed(&command_str) {
                return PermissionResult::Allowed;
            }
            return PermissionResult::Denied {
                reason: "Plan mode is active. Only read-only commands are allowed \
                         during planning. Use Read, Glob, and Grep tools to explore \
                         the codebase, or run read-only shell commands (e.g. ls, cat, \
                         grep, git status)."
                    .to_string(),
            };
        }

        // Run security analysis on the joined command string
        let (_, analysis) = cocode_shell_parser::parse_and_analyze(&command_str);

        if analysis.has_risks() {
            // Deny-phase risks → auto-block
            let deny_phase_risks =
                analysis.risks_by_phase(cocode_shell_parser::security::RiskPhase::Deny);
            if !deny_phase_risks.is_empty() {
                let risk_msgs: Vec<String> = deny_phase_risks
                    .iter()
                    .map(|r| format!("{}: {}", r.kind, r.message))
                    .collect();
                return PermissionResult::Denied {
                    reason: format!(
                        "Command blocked due to security risks: {}",
                        risk_msgs.join("; ")
                    ),
                };
            }

            // Ask-phase risks → NeedsApproval with risk details
            let ask_phase_risks =
                analysis.risks_by_phase(cocode_shell_parser::security::RiskPhase::Ask);
            if !ask_phase_risks.is_empty() {
                let risks: Vec<cocode_protocol::SecurityRisk> = ask_phase_risks
                    .iter()
                    .map(|r| super::map_shell_risk(r))
                    .collect();

                return PermissionResult::NeedsApproval {
                    request: ApprovalRequest {
                        request_id: format!("shell-security-{}", uuid::Uuid::new_v4()),
                        tool_name: cocode_protocol::ToolName::Shell.as_str().to_string(),
                        description: cocode_utils_string::truncate_str(&command_str, 120),
                        risks,
                        allow_remember: true,
                        proposed_prefix_pattern: None,
                        input: Some(input.clone()),
                    },
                };
            }
        }

        // Non-trivial command with no detected risks → still needs approval
        let description = cocode_utils_string::truncate_str(&command_str, 120);
        // Annotate when sandbox bypass is active so the user knows this runs unsandboxed
        let description = if bypass_requested && ctx.sandbox_state.is_some() {
            format!("{description} (unsandboxed)")
        } else {
            description
        };
        PermissionResult::NeedsApproval {
            request: ApprovalRequest {
                request_id: format!("shell-cmd-{}", uuid::Uuid::new_v4()),
                tool_name: cocode_protocol::ToolName::Shell.as_str().to_string(),
                description,
                risks: vec![],
                allow_remember: true,
                proposed_prefix_pattern: None,
                input: Some(input.clone()),
            },
        }
    }

    async fn execute(&self, input: Value, ctx: &mut ToolContext) -> Result<ToolOutput> {
        let args: Vec<String> = input["command"]
            .as_array()
            .ok_or_else(|| {
                crate::error::tool_error::InvalidInputSnafu {
                    message: "command must be an array of strings",
                }
                .build()
            })?
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect();

        if args.is_empty() {
            return Err(crate::error::tool_error::InvalidInputSnafu {
                message: "command array must not be empty",
            }
            .build());
        }

        let timeout_secs = input["timeout"]
            .as_i64()
            .unwrap_or(DEFAULT_TIMEOUT_SECS)
            .min(MAX_TIMEOUT_SECS);

        ctx.emit_progress(format!("Executing: {}", args.join(" ")))
            .await;

        // Direct exec — no shell interpreter
        let timeout_duration = std::time::Duration::from_secs(timeout_secs as u64);

        let result = tokio::time::timeout(timeout_duration, async {
            Command::new(&args[0])
                .args(&args[1..])
                .current_dir(&ctx.cwd)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .output()
                .await
        })
        .await;

        let output = match result {
            Ok(Ok(output)) => output,
            Ok(Err(e)) => {
                return Err(crate::error::tool_error::ExecutionFailedSnafu {
                    message: format!("Failed to execute command: {e}"),
                }
                .build());
            }
            Err(_) => {
                return Err(crate::error::tool_error::TimeoutSnafu { timeout_secs }.build());
            }
        };

        let exit_code = output.status.code().unwrap_or(-1);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        let mut text = String::new();
        if !stdout.is_empty() {
            text.push_str(&stdout);
        }
        if !stderr.is_empty() {
            if !text.is_empty() {
                text.push('\n');
            }
            text.push_str("STDERR:\n");
            text.push_str(&stderr);
        }

        super::format_redacted_output(&text, exit_code)
    }
}

#[cfg(test)]
#[path = "shell.test.rs"]
mod tests;
