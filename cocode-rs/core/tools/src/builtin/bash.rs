//! Bash tool for executing shell commands.
//!
//! Delegates to [`ShellExecutor`] for command execution, CWD tracking,
//! and background task management.

use super::prompts;
use crate::context::ToolContext;
use crate::error::Result;
use crate::tool::Tool;
use async_trait::async_trait;
use cocode_protocol::ApprovalRequest;
use cocode_protocol::ConcurrencySafety;
use cocode_protocol::PermissionResult;
use cocode_protocol::RiskSeverity;
use cocode_protocol::RiskType;
use cocode_protocol::SecurityRisk;
use cocode_protocol::ToolOutput;
use cocode_shell::CommandResult;
use cocode_shell::ExecuteResult;
use serde_json::Value;

/// Maximum number of subcommands allowed in a compound command.
const MAX_SUBCOMMANDS: usize = 20;

/// Default timeout in seconds.
const DEFAULT_TIMEOUT_SECS: i64 = 120;
/// Maximum timeout in seconds.
const MAX_TIMEOUT_SECS: i64 = 600;

/// Commands safe for plan mode (read-only, no side effects).
/// Superset of the concurrency-safe set, adding text processing and inspection tools
/// that are harmless when used in pipelines.
const PLAN_MODE_SAFE_BINARIES: &[&str] = &[
    // Basic inspection (same as concurrency whitelist)
    "ls", "cat", "head", "tail", "wc", "grep", "rg", "find", "which", "whoami", "pwd", "echo",
    "date", "env", "printenv", "uname", "hostname", "df", "du", "file", "stat", "type",
    // Text processing (pipe-friendly, no side effects)
    "sed", "awk", "sort", "uniq", "cut", "tr", "fmt", "column", "paste", "join", "comm",
    // Comparison / inspection
    "diff", "cmp", // Path utilities
    "dirname", "basename", "realpath", "readlink", // Tree / structured data
    "tree", "jq", "yq", // Output / testing
    "printf", "test", "true", "false", // Binary inspection
    "strings", "xxd", "od", "hexdump",
];

/// Git subcommands that are read-only (expanded for plan mode).
const PLAN_MODE_GIT_SUBCOMMANDS: &[&str] = &[
    "status",
    "log",
    "diff",
    "show",
    "branch",
    "tag",
    "remote",
    "rev-parse",
    "describe",
    "ls-files",
    "ls-tree",
    "cat-file",
    "config",
    "blame",
    "shortlog",
];

/// Tool for executing shell commands.
pub struct BashTool;

impl BashTool {
    /// Create a new Bash tool.
    pub fn new() -> Self {
        Self
    }
}

impl Default for BashTool {
    fn default() -> Self {
        Self::new()
    }
}

/// Check if a command is read-only (safe for concurrent execution).
pub fn is_read_only_command(command: &str) -> bool {
    let trimmed = command.trim();
    let first_word = trimmed.split_whitespace().next().unwrap_or("");

    let is_simple = !trimmed.contains("&&")
        && !trimmed.contains("||")
        && !trimmed.contains(';')
        && !trimmed.contains('|')
        && !trimmed.contains('>')
        && !trimmed.contains('<');

    if !is_simple {
        return false;
    }

    match first_word {
        "git" => {
            // Only read-only git subcommands are safe for concurrent execution
            let subcommand = trimmed.split_whitespace().nth(1).unwrap_or("");
            PLAN_MODE_GIT_SUBCOMMANDS.contains(&subcommand)
        }
        _ => matches!(
            first_word,
            "ls" | "cat"
                | "head"
                | "tail"
                | "wc"
                | "grep"
                | "rg"
                | "find"
                | "which"
                | "whoami"
                | "pwd"
                | "echo"
                | "date"
                | "env"
                | "printenv"
                | "uname"
                | "hostname"
                | "df"
                | "du"
                | "file"
                | "stat"
                | "type"
        ),
    }
}

/// Check if a binary name (with its args) is safe for plan mode.
fn is_plan_mode_safe_binary(name: &str, args: &[String]) -> bool {
    // git: only allow read-only subcommands
    if name == "git" {
        let subcommand = args.first().map(String::as_str).unwrap_or("");
        return PLAN_MODE_GIT_SUBCOMMANDS.contains(&subcommand);
    }

    // sed: block -i (in-place editing)
    if name == "sed" {
        let has_inplace = args.iter().any(|a| {
            // Standalone -i or -i.bak
            if a == "-i" || a.starts_with("-i") {
                return true;
            }
            // Combined short flags containing 'i': -ni, -in, etc.
            a.starts_with('-') && !a.starts_with("--") && a.contains('i')
        });
        return !has_inplace;
    }

    PLAN_MODE_SAFE_BINARIES.contains(&name)
}

