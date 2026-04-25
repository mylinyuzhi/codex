use coco_shell::read_only::is_read_only_command;
use coco_shell::sandbox::BypassRequest;
use coco_shell::sandbox::SandboxConfig as ShellSandboxConfig;
use coco_shell::sandbox::SandboxDecision;
use coco_shell::sandbox::SandboxMode as ShellSandboxMode;
use coco_shell::sandbox::should_sandbox_command;
use coco_shell::security::SecuritySeverity;
use coco_shell::security::check_security;
use coco_tool_runtime::BackgroundShellRequest;
use coco_tool_runtime::DescriptionOptions;
use coco_tool_runtime::Tool;
use coco_tool_runtime::ToolError;
use coco_tool_runtime::ToolProgress;
use coco_tool_runtime::ToolUseContext;
use coco_tool_runtime::ValidationResult;
use coco_types::ToolId;
use coco_types::ToolInputSchema;
use coco_types::ToolName;
use coco_types::ToolResult;
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;

fn default_timeout_ms(config: &coco_config::ToolConfig) -> u64 {
    config.bash.default_timeout_ms.max(1) as u64
}

fn max_timeout_ms(config: &coco_config::ToolConfig) -> u64 {
    config
        .bash
        .max_timeout_ms
        .max(config.bash.default_timeout_ms)
        .max(1) as u64
}

/// Long-form tool description shown to the model.
///
/// TS: `tools/BashTool/prompt.ts:275-369` `getSimplePrompt()`. This is
/// the **core instructional text** — the universal Bash usage guidance
/// that applies to all builds. The conditional sections that TS adds
/// based on runtime config (sandbox config dump, undercover guidance,
/// per-user git skill references, embedded-search-tool variants) are
/// intentionally omitted because:
///
///   1. They depend on runtime feature flags that coco-rs doesn't
///      currently model (USER_TYPE='ant', isUndercover, hasEmbeddedSearchTools).
///   2. The sandbox config dump leaks /private/tmp paths into the
///      prompt cache key — TS works around this by normalizing to
///      $TMPDIR but coco-rs doesn't have a sandbox manager that
///      emits config to the prompt.
///   3. The git commit/PR section is ~80 lines of skill-specific
///      guidance that's only relevant when /commit, /commit-push-pr
///      skills are loaded — coco-rs has its own skill discovery
///      pipeline.
///
/// What IS ported: the avoid-native-commands list, tool-preference
/// items, multi-command parallelism guidance, git safety bullets,
/// timeout/run_in_background notes, sleep-avoidance guidance, and the
/// commit safety/PR creation instructions (full text from TS lines
/// 81-160 — the external-user branch, since coco-rs is the OSS distro).
const BASH_TOOL_DESCRIPTION: &str = "Executes a given bash command and returns its output.

The working directory persists between commands, but shell state does not. The shell environment is initialized from the user's profile (bash or zsh).

IMPORTANT: Avoid using this tool to run `find`, `grep`, `cat`, `head`, `tail`, `sed`, `awk`, or `echo` commands, unless explicitly instructed or after you have verified that a dedicated tool cannot accomplish your task. Instead, use the appropriate dedicated tool as this will provide a much better experience for the user:

 - File search: Use Glob (NOT find or ls)
 - Content search: Use Grep (NOT grep or rg)
 - Read files: Use Read (NOT cat/head/tail)
 - Edit files: Use Edit (NOT sed/awk)
 - Write files: Use Write (NOT echo >/cat <<EOF)
 - Communication: Output text directly (NOT echo/printf)
While the Bash tool can do similar things, it's better to use the built-in tools as they provide a better user experience and make it easier to review tool calls and give permission.

