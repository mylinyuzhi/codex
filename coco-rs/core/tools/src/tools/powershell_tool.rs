//! PowerShellTool — security-gated pwsh execution.
//!
//! R5-T9: rounds 1-4 shipped a bare `PowerShellTool` that just spawned
//! `pwsh` with the raw command; the full security pipeline living in
//! `core/tools/src/tools/powershell.rs` (CLM type gate, git-internal-path
//! guard, UNC-path guard, UTF-16 output decode) was dead code. This
//! module re-houses the tool and wires every helper into the execute
//! path so PowerShell behaves like Bash: a permission gate, a read-only
//! fast path, and a destructive warning phase before the child is ever
//! spawned.
//!
//! TS: `tools/PowerShellTool/PowerShellTool.tsx`, `powershellSecurity.ts`,
//! `powershellPermissions.ts`, `clmTypes.ts`.

use coco_messages::ToolResult;
use coco_sandbox::SandboxBypass;
use coco_sandbox::SandboxState;
use coco_tool_runtime::DescriptionOptions;
use coco_tool_runtime::Tool;
use coco_tool_runtime::ToolError;
use coco_tool_runtime::ToolResultContentPart;
use coco_tool_runtime::ToolUseContext;
use coco_tool_runtime::ValidationResult;
use coco_types::ToolId;
use coco_types::ToolInputSchema;
use coco_types::ToolName;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

use super::powershell::analyze_ps_security;
use super::powershell::classify_ps_command;
use super::powershell::decode_ps_output;
use super::powershell::is_vulnerable_unc_path;

/// Default pwsh command timeout (2 minutes) — matches Bash default.
const DEFAULT_TIMEOUT_MS: u64 = 120_000;

/// Max pwsh command timeout (10 minutes) — matches Bash max.
const MAX_TIMEOUT_MS: u64 = 600_000;

pub struct PowerShellTool;

#[async_trait::async_trait]
impl Tool for PowerShellTool {
    fn id(&self) -> ToolId {
        ToolId::Builtin(ToolName::PowerShell)
    }

    fn name(&self) -> &str {
        ToolName::PowerShell.as_str()
    }

    fn description(&self, _: &Value, _options: &DescriptionOptions) -> String {
        "Execute a PowerShell command via pwsh. Subject to CLM type allowlist \
         and git-internal-path safety checks — unsafe commands are rejected \
         without running."
            .into()
    }

    fn input_schema(&self) -> ToolInputSchema {
        let mut p = HashMap::new();
        p.insert(
            "command".into(),
            serde_json::json!({
                "type": "string",
                "description": "The PowerShell command to execute"
            }),
        );
        p.insert(
            "timeout".into(),
            serde_json::json!({
                "type": "number",
                "description": "Optional timeout in milliseconds. Defaults to 120000 (2 min) \
                                and cannot exceed 600000 (10 min)."
            }),
        );
        p.insert(
            "run_in_background".into(),
            serde_json::json!({
                "type": "boolean",
                "description": "Set to true to run this command in the background. \
                                Returns immediately with a task_id."
            }),
        );
        // R7-T23: TS `PowerShellTool.tsx` exposes the same
        // `dangerouslyDisableSandbox` opt-out as BashTool. Without
        // this field the schema rejected legitimate uses where the
        // user explicitly approved sandbox bypass for a specific
        // PowerShell command.
        p.insert(
            "dangerouslyDisableSandbox".into(),
            serde_json::json!({
                "type": "boolean",
                "description": "Set this to true to dangerously override sandbox mode and run commands without sandboxing."
            }),
        );
        ToolInputSchema { properties: p }
    }

    /// Mirror Bash's read-only fast path. TS
    /// `isSearchOrReadPowerShellCommand` (`readOnlyValidation.ts`) runs
    /// the same classifier; a command classified as search/read is
    /// concurrency-safe and skips the user-approval flow upstream.
    fn is_read_only(&self, input: &Value) -> bool {
        let Some(cmd) = input.get("command").and_then(|v| v.as_str()) else {
            return false;
        };
        let (is_search, is_read) = classify_ps_command(cmd);
        is_search || is_read
    }

    fn is_concurrency_safe(&self, input: &Value) -> bool {
        self.is_read_only(input)
    }

    fn is_destructive(&self, input: &Value) -> bool {
        !self.is_read_only(input)
    }

    fn should_defer(&self) -> bool {
        true
    }
    fn search_hint(&self) -> Option<&str> {
        Some("run pwsh PowerShell commands on Windows")
    }