/// Check if a command is allowed in plan mode.
///
/// Plan mode only permits read-only commands. Uses a two-layer approach:
/// 1. **Fast path**: `is_read_only_command()` for simple commands without operators
/// 2. **Shell parser**: `try_extract_safe_commands()` for pipelines and chained commands,
///    verifying every binary in the pipeline is in the safe set
fn is_plan_mode_allowed(command: &str) -> bool {
    let trimmed = command.trim();

    // Commands with potential shell expansions ($VAR, $(cmd), `cmd`) skip the
    // fast path. is_read_only_command doesn't inspect these, but the parser
    // properly rejects dangerous constructs while allowing safe uses (e.g.
    // '$5' inside single quotes).
    let has_expansion = trimmed.contains('$') || trimmed.contains('`');

    // Fast path: simple read-only commands without pipes/operators/expansions
    if !has_expansion && is_read_only_command(command) {
        return true;
    }

    // Shell parser path: handle piped/chained commands.
    // try_extract_safe_commands() returns None for dangerous constructs
    // (subshells, redirections, variable expansions, command substitutions).
    let mut parser = cocode_shell_parser::ShellParser::new();
    let parsed = parser.parse(command);

    let commands = match parsed.try_extract_safe_commands() {
        Some(cmds) => cmds,
        None => return false,
    };

    // Every command in the pipeline must use a safe binary
    commands.iter().all(|cmd| {
        let name = cmd.first().map(String::as_str).unwrap_or("");
        let args: &[String] = if cmd.len() > 1 { &cmd[1..] } else { &[] };
        is_plan_mode_safe_binary(name, args)
    })
}

/// Check for risky compound command patterns.
///
/// Returns a reason string if the compound command should require approval.
fn check_compound_risks(commands: &[Vec<String>]) -> Option<String> {
    // 1. Subcommand count cap
    if commands.len() > MAX_SUBCOMMANDS {
        return Some(format!(
            "{} subcommands exceeds limit ({})",
            commands.len(),
            MAX_SUBCOMMANDS
        ));
    }

    // 2. Multiple cd detection
    let cd_count = commands
        .iter()
        .filter(|c| c.first().map(String::as_str) == Some("cd"))
        .count();
    if cd_count > 1 {
        return Some("multiple cd commands in one invocation".to_string());
    }

    // 3. cd + git write command protection
    let has_cd = cd_count > 0;
    let has_git_write = commands.iter().any(|c| {
        c.first().map(String::as_str) == Some("git")
            && c.get(1).is_some_and(|s| {
                matches!(
                    s.as_str(),
                    "push" | "commit" | "merge" | "rebase" | "reset" | "checkout"
                )
            })
    });
    if has_cd && has_git_write {
        return Some("cd combined with git write command requires approval".to_string());
    }

    None
}

#[async_trait]
impl Tool for BashTool {
    fn name(&self) -> &str {
        cocode_protocol::ToolName::Bash.as_str()
    }

    fn description(&self) -> &str {
        prompts::BASH_DESCRIPTION
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "The command to execute"
                },
                "description": {
                    "type": "string",
                    "description": "Clear description of what this command does"
                },
                "timeout": {
                    "type": "integer",
                    "description": "Optional timeout in milliseconds (max 600000)"
                },
                "run_in_background": {
                    "type": "boolean",
                    "description": "Run command in background",
                    "default": false
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
        // Bash is generally unsafe, but per-command check via is_concurrency_safe_for()
        ConcurrencySafety::Unsafe
    }

    fn is_concurrency_safe_for(&self, input: &Value) -> bool {
        input["command"]
            .as_str()
            .map(is_read_only_command)
            .unwrap_or(false)
    }

    fn is_read_only(&self) -> bool {
        false // Cannot determine without input
    }

    fn max_result_size_chars(&self) -> i32 {
        30_000
    }