# Instructions
 - If your command will create new directories or files, first use this tool to run `ls` to verify the parent directory exists and is the correct location.
 - Always quote file paths that contain spaces with double quotes in your command (e.g., cd \"path with spaces/file.txt\")
 - Try to maintain your current working directory throughout the session by using absolute paths and avoiding usage of `cd`. You may use `cd` if the User explicitly requests it.
 - You may specify an optional timeout in milliseconds (up to 600000ms / 10 minutes). By default, your command will timeout after 120000ms (2 minutes).
 - You can use the `run_in_background` parameter to run the command in the background. Only use this if you don't need the result immediately and are OK being notified when the command completes later. You do not need to check the output right away - you'll be notified when it finishes. You do not need to use '&' at the end of the command when using this parameter.
 - When issuing multiple commands:
  - If the commands are independent and can run in parallel, make multiple Bash tool calls in a single message. Example: if you need to run \"git status\" and \"git diff\", send a single message with two Bash tool calls in parallel.
  - If the commands depend on each other and must run sequentially, use a single Bash call with '&&' to chain them together.
  - Use ';' only when you need to run commands sequentially but don't care if earlier commands fail.
  - DO NOT use newlines to separate commands (newlines are ok in quoted strings).
 - For git commands:
  - Prefer to create a new commit rather than amending an existing commit.
  - Before running destructive operations (e.g., git reset --hard, git push --force, git checkout --), consider whether there is a safer alternative that achieves the same goal. Only use destructive operations when they are truly the best approach.
  - Never skip hooks (--no-verify) or bypass signing (--no-gpg-sign, -c commit.gpgsign=false) unless the user has explicitly asked for it. If a hook fails, investigate and fix the underlying issue.
 - Avoid unnecessary `sleep` commands:
  - Do not sleep between commands that can run immediately — just run them.
  - If your command is long running and you would like to be notified when it finishes — use `run_in_background`. No sleep needed.
  - Do not retry failing commands in a sleep loop — diagnose the root cause.
  - If waiting for a background task you started with `run_in_background`, you will be notified when it completes — do not poll.
  - If you must poll an external process, use a check command (e.g. `gh run view`) rather than sleeping first.
  - If you must sleep, keep the duration short (1-5 seconds) to avoid blocking the user.

# Committing changes with git

Only create commits when requested by the user. If unclear, ask first. When the user asks you to create a new git commit, follow these steps carefully:

You can call multiple tools in a single response. When multiple independent pieces of information are requested and all commands are likely to succeed, run multiple tool calls in parallel for optimal performance. The numbered steps below indicate which commands should be batched in parallel.

Git Safety Protocol:
- NEVER update the git config
- NEVER run destructive git commands (push --force, reset --hard, checkout ., restore ., clean -f, branch -D) unless the user explicitly requests these actions. Taking unauthorized destructive actions is unhelpful and can result in lost work, so it's best to ONLY run these commands when given direct instructions
- NEVER skip hooks (--no-verify, --no-gpg-sign, etc) unless the user explicitly requests it
- NEVER run force push to main/master, warn the user if they request it
- CRITICAL: Always create NEW commits rather than amending, unless the user explicitly requests a git amend. When a pre-commit hook fails, the commit did NOT happen — so --amend would modify the PREVIOUS commit, which may result in destroying work or losing previous changes. Instead, after hook failure, fix the issue, re-stage, and create a NEW commit
- When staging files, prefer adding specific files by name rather than using \"git add -A\" or \"git add .\", which can accidentally include sensitive files (.env, credentials) or large binaries
- NEVER commit changes unless the user explicitly asks you to. It is VERY IMPORTANT to only commit when explicitly asked, otherwise the user will feel that you are being too proactive

1. Run the following bash commands in parallel, each using the Bash tool:
  - Run a git status command to see all untracked files. IMPORTANT: Never use the -uall flag as it can cause memory issues on large repos.
  - Run a git diff command to see both staged and unstaged changes that will be committed.
  - Run a git log command to see recent commit messages, so that you can follow this repository's commit message style.
2. Analyze all staged changes (both previously staged and newly added) and draft a commit message:
  - Summarize the nature of the changes (eg. new feature, enhancement to an existing feature, bug fix, refactoring, test, docs, etc.). Ensure the message accurately reflects the changes and their purpose (i.e. \"add\" means a wholly new feature, \"update\" means an enhancement to an existing feature, \"fix\" means a bug fix, etc.).
  - Do not commit files that likely contain secrets (.env, credentials.json, etc). Warn the user if they specifically request to commit those files
  - Draft a concise (1-2 sentences) commit message that focuses on the \"why\" rather than the \"what\"
  - Ensure it accurately reflects the changes and their purpose
3. Run the following commands in parallel:
   - Add relevant untracked files to the staging area.
   - Create the commit.
   - Run git status after the commit completes to verify success.
   Note: git status depends on the commit completing, so run it sequentially after the commit.
4. If the commit fails due to pre-commit hook: fix the issue and create a NEW commit

Important notes:
- NEVER run additional commands to read or explore code, besides git bash commands
- DO NOT push to the remote repository unless the user explicitly asks you to do so
- IMPORTANT: Never use git commands with the -i flag (like git rebase -i or git add -i) since they require interactive input which is not supported.
- IMPORTANT: Do not use --no-edit with git rebase commands, as the --no-edit flag is not a valid option for git rebase.
- If there are no changes to commit (i.e., no untracked files and no modifications), do not create an empty commit

# Creating pull requests
Use the gh command via the Bash tool for ALL GitHub-related tasks including working with issues, pull requests, checks, and releases. If given a Github URL use the gh command to get the information needed.

IMPORTANT: When the user asks you to create a pull request, follow these steps carefully:

1. Run the following bash commands in parallel using the Bash tool, in order to understand the current state of the branch since it diverged from the main branch:
   - Run a git status command to see all untracked files (never use -uall flag)
   - Run a git diff command to see both staged and unstaged changes that will be committed
   - Check if the current branch tracks a remote branch and is up to date with the remote, so you know if you need to push to the remote
   - Run a git log command and `git diff [base-branch]...HEAD` to understand the full commit history for the current branch (from the time it diverged from the base branch)
2. Analyze all changes that will be included in the pull request, making sure to look at all relevant commits (NOT just the latest commit, but ALL commits that will be included in the pull request!!!), and draft a pull request title and summary:
   - Keep the PR title short (under 70 characters)
   - Use the description/body for details, not the title
3. Run the following commands in parallel:
   - Create new branch if needed
   - Push to remote with -u flag if needed
   - Create PR using gh pr create

Important:
- DO NOT use the TodoWrite or Agent tools
- Return the PR URL when you're done, so the user can see it

# Other common operations
- View comments on a Github PR: gh api repos/foo/bar/pulls/123/comments";

/// Build a `SandboxConfig` from environment variables, matching the
/// TS `SandboxManager.isSandboxingEnabled()` + settings pipeline as
/// close as we can without a full config/settings system.
///
/// Env vars (R6-T18):
///   - `COCO_SANDBOX_ENABLED` — truthy values ("1", "true", "yes") enable
///     sandboxing. Defaults to disabled to match current coco-rs behavior.
///   - `COCO_SANDBOX_MODE` — one of `read_only`, `strict`, `external`.
///     Defaults to `read_only` when enabled.
///   - `COCO_SANDBOX_EXCLUDED_COMMANDS` — colon-separated command prefixes
///     to exclude (e.g. `"git:npm:cargo"`). Supports the same patterns
///     as `coco_shell::sandbox::is_excluded_command`.
///   - `COCO_SANDBOX_ALLOW_NETWORK` — truthy → allow network inside sandbox.
///
/// TS `shouldUseSandbox.ts` uses similar logic: enable check → bypass
/// check → exclusion list → sandbox decision.
fn shell_sandbox_config_from_runtime(config: &coco_config::SandboxConfig) -> ShellSandboxConfig {
    if !config.enabled {
        return ShellSandboxConfig::default();
    }
    let mode = match config.mode {
        coco_types::SandboxMode::ReadOnly => ShellSandboxMode::ReadOnly,
        coco_types::SandboxMode::WorkspaceWrite => ShellSandboxMode::Strict,
        coco_types::SandboxMode::FullAccess => ShellSandboxMode::None,
        coco_types::SandboxMode::ExternalSandbox => ShellSandboxMode::External,
    };

    ShellSandboxConfig {
        mode,
        excluded_commands: config.excluded_commands.clone(),
        allow_bypass: true,
        allow_network: config.allow_network,
        ..Default::default()
    }
}

/// Effective max Bash output byte budget.
///
/// Clamp to `[0, BASH_MAX_OUTPUT_BYTES_UPPER]` is enforced by
/// `BashConfig::finalize()` at config-resolution time — this helper
/// just normalizes the (already non-negative) value to `usize`.
fn max_output_bytes(config: &coco_config::ToolConfig) -> usize {
    config.bash.max_output_bytes.max(0) as usize
}

/// Bash tool -- executes shell commands via bash -c.
/// Captures stdout, stderr, and exit code.
///
/// Supports `run_in_background: true` to spawn the command as a background
/// task. The task ID is returned immediately and the model is notified
/// asynchronously when the command completes via task-notification XML.
pub struct BashTool;

#[async_trait::async_trait]
impl Tool for BashTool {
    fn id(&self) -> ToolId {
        ToolId::Builtin(ToolName::Bash)
    }

    fn name(&self) -> &str {
        ToolName::Bash.as_str()
    }

    fn description(&self, _input: &Value, _options: &DescriptionOptions) -> String {
        BASH_TOOL_DESCRIPTION.into()
    }

    fn input_schema(&self) -> ToolInputSchema {
        let mut props = HashMap::new();
        props.insert(
            "command".into(),
            serde_json::json!({
                "type": "string",
                "description": "The command to execute"
            }),
        );
        props.insert(
            "timeout".into(),
            serde_json::json!({
                "type": "number",
                "description": "Optional timeout in milliseconds. Defaults to the resolved \
                                Bash tool config and cannot exceed its configured max timeout."
            }),
        );
        props.insert(
            "description".into(),
            serde_json::json!({
                "type": "string",
                "description": "Clear description of what this command does"
            }),
        );
        props.insert(
            "run_in_background".into(),
            serde_json::json!({
                "type": "boolean",
                "description": "Set to true to run this command in the background. You will be notified when it completes."
            }),
        );
        props.insert(
            "dangerouslyDisableSandbox".into(),
            serde_json::json!({
                "type": "boolean",
                "description": "Set this to true to dangerously override sandbox mode and run commands without sandboxing."
            }),
        );
        ToolInputSchema { properties: props }
    }

    /// Read-only fast path. Mirrors TS `BashTool.isReadOnly` → `checkReadOnlyConstraints`
    /// (`readOnlyValidation.ts:1876`). Commands on the allowlist (`cat`, `ls`, `grep`,
    /// `git log`, `docker ps`, etc.) get auto-approved upstream and batched as
    /// concurrency-safe, avoiding the permission UI for routine inspection.
    ///
    /// Delegates to `coco_shell::read_only::is_read_only_command` which wraps the
    /// 40+ safe-command allowlist + conditional safety rules for git/sed/find/rg/etc.
    fn is_read_only(&self, input: &Value) -> bool {
        input
            .get("command")
            .and_then(|v| v.as_str())
            .map(is_read_only_command)
            .unwrap_or(false)
    }

    /// Concurrency-safe iff read-only. TS `isConcurrencySafe` is driven by the
    /// same allowlist — read-only commands have no shared mutable state with
    /// sibling tools, so the executor can batch them with Read/Grep/Glob.
    fn is_concurrency_safe(&self, input: &Value) -> bool {
        self.is_read_only(input)
    }

    fn get_activity_description(&self, input: &Value) -> Option<String> {
        let command = input.get("command").and_then(|v| v.as_str())?;
        // Truncate long commands for display (char-safe)
        let truncated: String = command.chars().take(57).collect();
        let display = if truncated.len() < command.len() {
            format!("Running {truncated}...")
        } else {
            format!("Running {command}")
        };
        Some(display)
    }

    /// Destructive iff NOT read-only. The upstream permission evaluator uses this
    /// flag to decide whether the command needs user approval — the old hardcoded
    /// `true` forced approval for every `ls`/`cat`/`git log`, which was a major UX
    /// regression vs. TS. Matches TS multi-stage pipeline where the read-only fast
    /// path (`bashPermissions.ts:1663+`) auto-allows before reaching the Ask phase.
    fn is_destructive(&self, input: &Value) -> bool {
        !self.is_read_only(input)
    }

    /// Tool-result persistence threshold. TS: `BashTool.tsx:424`
    /// `maxResultSizeChars: 30_000`. When Bash output exceeds this budget,
    /// the executor persists the full output to a tool-results file and
    /// only keeps a truncated snippet inline. We match TS exactly so
    /// cross-runtime sessions handle large outputs identically.
    fn max_result_size_chars(&self) -> i32 {
        30_000
    }

    fn validate_input(&self, input: &Value, ctx: &ToolUseContext) -> ValidationResult {
        if input.get("command").and_then(|v| v.as_str()).is_none() {
            return ValidationResult::invalid("missing required field: command");
        }
        let max_timeout_ms = max_timeout_ms(&ctx.tool_config);
        if let Some(timeout) = input.get("timeout").and_then(serde_json::Value::as_u64)
            && timeout > max_timeout_ms
        {
            return ValidationResult::invalid(format!(
                "timeout must not exceed {max_timeout_ms}ms"
            ));
        }
        ValidationResult::Valid
    }

    async fn execute(
        &self,
        input: Value,
        ctx: &ToolUseContext,
    ) -> Result<ToolResult<Value>, ToolError> {
        // ── R7-T11: _simulatedSedEdit short-circuit ──
        //
        // TS `BashTool.tsx:243-258, 627-628`: the BashTool input schema
        // accepts an internal `_simulatedSedEdit: { filePath, newContent }`
        // field that the SedEditPermissionRequest TUI dialog populates
        // when the user reviews a `sed -i ...` command and chooses to
        // convert it to a previewed Edit-style write. The dialog does
        // the actual sed-against-original computation and hands BashTool
        // the precomputed `newContent` so what the user previewed is
        // exactly what gets written.
        //
        // The field is **deliberately omitted** from coco-rs's
        // `input_schema()` so the model can never see it as a valid
        // input. The upstream executor SHOULD also strip incoming
        // `_simulatedSedEdit` payloads before invoking this method as a
        // defense-in-depth measure (TS does this in
        // `services/tools/toolExecution.ts:756-770`). We emit a debug
        // log when the field is present so anomalous traffic is visible
        // even if the executor strip is missing.
        if let Some(sed_input) = input.get("_simulatedSedEdit") {
            tracing::debug!(
                "BashTool received _simulatedSedEdit input — applying as Edit-style write"
            );
            return apply_sed_edit(sed_input, ctx).await;
        }

        let command = input
            .get("command")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidInput {
                message: "missing command".into(),
                error_code: None,
            })?;

        let default_timeout_ms = default_timeout_ms(&ctx.tool_config);
        let max_timeout_ms = max_timeout_ms(&ctx.tool_config);
        let timeout_ms = input
            .get("timeout")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(default_timeout_ms)
            .min(max_timeout_ms);

        let run_in_background = input
            .get("run_in_background")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);

        // R6-T18: sandbox decision. Matches TS `shouldUseSandbox(input)`
        // at `shouldUseSandbox.ts:130-153`:
        //   1. If sandbox not globally enabled → unsandboxed
        //   2. If `dangerouslyDisableSandbox` and bypass allowed → unsandboxed
        //   3. If command matches an excluded pattern → unsandboxed
        //   4. Otherwise → sandboxed
        //
        let sandbox_config = shell_sandbox_config_from_runtime(&ctx.sandbox_config);
        let bypass = if input
            .get("dangerouslyDisableSandbox")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false)
        {
            BypassRequest::Requested
        } else {
            BypassRequest::No
        };
        let sandbox_decision = should_sandbox_command(&sandbox_config, command, bypass);
        if let SandboxDecision::Sandboxed { mode } = sandbox_decision {
            tracing::debug!(
                ?mode,
                command = %command,
                "bash command will run under sandbox (decision only — enforcement is a follow-up)",
            );
        }

        // Permission pipeline (TS: `tools/BashTool/bashPermissions.ts:1663+`,
        // with a coco-rs security extension noted below).
        //
        // Stage 1 — read-only fast path. Already handled by `is_read_only()`
        // at the trait level; the upstream permission evaluator auto-allows
        // read-only commands and batches them with other concurrency-safe
        // tools. Same behavior as TS `checkReadOnlyConstraints` returning
        // `{ behavior: 'allow' }`.
        //
        // Stage 2 — **coco-rs security Deny extension** (stricter than TS).
        // TS's `bashCommandIsSafe_DEPRECATED` returns `{behavior: 'ask'}` for
        // risky patterns (eval, IFS=, backtick substitution, etc.) and routes
        // them through the user approval flow. coco-rs chooses to HARD-FAIL
        // a small set of patterns that are nearly always malicious
        // (IFS injection, `eval`, `source /dev/tcp/...`) — we consider these
        // footguns even with approval and want them blocked without prompting.
        //
        // TS's `behavior: 'deny'` paths (`bashPermissions.ts:1000, 2254, etc.`)
        // cover user-configured deny rules and path validation — DIFFERENT
        // concerns from the pattern-based checks below. Both systems hard-fail
        // in their respective scopes.
        //
        // This is a DELIBERATE DIVERGENCE from TS. If an IFS-injecting script
        // is a legitimate use case in a specific workflow, the user should use
        // a sandbox or construct the env differently — coco-rs does not accept
        // IFS manipulation via user approval.
        //
        // Stage 3 — destructive warning. Catches `rm -rf /`, `dd of=...`,
        // and other patterns the Ask-phase classifier doesn't cover.
        //
        // Read-only commands skip stages 2 and 3 to avoid false positives on
        // harmless `grep 'foo`bar'` patterns that contain metacharacters
        // inside quoted strings.
        if !is_read_only_command(command) {
            for check in check_security(command) {
                if check.severity == SecuritySeverity::Deny {
                    return Err(ToolError::PermissionDenied {
                        message: format!(
                            "Command blocked by coco-rs security check (stricter than \
                             Claude Code): {}. If you believe this is a false positive, \
                             use a sandbox or restructure the command.",
                            check.message
                        ),
                    });
                }
            }
            if let Some(warning) = coco_shell::destructive::get_destructive_warning(command) {
                return Err(ToolError::PermissionDenied { message: warning });
            }
        }

        // Background execution: spawn task and return immediately
        if run_in_background {
            return execute_background(command, timeout_ms, ctx).await;
        }

        // Foreground execution. The sandbox decision is resolved here
        // (not inside execute_foreground) so the decision logic lives
        // alongside all other input parsing.
        //
        // R6-T19: on foreground timeout, if a TaskHandle is available
        // and auto-background is enabled, spawn a background task with
        // the same command and return the task_id as `backgroundTaskId`.
        // This matches the TS `BashTool.tsx:610, 965-969` output shape
        // (`backgroundTaskId` set when the fg command was moved to bg),
        // while acknowledging that coco-rs re-runs the command rather
        // than transferring the process handle — a true handle-transfer
        // requires a ShellExecutor API change that's out of scope here.
        // The re-run is only triggered for commands that explicitly
        // opt in via resolved runtime config so
        // side-effectful commands aren't duplicated unexpectedly.
        match execute_foreground(command, timeout_ms, ctx, sandbox_decision.is_sandboxed()).await {
            Ok(result) => Ok(result),
            Err(ToolError::ExecutionFailed { message, .. }) if message.contains("timed out") => {
                if ctx.tool_config.bash.auto_background_on_timeout && ctx.task_handle.is_some() {
                    // Spawn a fresh bg task. The fg child was already
                    // killed by the watchdog, so this is a re-run — TS
                    // transfers the handle instead, but we document the
                    // divergence rather than fake it.
                    let bg = execute_background(command, timeout_ms, ctx).await?;
                    let task_id = bg.data["task_id"].as_str().unwrap_or("").to_string();
                    return Ok(ToolResult {
                        data: serde_json::json!({
                            "stdout": "",
                            "stderr": format!("Command timed out after {timeout_ms}ms; re-running in background."),
                            "exitCode": -1,
                            "interrupted": true,
                            "backgroundTaskId": task_id,
                            "assistantAutoBackgrounded": true,
                        }),
                        new_messages: vec![],
                        app_state_patch: None,
                    });
                }
                Err(ToolError::ExecutionFailed {
                    message,
                    source: None,
                })
            }
            Err(other) => Err(other),
        }
    }
}