    fn get_activity_description(&self, input: &Value) -> Option<String> {
        let command = input.get("command").and_then(|v| v.as_str())?;
        let truncated: String = command.chars().take(57).collect();
        Some(if truncated.len() < command.len() {
            format!("Running pwsh {truncated}...")
        } else {
            format!("Running pwsh {command}")
        })
    }

    /// TS `maxResultSizeChars: 30_000` at `PowerShellTool.tsx:275`.
    fn max_result_size_chars(&self) -> i64 {
        30_000
    }

    fn validate_input(&self, input: &Value, _ctx: &ToolUseContext) -> ValidationResult {
        if input.get("command").and_then(|v| v.as_str()).is_none() {
            return ValidationResult::invalid("missing required field: command");
        }
        if let Some(timeout) = input.get("timeout").and_then(serde_json::Value::as_u64)
            && timeout > MAX_TIMEOUT_MS
        {
            return ValidationResult::invalid(format!(
                "timeout must not exceed {MAX_TIMEOUT_MS}ms"
            ));
        }
        ValidationResult::Valid
    }

    /// Render the PowerShell envelope. TS parity:
    /// `PowerShellTool.tsx:383-435 mapToolResultToToolResultBlockParam`.
    ///
    /// Branches mirror Bash's render so future fg→bg promotion / oversize
    /// stdout persistence wiring requires only execute-side changes:
    /// 1. **Status==background** (user-initiated `run_in_background:true`):
    ///    emit prebuilt `message` field.
    /// 2. **Foreground**: build `[processedStdout, errorMessage,
    ///    backgroundInfo]` joined with `\n`, skipping empties.
    ///    `processedStdout` strips leading blank lines + trims trailing
    ///    whitespace; `persistedOutputPath` swaps it for a
    ///    `<persisted-output>` envelope. `backgroundTaskId` triggers one
    ///    of three messages (`assistantAutoBackgrounded` /
    ///    `backgroundedByUser` / default).
    ///
    /// The `isImage` branch (TS:395-398) is intentionally unimplemented
    /// because `execute_foreground` decodes UTF-16 stdout into a UTF-8
    /// string before the data envelope is built — image bytes would be
    /// mangled by that decode. Wire image detection into the execute path
    /// (emit `structuredContent`) before adding the render branch.
    fn render_for_model(&self, data: &Value) -> Vec<ToolResultContentPart> {
        if data
            .get("status")
            .and_then(Value::as_str)
            .is_some_and(|s| s == "background")
        {
            let msg = data
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("PowerShell command running in background.");
            return vec![ToolResultContentPart::Text {
                text: msg.to_string(),
                provider_options: None,
            }];
        }

        let stdout = data.get("stdout").and_then(Value::as_str).unwrap_or("");
        let stderr = data.get("stderr").and_then(Value::as_str).unwrap_or("");
        let interrupted = data
            .get("interrupted")
            .and_then(Value::as_bool)
            .unwrap_or(false);

        let mut processed = super::shell_render::strip_leading_blank_lines(stdout)
            .trim_end()
            .to_string();
        if let Some(path) = data.get("persistedOutputPath").and_then(Value::as_str) {
            let original_size = data
                .get("persistedOutputSize")
                .and_then(Value::as_u64)
                .unwrap_or(0) as usize;
            processed = super::shell_render::build_persisted_output_message(
                path,
                original_size,
                &processed,
            );
        }

        let mut error_message = stderr.trim().to_string();
        if interrupted {
            if !error_message.is_empty() {
                error_message.push('\n');
            }
            error_message.push_str("<error>Command was aborted before completion</error>");
        }

        let background_info = data
            .get("backgroundTaskId")
            .and_then(Value::as_str)
            .map(|task_id| {
                let auto = data
                    .get("assistantAutoBackgrounded")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                let by_user = data
                    .get("backgroundedByUser")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                if auto {
                    let budget_seconds =
                        super::bash_advanced::ASSISTANT_BLOCKING_BUDGET_MS / 1000;
                    format!(
                        "Command exceeded the assistant-mode blocking budget ({budget_seconds}s) and was moved to the background with ID: {task_id}. It is still running — you will be notified when it completes. Output is being written to the task output. In assistant mode, delegate long-running work to a subagent or use run_in_background to keep this conversation responsive."
                    )
                } else if by_user {
                    format!(
                        "Command was manually backgrounded by user with ID: {task_id}. Output is being written to the task output."
                    )
                } else {
                    format!(
                        "Command running in background with ID: {task_id}. Output is being written to the task output."
                    )
                }
            })
            .unwrap_or_default();

        let combined = [
            processed.as_str(),
            error_message.as_str(),
            background_info.as_str(),
        ]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<&str>>()
        .join("\n");

        vec![ToolResultContentPart::Text {
            text: combined,
            provider_options: None,
        }]
    }