    async fn check_permission(&self, input: &Value, _ctx: &ToolContext) -> PermissionResult {
        let command = match input.get("command").and_then(|v| v.as_str()) {
            Some(cmd) => cmd,
            None => return PermissionResult::Passthrough,
        };

        // Sandbox auto-allow: when sandbox is active, auto-allow is enabled,
        // and the command qualifies for sandboxing (not bypass-requested, not excluded),
        // the sandbox itself becomes the security boundary — no manual approval needed.
        let bypass_requested =
            super::input_helpers::bool_or(input, "dangerouslyDisableSandbox", false);
        if let Some(ref state) = _ctx.sandbox_state
            && state.auto_allow_enabled()
            && !bypass_requested
            && state.should_sandbox_command(command, cocode_sandbox::SandboxBypass::No)
        {
            return PermissionResult::Allowed;
        }

        // Plan mode: only allow read-only commands.
        if _ctx.is_plan_mode {
            if is_plan_mode_allowed(command) {
                return PermissionResult::Allowed;
            }
            return PermissionResult::Denied {
                reason: "Plan mode is active. Only read-only commands are allowed during \
                         planning. Use Read, Glob, and Grep tools to explore the codebase, \
                         or run read-only shell commands (e.g. ls, cat, grep, git status)."
                    .to_string(),
            };
        }

        // Read-only commands are always allowed
        if is_read_only_command(command) {
            return PermissionResult::Allowed;
        }

        // Argv-based safe command check: handles compound commands (&&, ||, ;, |)
        // by recursively verifying each sub-command, and applies per-binary rules
        // (e.g. find -exec, git branch -d, rg --pre, base64 --output).
        let shell_argv = vec!["bash".to_string(), "-lc".to_string(), command.to_string()];
        if cocode_shell_parser::is_known_safe_command(&shell_argv) {
            return PermissionResult::Allowed;
        }

        // Run security analysis using cocode-shell-parser
        let (parsed, analysis) = cocode_shell_parser::parse_and_analyze(command);

        if analysis.has_risks() {
            // Deny-phase risks → Deny immediately (injection vectors)
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
                let risks: Vec<SecurityRisk> = ask_phase_risks
                    .iter()
                    .map(|r| SecurityRisk {
                        risk_type: match r.kind {
                            cocode_shell_parser::security::RiskKind::NetworkExfiltration => {
                                RiskType::Network
                            }
                            cocode_shell_parser::security::RiskKind::PrivilegeEscalation => {
                                RiskType::Elevated
                            }
                            cocode_shell_parser::security::RiskKind::FileSystemTampering => {
                                RiskType::Destructive
                            }
                            cocode_shell_parser::security::RiskKind::SensitiveRedirect => {
                                RiskType::SensitiveFile
                            }
                            cocode_shell_parser::security::RiskKind::CodeExecution => {
                                RiskType::SystemConfig
                            }
                            _ => RiskType::Unknown,
                        },
                        severity: match r.level {
                            cocode_shell_parser::security::RiskLevel::Low => RiskSeverity::Low,
                            cocode_shell_parser::security::RiskLevel::Medium => {
                                RiskSeverity::Medium
                            }
                            cocode_shell_parser::security::RiskLevel::High => RiskSeverity::High,
                            cocode_shell_parser::security::RiskLevel::Critical => {
                                RiskSeverity::Critical
                            }
                        },
                        message: r.message.clone(),
                    })
                    .collect();

                let description = if command.len() > 120 {
                    format!("{}...", &command[..120])
                } else {
                    command.to_string()
                };

                return PermissionResult::NeedsApproval {
                    request: ApprovalRequest {
                        request_id: format!("bash-security-{}", uuid::Uuid::new_v4()),
                        tool_name: cocode_protocol::ToolName::Bash.as_str().to_string(),
                        description,
                        risks,
                        allow_remember: true,
                        proposed_prefix_pattern: None,
                        input: Some(input.clone()),
                    },
                };
            }
        }

        // Check compound command risks (subcommand count, multiple cd, cd+git write)
        {
            let commands = parsed.extract_commands();
            if let Some(reason) = check_compound_risks(&commands) {
                let description = if command.len() > 120 {
                    format!("{}...", &command[..120])
                } else {
                    command.to_string()
                };
                return PermissionResult::NeedsApproval {
                    request: ApprovalRequest {
                        request_id: format!("bash-compound-{}", uuid::Uuid::new_v4()),
                        tool_name: cocode_protocol::ToolName::Bash.as_str().to_string(),
                        description: format!("{reason}: {description}"),
                        risks: vec![],
                        allow_remember: true,
                        proposed_prefix_pattern: None,
                        input: Some(input.clone()),
                    },
                };
            }
        }