/// Execute a command in the background via TaskHandle.
///
/// TS: `spawnShellTask()` -- creates a background task, returns task ID immediately.
/// Model receives `<task-notification>` XML when the task completes.
async fn execute_background(
    command: &str,
    timeout_ms: u64,
    ctx: &ToolUseContext,
) -> Result<ToolResult<Value>, ToolError> {
    let task_handle = ctx
        .task_handle
        .as_ref()
        .ok_or_else(|| ToolError::ExecutionFailed {
            message: "Background task execution is not available in this context.".into(),
            source: None,
        })?;

    let description = ctx.tool_use_id.as_deref().unwrap_or("bash").to_string();

    let task_id = task_handle
        .spawn_shell_task(BackgroundShellRequest {
            command: command.to_string(),
            timeout_ms: Some(timeout_ms as i64),
            description: Some(description),
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
            "message": format!("Command is running in the background. Task ID: {task_id}. You will be notified when it completes.")
        }),
        new_messages: vec![],
        app_state_patch: None,
    })
}

/// Execute a command in the foreground with continuous progress reporting.
///
/// TS: BashTool polls TaskOutput at ~1s intervals, sending progress updates
/// with elapsed time, total bytes, and output chunks. The TUI renders
/// these as a streaming spinner with timing info.
async fn execute_foreground(
    command: &str,
    timeout_ms: u64,
    ctx: &ToolUseContext,
    should_use_sandbox: bool,
) -> Result<ToolResult<Value>, ToolError> {
    // Resolve the working directory. Worktree-isolated subagents set
    // `ctx.cwd_override` so their bash commands must run inside the
    // isolated checkout, not the host process's cwd. TS uses `getCwd()`
    // which the runtime swaps out for isolated sessions
    // (`BashTool.tsx:643-649`). Falling back to the process cwd (and
    // finally `/tmp`) matches TS's fallback chain when no override is set.
    let cwd = ctx
        .cwd_override
        .clone()
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    let mut executor = coco_shell::ShellExecutor::new_with_config(&cwd, &ctx.shell_config);

    // R6-T17 + R6-T18: thread the ctx cancel token and the sandbox
    // decision through to the shell executor. The `should_use_sandbox`
    // field on ExecOptions is a boolean flag the executor reads to
    // decide whether to wrap the command in a sandbox binary
    // (`bwrap` / `sandbox-exec`) — that wrap logic is a TODO but the
    // decision is now correctly computed and propagated.
    let opts = coco_shell::ExecOptions {
        timeout_ms: Some(timeout_ms as i64),
        cancel: Some(ctx.cancel.clone()),
        should_use_sandbox,
        ..Default::default()
    };

    // Use streaming execution with progress if progress channel is available
    let cmd_result = if let Some(progress_tx) = ctx.progress_tx.clone() {
        let tool_use_id: std::sync::Arc<str> = ctx.tool_use_id.clone().unwrap_or_default().into();

        // Initial "running" progress
        let _ = progress_tx.send(ToolProgress {
            tool_use_id: tool_use_id.to_string(),
            parent_tool_use_id: None,
            data: serde_json::json!({
                "type": "bash_progress",
                "status": "running",
                "command": command,
            }),
        });

        // Continuous progress via streaming executor (~1s interval)
        let progress_id = tool_use_id.clone();
        executor
            .execute_with_progress(command, &opts, move |progress| {
                let _ = progress_tx.send(ToolProgress {
                    tool_use_id: progress_id.to_string(),
                    parent_tool_use_id: None,
                    data: serde_json::json!({
                        "type": "bash_progress",
                        "status": "running",
                        "elapsedTimeSeconds": progress.elapsed_seconds,
                        "totalBytes": progress.total_bytes,
                    }),
                });
            })
            .await
    } else {
        executor.execute(command, &opts).await
    };

    let cmd_result = cmd_result.map_err(|e| ToolError::ExecutionFailed {
        message: format!("shell execution failed: {e}"),
        source: None,
    })?;

    // ── B4.2: auto-background-on-timeout ──
    //
    // TS `BashTool.tsx:610, 965-969` handles foreground timeout by
    // converting the running process into a background task (i.e. the
    // process keeps running, just detached from the fg await). This
    // requires the shell executor to support process handle transfer,
    // which coco-rs's current `ShellExecutor` does not.
    //
    // Until that architectural change lands, we fall back to a
    // safer-but-weaker behavior: on timeout, return an error that
    // **explicitly recommends** retrying with `run_in_background=true`.
    // The structured payload tells the model:
    //   - the command did time out (not a different error)
    //   - a task_handle is available so bg retry is possible
    //   - the original command + timeout so the retry is trivial
    //
    // This is strictly an error-message improvement — we don't risk
    // double-execution of side-effectful commands by re-spawning on
    // the tool's behalf. Re-spawn would happen only if the model
    // explicitly issued a new Bash call with run_in_background=true,
    // which is the user's explicit opt-in.
    if cmd_result.timed_out {
        let bg_available = ctx.task_handle.is_some();
        let suggestion = if bg_available {
            format!(
                " The command is a candidate for background execution. \
                 Retry with `run_in_background: true` if you want it to \
                 keep running past the {timeout_ms}ms limit."
            )
        } else {
            String::new()
        };
        return Err(ToolError::ExecutionFailed {
            message: format!("Command timed out after {timeout_ms}ms.{suggestion}"),
            source: None,
        });
    }

    let max_bytes = max_output_bytes(&ctx.tool_config);
    // The `stdout` String field comes from `cmd_result.stdout`, which
    // the executor has already cleaned up (CWD-marker stripped via
    // `extract_cwd_from_output`). Truncate that for the inline view.
    let stdout = truncate_output(cmd_result.stdout.as_bytes(), max_bytes);
    let stderr = truncate_output(cmd_result.stderr.as_bytes(), max_bytes);
    let exit_code = cmd_result.exit_code;
    // R7-T18: image detection inspects the executor's raw stdout bytes
    // (pre-UTF-8-lossy) when available so the magic-byte signature
    // isn't mangled by replacement characters. Note this DOES include
    // the CWD marker for shells that emit one — the detector is
    // resilient to trailing bytes (PNG / JPEG / GIF / WebP all match
    // on the leading bytes only). The fallback to `cmd_result.stdout`
    // covers test stubs that don't populate `stdout_bytes`.
    let raw_stdout_bytes_for_detection: &[u8] = cmd_result
        .stdout_bytes
        .as_deref()
        .unwrap_or(cmd_result.stdout.as_bytes());

    // R5-T14 + R6-T17 + R7-T12: structured output matching TS
    // `BashTool.tsx:279-294` `outputSchema`:
    //
    //   { stdout, stderr, exitCode, interrupted, isImage?,
    //     backgroundTaskId?, structuredContent?, persistedOutputPath?,
    //     persistedOutputSize? }
    //
    // `interrupted` is now sourced from the shell executor's own
    // `interrupted` flag, which is set when the ctx cancel token fires
    // and the child process is killed. That's distinct from a timeout
    // (the executor sets `timed_out` for the watchdog path). Either
    // condition surfaces as `interrupted=true` to the model, matching
    // TS semantics where both AbortController and the timeout watchdog
    // set `interrupted=true`.
    let interrupted = cmd_result.interrupted || cmd_result.timed_out;

    // R7-T12 fields:
    //
    // 1. `isImage`: detect from stdout magic bytes. TS sets this when
    //    e.g. `cat image.png` returns binary image data, so the UI can
    //    render it as an inline image block.
    // 2. `structuredContent`: when stdout IS an image, attach a base64
    //    multimodal block so the model receives the actual pixels.
    //    For text output, structuredContent is omitted (the plain
    //    `stdout` field is enough).
    // 3. `persistedOutputPath`/`persistedOutputSize`: when the raw
    //    stdout exceeds the persistence threshold (30K — matches TS
    //    `BashTool.maxResultSizeChars`), write the FULL untruncated
    //    output to a temp file. The model can then Read the file if
    //    it needs the full content.
    let is_image = is_likely_image_bytes(raw_stdout_bytes_for_detection);
    let structured_content = if is_image {
        Some(build_image_block(raw_stdout_bytes_for_detection))
    } else {
        None
    };
    // Persistence threshold operates on the cleaned `stdout` String so
    // we don't persist trailing CWD markers or other executor-internal
    // bytes.
    let (persisted_path, persisted_size) =
        maybe_persist_oversized_output(cmd_result.stdout.as_bytes());

    let mut result_obj = serde_json::json!({
        "stdout": stdout,
        "stderr": stderr,
        "exitCode": exit_code,
        "interrupted": interrupted,
    });
    if is_image {
        result_obj["isImage"] = serde_json::Value::Bool(true);
    }
    if let Some(content) = structured_content {
        result_obj["structuredContent"] = content;
    }
    if let Some(path) = persisted_path {
        result_obj["persistedOutputPath"] = serde_json::Value::String(path);
        result_obj["persistedOutputSize"] =
            serde_json::Value::Number(serde_json::Number::from(persisted_size));
    }

    Ok(ToolResult {
        data: result_obj,
        new_messages: vec![],
        app_state_patch: None,
    })
}