    async fn execute(
        &self,
        input: Value,
        ctx: &ToolUseContext,
    ) -> Result<ToolResult<Value>, ToolError> {
        let command = input
            .get("command")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidInput {
                message: "missing command".into(),
                error_code: None,
            })?;

        // ── Stage 1: CLM type + git-internal guard ──
        //
        // TS `powershellCommandIsSafe()` runs before the child is spawned;
        // commands using .NET types outside the CLM allowlist or writing
        // to `.git/{hooks,refs,objects,HEAD,config}` are rejected hard.
        // Read-only commands skip this gate because a harmless
        // `Get-Content ./x` must not be blocked because it mentions
        // `[IO.File]::ReadAllText(...)` in a string literal.
        let (is_search, is_read) = classify_ps_command(command);
        if !(is_search || is_read) {
            let result = analyze_ps_security(command);
            if !result.is_safe {
                let reason = result
                    .reason
                    .unwrap_or_else(|| "PowerShell security check failed".into());
                return Err(ToolError::PermissionDenied {
                    message: format!(
                        "Command blocked by coco-rs PowerShell security gate: {reason}. \
                         If you believe this is a false positive, restructure the command."
                    ),
                });
            }
        }

        // ── Stage 2: UNC path guard ──
        //
        // UNC paths (`\\server\share\...`) in command arguments can be
        // used for NTLM credential leakage. TS
        // `tools/PowerShellTool/pathValidation.ts` rejects any non-
        // whitelisted UNC path before execution.
        for token in command.split_ascii_whitespace() {
            if is_vulnerable_unc_path(token) {
                return Err(ToolError::PermissionDenied {
                    message: format!(
                        "Command contains UNC path `{token}` which can leak NTLM \
                         credentials. Reject per PowerShell path validation."
                    ),
                });
            }
        }

        let run_in_background = input
            .get("run_in_background")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);

        if run_in_background {
            return execute_background(command, &input, ctx).await;
        }

        // Sandbox decision parity with Bash. Resolve the active state +
        // bypass flag here; the foreground helper applies the platform
        // wrap before spawning pwsh.
        let sandbox_state = if ctx.features.enabled(coco_types::Feature::Sandbox) {
            ctx.sandbox_state.clone()
        } else {
            None
        };
        let sandbox_bypass = SandboxBypass::from_flag(
            input
                .get("dangerouslyDisableSandbox")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false),
        );

        execute_foreground(command, &input, ctx, sandbox_state, sandbox_bypass).await
    }
}

/// Spawn the command as a background task via `task_handle`.
async fn execute_background(
    command: &str,
    input: &Value,
    ctx: &ToolUseContext,
) -> Result<ToolResult<Value>, ToolError> {
    let task_handle = ctx
        .task_handle
        .as_ref()
        .ok_or_else(|| ToolError::ExecutionFailed {
            message: "Background task execution is not available in this context.".into(),
            source: None,
        })?;

    // Wrap the command in the same pwsh invocation we use for
    // foreground. The task handle runs the shell for us; we just feed
    // the wrapped command string through.
    let wrapped = format!("pwsh -NoProfile -NonInteractive -Command {command:?}");
    let task_id = task_handle
        .spawn_shell_task(coco_tool_runtime::BackgroundShellRequest {
            command: wrapped,
            timeout_ms: input.get("timeout").and_then(serde_json::Value::as_i64),
            description: Some("PowerShell background task".into()),
        })
        .await
        .map_err(|e| ToolError::ExecutionFailed {
            message: format!("Failed to spawn background task: {e}"),
            source: None,
        })?;

    Ok(ToolResult {
        data: serde_json::json!({
            "task_id": task_id,
            "status": "background",
            "message": format!(
                "PowerShell command running in background. Task ID: {task_id}. \
                 You will be notified when it completes."
            ),
        }),
        new_messages: vec![],
        app_state_patch: None,
        permission_updates: Vec::new(),
    })
}