        // Non-read-only command with no detected risks → still needs approval
        let description = if command.len() > 120 {
            format!("{}...", &command[..120])
        } else {
            command.to_string()
        };
        // Annotate when sandbox bypass is active so the user knows this runs unsandboxed
        let description = if bypass_requested && _ctx.sandbox_state.is_some() {
            format!("{description} (unsandboxed)")
        } else {
            description
        };
        PermissionResult::NeedsApproval {
            request: ApprovalRequest {
                request_id: format!("bash-cmd-{}", uuid::Uuid::new_v4()),
                tool_name: cocode_protocol::ToolName::Bash.as_str().to_string(),
                description,
                risks: vec![],
                allow_remember: true,
                proposed_prefix_pattern: None,
                input: Some(input.clone()),
            },
        }
    }

    async fn execute(&self, input: Value, ctx: &mut ToolContext) -> Result<ToolOutput> {
        let command = input["command"].as_str().ok_or_else(|| {
            crate::error::tool_error::InvalidInputSnafu {
                message: "command must be a string",
            }
            .build()
        })?;

        let timeout_ms = input["timeout"]
            .as_i64()
            .unwrap_or(DEFAULT_TIMEOUT_SECS * 1000);
        let timeout_secs = (timeout_ms / 1000).min(MAX_TIMEOUT_SECS);
        let run_in_background = super::input_helpers::bool_or(&input, "run_in_background", false);
        let sandbox_bypass = cocode_sandbox::SandboxBypass::from_flag(
            super::input_helpers::bool_or(&input, "dangerouslyDisableSandbox", false),
        );

        // Emit progress
        let desc = input["description"].as_str().unwrap_or("Executing command");
        ctx.emit_progress(desc).await;

        // Background execution — delegate to ShellExecutor
        if run_in_background {
            let task_id = ctx
                .shell_executor
                .spawn_background(command)
                .await
                .map_err(|e| {
                    crate::error::tool_error::ExecutionFailedSnafu { message: e }.build()
                })?;

            return Ok(ToolOutput::text(format!(
                "Background task started with ID: {task_id}\n\
                 Use TaskOutput tool with task_id=\"{task_id}\" to retrieve output."
            )));
        }

        // Foreground execution — delegate to ShellExecutor with backgrounding support
        match ctx
            .shell_executor
            .execute_backgroundable_with_cwd_tracking(
                command,
                timeout_secs,
                &ctx.call_id,
                sandbox_bypass,
            )
            .await
        {
            ExecuteResult::Completed(mut result) => {
                // Sync CWD back to ctx only on success
                if result.exit_code == 0
                    && let Some(ref new_cwd) = result.new_cwd
                {
                    ctx.cwd = new_cwd.clone();
                }

                // Annotate sandbox violations in stderr (matches Claude Code's pattern)
                if result.sandboxed
                    && let Some(ref state) = ctx.sandbox_state
                {
                    let store = state.violations().lock().await;
                    let recent = store.recent(10);
                    let non_benign: Vec<_> = recent.iter().filter(|v| !v.benign).collect();
                    if !non_benign.is_empty() {
                        if !result.stderr.is_empty() {
                            result.stderr.push('\n');
                        }
                        result
                            .stderr
                            .push_str("--- Sandbox Violations Detected ---\n");
                        for v in &non_benign {
                            if let Some(ref path) = v.path {
                                result
                                    .stderr
                                    .push_str(&format!("  {} {path}\n", v.operation));
                            } else {
                                result.stderr.push_str(&format!("  {}\n", v.operation));
                            }
                        }
                    }
                }

                format_command_result(&result)
            }
            ExecuteResult::Backgrounded { task_id } => Ok(ToolOutput::text(format!(
                "Command was backgrounded by user (Ctrl+B).\n\
                 Background task ID: {task_id}\n\
                 Use TaskOutput tool with task_id=\"{task_id}\" to retrieve output."
            ))),
        }
    }
}

/// Convert a [`CommandResult`] to a [`ToolOutput`].
fn format_command_result(result: &CommandResult) -> Result<ToolOutput> {
    let mut text = String::new();
    if !result.stdout.is_empty() {
        text.push_str(&result.stdout);
    }
    if !result.stderr.is_empty() {
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str("STDERR:\n");
        text.push_str(&result.stderr);
    }

    if result.exit_code != 0 {
        if text.is_empty() {
            text = format!("Command failed with exit code {}", result.exit_code);
        } else {
            text.push_str(&format!("\n\nExit code: {}", result.exit_code));
        }
        return Ok(ToolOutput::error(text));
    }

    if text.is_empty() {
        text = "(no output)".to_string();
    }
    Ok(ToolOutput::text(text))
}

#[cfg(test)]
#[path = "bash.test.rs"]
mod tests;