/// Detect whether a byte buffer is a known image format from its magic
/// bytes. Matches TS `BashTool.tsx`-side detection which sets `isImage`
/// when `cat image.png` style commands return binary image data on
/// stdout. Used by the UI to render the result as an inline image
/// rather than attempting to display raw bytes as text.
///
/// Recognized formats (order = check priority):
///  - PNG: `89 50 4E 47 0D 0A 1A 0A`
///  - JPEG: `FF D8 FF`
///  - GIF: `47 49 46 38` (`GIF8`, both 87a and 89a)
///  - WebP: `52 49 46 46` ... `57 45 42 50` (`RIFF`...`WEBP`)
///  - BMP: `42 4D` (`BM`)
pub(crate) fn is_likely_image_bytes(bytes: &[u8]) -> bool {
    if bytes.starts_with(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]) {
        return true;
    }
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return true;
    }
    if bytes.starts_with(b"GIF8") {
        return true;
    }
    // WebP: RIFF....WEBP — needs at least 12 bytes and the magic at
    // positions 0-3 + 8-11.
    if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        return true;
    }
    if bytes.starts_with(b"BM") && bytes.len() > 14 {
        return true;
    }
    false
}

/// Build a multimodal content-block array containing a single base64
/// image entry from raw image bytes. The MIME type is inferred from
/// the magic bytes detected in `is_likely_image_bytes` so the model
/// receives the correct content type. Used to populate the
/// `structuredContent` field of the BashTool result envelope when
/// stdout is an image.
///
/// Each magic-byte check mirrors `is_likely_image_bytes` exactly,
/// including the BMP `len() > 14` length gate, so direct callers
/// can't accidentally tag a 2-byte `b"BM"` payload as a BMP image.
fn build_image_block(bytes: &[u8]) -> Value {
    use base64::Engine;
    let mime = if bytes.starts_with(&[0x89, 0x50, 0x4E, 0x47]) {
        "image/png"
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        "image/jpeg"
    } else if bytes.starts_with(b"GIF8") {
        "image/gif"
    } else if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        "image/webp"
    } else if bytes.len() > 14 && bytes.starts_with(b"BM") {
        "image/bmp"
    } else {
        "application/octet-stream"
    };
    let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
    serde_json::json!([
        {
            "type": "image",
            "source": {
                "type": "base64",
                "media_type": mime,
                "data": b64,
            }
        }
    ])
}