/// Foreground execution with UTF-16 output decode and optional sandbox wrap.
///
/// Goes through [`coco_shell::ShellExecutor`] + [`coco_shell::PowerShellProvider`]
/// so cancel-token, timeout, sandbox-wrap, and CWD tracking all behave
/// the same way they do for `BashTool`. The provider's `build_exec_command`
/// emits `-EncodedCommand <base64-utf16le>` when sandboxed (to dodge the
/// sandbox-runtime's shellquote layer corrupting `!`/`$`/`?`); we get
/// `stdout_bytes` / `stderr_bytes` back so [`decode_ps_output`] can
/// recover the original UTF-16 BOM-prefixed text without going through
/// the lossy String conversion the executor applies for display.
async fn execute_foreground(
    command: &str,
    input: &Value,
    ctx: &ToolUseContext,
    sandbox_state: Option<Arc<SandboxState>>,
    sandbox_bypass: SandboxBypass,
) -> Result<ToolResult<Value>, ToolError> {
    let timeout_ms = input
        .get("timeout")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(DEFAULT_TIMEOUT_MS)
        .min(MAX_TIMEOUT_MS);

    // 4-tier cwd resolution. Spawn at live session cwd; reset guard
    // runs AFTER exec to match TS `PowerShellTool.tsx:520-525` —
    // annotation lands on the offending command's stderr.
    let cwd = crate::tools::shell_cwd::resolve_spawn_cwd(ctx).await;

    // Build a per-call pwsh provider. PowerShell isn't currently
    // session-scoped (no snapshot/session-env story for pwsh) — a fresh
    // provider per call is cheap and isolates `/env` state correctly.
    // Note this is independent of cwd persistence, which uses the same
    // session-cwd plumbing as bash above.
    let pwsh_shell = coco_shell::get_shell(coco_shell::ShellType::PowerShell, None).ok_or(
        ToolError::ExecutionFailed {
            message: "pwsh not found on PATH. Install PowerShell to use this tool.".into(),
            source: None,
        },
    )?;
    let provider: Arc<dyn coco_shell::ShellProvider> =
        Arc::new(coco_shell::PowerShellProvider::from_shell(pwsh_shell));
    let mut executor = coco_shell::ShellExecutor::with_provider(&cwd, provider);

    let violations_baseline = if let Some(state) = &sandbox_state {
        Some(state.violations_total_snapshot().await)
    } else {
        None
    };

    let opts = coco_shell::ExecOptions {
        timeout_ms: Some(timeout_ms as i64),
        cancel: Some(ctx.cancel.clone()),
        sandbox: sandbox_state.clone(),
        sandbox_bypass,
        ..Default::default()
    };

    let cmd_result =
        executor
            .execute(command, &opts)
            .await
            .map_err(|e| ToolError::ExecutionFailed {
                message: format!("PowerShell execution failed: {e}"),
                source: None,
            })?;

    if cmd_result.timed_out {
        return Err(ToolError::Timeout {
            timeout_ms: timeout_ms as i64,
        });
    }

    // setCwd(new_cwd) → resetCwdIfOutsideProject (TS parity with
    // `PowerShellTool.tsx:520-525`). `Set-Location C:\foo` in turn N
    // persists into turn N+1; if it drifted outside the allowed set,
    // session_cwd snaps back to original and we annotate stderr.
    let reset_message =
        crate::tools::shell_cwd::finalize_cwd_post_exec(ctx, cmd_result.new_cwd.clone()).await;

    // Decode UTF-16 BOM-prefixed output. Falls through to UTF-8 lossy
    // when the byte buffer is plain UTF-8.
    let stdout = cmd_result
        .stdout_bytes
        .as_deref()
        .map(decode_ps_output)
        .unwrap_or_else(|| cmd_result.stdout.clone());
    let mut stderr = cmd_result
        .stderr_bytes
        .as_deref()
        .map(decode_ps_output)
        .unwrap_or_else(|| cmd_result.stderr.clone());

    crate::tools::shell_cwd::annotate_stderr_with_reset(&mut stderr, reset_message);

    if let (Some(state), Some(prev)) = (&sandbox_state, violations_baseline)
        && let Some(annotation) = state.format_violations_since(prev).await
    {
        if stderr.is_empty() {
            stderr = annotation;
        } else {
            stderr.push('\n');
            stderr.push_str(&annotation);
        }
    }

    Ok(ToolResult {
        data: serde_json::json!({
            "stdout": stdout,
            "stderr": stderr,
            "exitCode": cmd_result.exit_code,
            "interrupted": cmd_result.interrupted,
        }),
        new_messages: vec![],
        app_state_patch: None,
        permission_updates: Vec::new(),
    })
}

#[cfg(test)]
#[path = "powershell_tool.test.rs"]
mod tests;