/// If stdout exceeds the persistence threshold, write the FULL
/// untruncated bytes to a temp file and return its path + size. The
/// inline `stdout` field still gets the truncated snippet — the model
/// can Read the persisted file to recover the rest.
///
/// TS: `BashTool.tsx:279-293` `persistedOutputPath` / `persistedOutputSize`
/// fields plus the `maxResultSizeChars: 30_000` persistence threshold.
/// TS writes to a per-session tool-results directory; coco-rs uses
/// `std::env::temp_dir()` for now since there's no persistent
/// tool-results dir wired up at the tool layer (a follow-up could
/// route this through `ctx.config_home`).
///
/// Returns `(None, 0)` when output is small enough to keep inline,
/// or `(Some(path), bytes)` after persisting. Failure to write the
/// file silently returns `(None, 0)` — persistence is best-effort
/// and shouldn't break the tool result.
pub(crate) fn maybe_persist_oversized_output(bytes: &[u8]) -> (Option<String>, usize) {
    // Match TS `maxResultSizeChars: 30_000` as the persistence trigger.
    // We threshold against raw bytes since TS uses char count and
    // both equate to ~30K for typical ASCII output.
    const PERSIST_THRESHOLD: usize = 30_000;
    if bytes.len() <= PERSIST_THRESHOLD {
        return (None, 0);
    }

    let dir = std::env::temp_dir().join("coco-bash-output");
    if std::fs::create_dir_all(&dir).is_err() {
        return (None, 0);
    }
    // Unique filename: nanosecond timestamp + process id, no atomic
    // counter needed since collisions are vanishingly unlikely and
    // best-effort persistence tolerates the rare clash by overwriting.
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let pid = std::process::id();
    let path = dir.join(format!("bash-{pid}-{ts}.out"));
    if std::fs::write(&path, bytes).is_err() {
        return (None, 0);
    }
    (Some(path.display().to_string()), bytes.len())
}

/// Apply a previewed sed edit by writing the precomputed `newContent`
/// to `filePath`, preserving the file's original encoding and line
/// endings. Used by the `_simulatedSedEdit` short-circuit in
/// `BashTool::execute` so that what the user previewed in the
/// SedEditPermissionRequest dialog is exactly what hits disk.
///
/// TS: `tools/BashTool/BashTool.tsx:355-419` `applySedEdit`. Behavior
/// matches TS:
///   1. Resolve the absolute path (`expandPath` in TS, `canonicalize`
///      with a fallback to the input path here).
///   2. Read the original file metadata to detect encoding + line
///      endings — sed preserves both.
///   3. ENOENT → return a sed-formatted error message via stderr,
///      `exitCode: 1`, never throw. The model sees a normal Bash
///      result with the sed CLI's "No such file or directory" text.
///   4. Track file history before mutating (matches `track_file_edit`
///      called from FileEditTool/FileWriteTool).
///   5. Write the new content with the detected encoding + line
///      endings (NOT the LF-always policy used by FileWriteTool —
///      sed is an in-place edit and must round-trip the format).
///   6. Update `FileReadState` so subsequent edits/writes don't trip
///      the read-before-write check, and so the file_unchanged dedup
///      cache is still consistent.
///   7. Return TS-shaped `{ stdout: "", stderr: "", exitCode: 0,
///      interrupted: false }`.
///
/// `sed_input` must be a JSON object with string `filePath` and string
/// `newContent` fields. Missing/wrong-type fields surface as
/// `InvalidInput` errors.
async fn apply_sed_edit(
    sed_input: &Value,
    ctx: &ToolUseContext,
) -> Result<ToolResult<Value>, ToolError> {
    let file_path = sed_input
        .get("filePath")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ToolError::InvalidInput {
            message: "_simulatedSedEdit.filePath is required".into(),
            error_code: None,
        })?;
    let new_content = sed_input
        .get("newContent")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ToolError::InvalidInput {
            message: "_simulatedSedEdit.newContent is required".into(),
            error_code: None,
        })?;

    let path = std::path::Path::new(file_path);

    // ENOENT → sed-shaped error envelope, NOT a tool error. TS does the
    // same so the model can pattern-match `sed: ... No such file ...`
    // and recover.
    if !path.exists() {
        return Ok(ToolResult {
            data: serde_json::json!({
                "stdout": "",
                "stderr": format!("sed: {file_path}: No such file or directory"),
                "exitCode": 1,
                "interrupted": false,
            }),
            new_messages: vec![],
            app_state_patch: None,
        });
    }

    // Detect encoding + line endings from the existing file so the
    // write round-trips the format. Read failure (rare — file existed
    // a microsecond ago) → sed-style error.
    let (_old_content, encoding, line_ending) = match coco_file_encoding::read_with_format(path) {
        Ok(v) => v,
        Err(e) => {
            return Ok(ToolResult {
                data: serde_json::json!({
                    "stdout": "",
                    "stderr": format!("sed: {file_path}: {e}"),
                    "exitCode": 1,
                    "interrupted": false,
                }),
                new_messages: vec![],
                app_state_patch: None,
            });
        }
    };

    // File-history snapshot before mutating. Matches FileEditTool /
    // FileWriteTool / NotebookEditTool ordering.
    crate::track_file_edit(ctx, path).await;

    // Write the previewed content, preserving original encoding +
    // line endings (the key difference from FileWriteTool which
    // always normalizes to LF).
    if let Err(e) = coco_file_encoding::write_with_format(path, new_content, encoding, line_ending)
    {
        return Err(ToolError::ExecutionFailed {
            message: format!("failed to write sed-edit result to {file_path}: {e}"),
            source: None,
        });
    }

    // Refresh the cache so the next Edit/Write doesn't fail its mtime
    // check against a stale entry left by the earlier Read.
    crate::record_file_edit(ctx, path, new_content.to_string()).await;
    // Fire skill auto-discovery — TS `BashTool.ts` does this too when a
    // sed pipeline touches a path inside a `.claude/skills/` ancestor.
    crate::track_skill_discovery(ctx, path).await;

    Ok(ToolResult {
        data: serde_json::json!({
            "stdout": "",
            "stderr": "",
            "exitCode": 0,
            "interrupted": false,
        }),
        new_messages: vec![],
        app_state_patch: None,
    })
}

/// Truncate output using a first+last pattern.
///
/// TS `BashTool/utils.ts::formatOutput` keeps only the first
/// `maxOutputLength` chars and appends a line-count footer. coco-rs uses a
/// slightly richer first+last pattern — the tail usually contains the
/// most actionable information (error messages, exit status) so
/// preserving both halves beats a pure head-truncation for debuggability.
/// Still TS-compatible at the byte-budget level: the caller passes the
/// resolved `max_bytes` from [`max_output_bytes`].
///
/// Char boundaries are respected so the truncation never yields invalid
/// UTF-8. If `max_bytes` is odd, the first half is one byte smaller than
/// the last half (they're both char-boundary snapped).
fn truncate_output(bytes: &[u8], max_bytes: usize) -> String {
    let s = String::from_utf8_lossy(bytes);
    if s.len() <= max_bytes {
        return s.to_string();
    }

    let half = max_bytes / 2;
    // Snap the first slice to the nearest preceding char boundary.
    let mut first_end = half;
    while first_end > 0 && !s.is_char_boundary(first_end) {
        first_end -= 1;
    }
    // Snap the last slice to the nearest following char boundary.
    let mut last_start = s.len() - half;
    while last_start < s.len() && !s.is_char_boundary(last_start) {
        last_start += 1;
    }
    let first = &s[..first_end];
    let last = &s[last_start..];
    let truncated_count = s.len() - first_end - (s.len() - last_start);
    format!("{first}\n... [{truncated_count} chars truncated] ...\n{last}")
}

#[cfg(test)]
#[path = "bash.test.rs"]
mod tests;
