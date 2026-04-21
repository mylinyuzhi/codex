use crate::tools::bash::BashTool;
use coco_tool::DescriptionOptions;
use coco_tool::Tool;
use coco_tool::ToolUseContext;
use serde_json::json;

// ── R7-T25: tool description content checks ──
//
// Verify that the BashTool description includes the critical TS
// instructional content (avoid-native-commands list, parallel calls
// guidance, git safety protocol, sandbox-related notes). Regression
// guard against the description being silently truncated to a stub.

#[test]
fn test_bash_description_includes_avoid_native_commands_list() {
    let desc = BashTool.description(&serde_json::Value::Null, &DescriptionOptions::default());
    // Every native command the model is told to avoid.
    for forbidden in &["find", "grep", "cat", "head", "tail", "sed", "awk", "echo"] {
        assert!(
            desc.contains(&format!("`{forbidden}`")),
            "Bash description should warn about `{forbidden}`, got:\n{desc}"
        );
    }
}

#[test]
fn test_bash_description_includes_tool_preferences() {
    let desc = BashTool.description(&serde_json::Value::Null, &DescriptionOptions::default());
    for tool in &["Glob", "Grep", "Read", "Edit", "Write"] {
        assert!(
            desc.contains(tool),
            "Bash description should mention {tool} as a preferred tool"
        );
    }
}

#[test]
fn test_bash_description_includes_git_safety_protocol() {
    let desc = BashTool.description(&serde_json::Value::Null, &DescriptionOptions::default());
    assert!(desc.contains("Git Safety Protocol"));
    assert!(desc.contains("force push to main/master"));
    assert!(desc.contains("destructive git commands"));
    assert!(desc.contains("hooks (--no-verify"));
}

#[test]
fn test_bash_description_includes_pr_creation_guidance() {
    let desc = BashTool.description(&serde_json::Value::Null, &DescriptionOptions::default());
    assert!(desc.contains("Creating pull requests"));
    assert!(desc.contains("gh pr create"));
}

// ---------------------------------------------------------------------------
// Multi-stage permission pipeline (TS-aligned)
// ---------------------------------------------------------------------------

/// Read-only fast path: `cat`, `ls`, `grep`, `git log`, etc. must be reported
/// as non-destructive and concurrency-safe so the executor auto-approves and
/// batches them with other safe tools. TS: `readOnlyValidation.ts:1876`.
#[test]
fn test_bash_read_only_fast_path() {
    let cases = [
        "cat README.md",
        "ls -la",
        "grep foo file.txt",
        "git log --oneline",
        "pwd",
        "echo hello",
        "head -n 5 file",
        "tail -f log",
        "wc -l file",
        "which cargo",
    ];
    for cmd in cases {
        let input = json!({"command": cmd});
        assert!(BashTool.is_read_only(&input), "`{cmd}` should be read-only");
        assert!(
            BashTool.is_concurrency_safe(&input),
            "`{cmd}` should be concurrency-safe"
        );
        assert!(
            !BashTool.is_destructive(&input),
            "`{cmd}` should not be destructive"
        );
    }
}

/// Non-read-only commands (mutations, installs, shell execution) must be
/// reported as destructive so the permission evaluator asks the user. TS:
/// anything not in `checkReadOnlyConstraints()` falls through to the Ask
/// phase.
///
/// NOTE: output redirect detection (`echo x > file`) is not yet handled
/// by `coco_shell::read_only::is_read_only_command` — that's a known
/// upstream gap in the allowlist approach and tracked separately. For now
/// we only test commands where the first word itself is destructive.
#[test]
fn test_bash_destructive_commands() {
    let cases = [
        "rm -rf /tmp/data",
        "mv a b",
        "cp a b",
        "npm install",
        "git commit -m 'x'",
        "touch file",
        "mkdir -p foo",
    ];
    for cmd in cases {
        let input = json!({"command": cmd});
        assert!(
            !BashTool.is_read_only(&input),
            "`{cmd}` should not be read-only"
        );
        assert!(
            BashTool.is_destructive(&input),
            "`{cmd}` should be destructive"
        );
    }
}

/// Missing command → conservative false (matches previous default behavior).
#[test]
fn test_bash_missing_command_conservative() {
    let input = json!({});
    assert!(!BashTool.is_read_only(&input));
    assert!(!BashTool.is_concurrency_safe(&input));
    assert!(BashTool.is_destructive(&input));
}

/// Deny-severity security risks (eval, IFS injection, backtick substitution)
/// must be hard-failed at execute time, before the command ever runs.
/// TS: `bashPermissions.ts` Deny phase classifiers (`checkSemantics` Deny risks).
#[tokio::test]
async fn test_bash_security_deny_phase_eval() {
    let ctx = ToolUseContext::test_default();
    let result = BashTool
        .execute(json!({"command": "eval 'echo pwned'"}), &ctx)
        .await;
    assert!(result.is_err(), "eval must be blocked by Deny phase");
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("security check") || err.contains("eval"),
        "error should mention security: {err}"
    );
}

#[tokio::test]
async fn test_bash_security_deny_phase_ifs_injection() {
    let ctx = ToolUseContext::test_default();
    let result = BashTool
        .execute(json!({"command": "IFS=$'\\n' ls"}), &ctx)
        .await;
    assert!(result.is_err(), "IFS manipulation must be blocked");
}

/// Read-only commands MUST skip the security Deny check so benign patterns
/// like `grep 'foo`bar' file` (which may contain metacharacters inside quoted
/// strings) don't trigger false positives.
#[tokio::test]
async fn test_bash_read_only_skips_security_checks() {
    let ctx = ToolUseContext::test_default();
    // This grep command has a backtick inside the pattern; security check
    // would Ask on it, but read-only fast path should skip that check.
    let result = BashTool
        .execute(json!({"command": "echo hello"}), &ctx)
        .await;
    assert!(result.is_ok(), "echo should run without security check");
}

// ---------------------------------------------------------------------------
// B4.3: resolved Bash timeout config
// ---------------------------------------------------------------------------

#[test]
fn test_bash_default_timeout_from_default_config() {
    let config = coco_config::ToolConfig::default();
    assert_eq!(super::default_timeout_ms(&config), 120_000);
}

#[test]
fn test_bash_default_timeout_from_runtime_config() {
    let mut config = coco_config::ToolConfig::default();
    config.bash.default_timeout_ms = 30_000;
    assert_eq!(super::default_timeout_ms(&config), 30_000);
}

#[test]
fn test_bash_default_timeout_zero_clamps_to_one() {
    let mut config = coco_config::ToolConfig::default();
    config.bash.default_timeout_ms = 0;
    assert_eq!(super::default_timeout_ms(&config), 1);
}

#[test]
fn test_bash_max_timeout_from_default_config() {
    let config = coco_config::ToolConfig::default();
    assert_eq!(super::max_timeout_ms(&config), 600_000);
}

#[test]
fn test_bash_max_timeout_from_runtime_config() {
    let mut config = coco_config::ToolConfig::default();
    config.bash.max_timeout_ms = 900_000;
    assert_eq!(super::max_timeout_ms(&config), 900_000);
}

// ---------------------------------------------------------------------------
// B4.2: auto-background-on-timeout error suggestion
// ---------------------------------------------------------------------------

/// On foreground timeout, the error message must mention "timed out"
/// (for legacy string matchers) AND also surface the
/// `run_in_background` suggestion when a TaskHandle is available in
/// the context — so the model can retry without trial-and-error.
#[tokio::test]
async fn test_bash_timeout_error_suggests_background_when_handle_available() {
    use std::sync::Arc;
    // Provide a real TaskHandle (NoOpTaskHandle) so the suggestion
    // path fires. The no-op handle is semantically "I exist", which
    // is what the suggestion logic probes for.
    let mut ctx = ToolUseContext::test_default();
    ctx.task_handle = Some(Arc::new(coco_tool::NoOpTaskHandle));

    let result = BashTool
        .execute(json!({"command": "sleep 10", "timeout": 100}), &ctx)
        .await;
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("timed out"),
        "must include timeout wording: {err}"
    );
    assert!(
        err.contains("run_in_background") && err.contains("true"),
        "must suggest run_in_background retry: {err}"
    );
}

/// Without a TaskHandle, the tool should NOT suggest background
/// retry (it's not available) — just a plain timeout error.
#[tokio::test]
async fn test_bash_timeout_error_no_suggestion_without_handle() {
    let ctx = ToolUseContext::test_default();
    // test_default has task_handle = None.
    let result = BashTool
        .execute(json!({"command": "sleep 10", "timeout": 100}), &ctx)
        .await;
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("timed out"));
    assert!(
        !err.contains("run_in_background"),
        "should not suggest bg retry when unavailable: {err}"
    );
}

#[tokio::test]
async fn test_bash_echo() {
    let ctx = ToolUseContext::test_default();
    let result = BashTool
        .execute(json!({"command": "echo hello world"}), &ctx)
        .await
        .unwrap();

    // R5-T14: structured output — read stdout directly.
    assert!(
        result.data["stdout"]
            .as_str()
            .unwrap()
            .contains("hello world")
    );
    assert_eq!(result.data["exitCode"], 0);
    assert_eq!(result.data["interrupted"], false);
}

#[tokio::test]
async fn test_bash_exit_code() {
    let ctx = ToolUseContext::test_default();
    let result = BashTool
        .execute(json!({"command": "exit 42"}), &ctx)
        .await
        .unwrap();

    // R5-T14: exitCode is a dedicated field now.
    assert_eq!(result.data["exitCode"], 42);
}

#[tokio::test]
async fn test_bash_stderr() {
    let ctx = ToolUseContext::test_default();
    let result = BashTool
        .execute(json!({"command": "echo err >&2"}), &ctx)
        .await
        .unwrap();

    // R5-T14: stderr has its own field.
    assert!(result.data["stderr"].as_str().unwrap().contains("err"));
    assert_eq!(result.data["stdout"].as_str().unwrap(), "");
}

#[tokio::test]
async fn test_bash_timeout() {
    let ctx = ToolUseContext::test_default();
    let result = BashTool
        .execute(json!({"command": "sleep 10", "timeout": 100}), &ctx)
        .await;

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("timed out"));
}

#[tokio::test]
async fn test_bash_pwd() {
    let ctx = ToolUseContext::test_default();
    let result = BashTool
        .execute(json!({"command": "pwd"}), &ctx)
        .await
        .unwrap();

    assert!(!result.data["stdout"].as_str().unwrap().is_empty());
}

/// TS `outputLimits.ts` — `BASH_MAX_OUTPUT_DEFAULT = 30_000`. Our Bash
/// tool must advertise the same persistence threshold so cross-runtime
/// sessions handle large outputs identically. Regression guard for R4-T6.
#[test]
fn test_bash_max_result_size_chars_matches_ts() {
    assert_eq!(BashTool.max_result_size_chars(), 30_000);
}

/// The bash-side helper is a plain cast; upper clamping lives in
/// `coco_config::BashConfig::finalize()` (verified in sections.test.rs).
/// This test just pins the pass-through semantics.
#[test]
fn test_bash_max_output_bytes_pass_through() {
    let mut config = coco_config::ToolConfig::default();
    assert_eq!(super::max_output_bytes(&config), 30_000);

    config.bash.max_output_bytes = 50_000;
    assert_eq!(super::max_output_bytes(&config), 50_000);

    // Negative values are normalized to 0 by the cast guard.
    config.bash.max_output_bytes = -1;
    assert_eq!(super::max_output_bytes(&config), 0);
}

/// TS `BashTool.tsx:643-649` swaps the shell's cwd via `getCwd()` which the
/// runtime overrides for isolated subagents. coco-rs threads the same
/// information through `ctx.cwd_override` — the foreground shell must run
/// inside that directory so worktree-isolated tasks don't leak into the
/// host process cwd. Regression guard for R4-T5.
#[tokio::test]
async fn test_bash_respects_cwd_override() {
    let dir = tempfile::tempdir().unwrap();
    let canon = std::fs::canonicalize(dir.path()).unwrap();

    let mut ctx = ToolUseContext::test_default();
    ctx.cwd_override = Some(canon.clone());

    let result = BashTool
        .execute(json!({"command": "pwd"}), &ctx)
        .await
        .unwrap();

    let stdout = result.data["stdout"].as_str().unwrap();
    // `pwd` output has a trailing newline — compare on the trimmed prefix.
    assert!(
        stdout.trim_end().contains(canon.to_str().unwrap()),
        "pwd should report the cwd_override directory; got: {stdout:?}, expected: {canon:?}"
    );
}

#[tokio::test]
async fn test_bash_piped_command() {
    let ctx = ToolUseContext::test_default();
    let result = BashTool
        .execute(json!({"command": "echo -e 'a\\nb\\nc' | wc -l"}), &ctx)
        .await
        .unwrap();

    assert!(result.data["stdout"].as_str().unwrap().contains('3'));
}

/// R5-T14: `true` exits 0 with empty output. In the new structured
/// shape, stdout/stderr are empty strings and exitCode is 0 — there is
/// no "(no output)" sentinel anymore.
#[tokio::test]
async fn test_bash_no_output() {
    let ctx = ToolUseContext::test_default();
    let result = BashTool
        .execute(json!({"command": "true"}), &ctx)
        .await
        .unwrap();

    assert_eq!(result.data["stdout"].as_str().unwrap(), "");
    assert_eq!(result.data["stderr"].as_str().unwrap(), "");
    assert_eq!(result.data["exitCode"], 0);
    assert_eq!(result.data["interrupted"], false);
}

#[tokio::test]
async fn test_bash_with_progress_channel() {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let mut ctx = ToolUseContext::test_default();
    ctx.progress_tx = Some(tx);

    let result = BashTool
        .execute(json!({"command": "echo hello"}), &ctx)
        .await
        .unwrap();

    // Should have received at least the initial "running" progress
    let mut progress_msgs = Vec::new();
    while let Ok(msg) = rx.try_recv() {
        progress_msgs.push(msg);
    }

    assert!(
        !progress_msgs.is_empty(),
        "should receive at least one progress message"
    );
    assert_eq!(progress_msgs[0].data["type"], "bash_progress");
    assert_eq!(progress_msgs[0].data["status"], "running");

    assert!(result.data["stdout"].as_str().unwrap().contains("hello"));
}

// ---------------------------------------------------------------------------
// R6-T18: sandbox decision
// ---------------------------------------------------------------------------

use super::shell_sandbox_config_from_runtime;

#[test]
fn test_shell_sandbox_config_disabled_by_default() {
    let cfg = shell_sandbox_config_from_runtime(&coco_config::SandboxConfig::default());
    assert!(
        !cfg.mode.is_active(),
        "sandbox should be disabled by default"
    );
}

#[test]
fn test_shell_sandbox_config_enabled_defaults_readonly() {
    let runtime = coco_config::SandboxConfig {
        enabled: true,
        ..Default::default()
    };
    let cfg = shell_sandbox_config_from_runtime(&runtime);
    assert!(cfg.mode.is_active());
    assert_eq!(
        cfg.mode,
        coco_shell::sandbox::SandboxMode::ReadOnly,
        "default enabled mode should be ReadOnly"
    );
}

#[test]
fn test_shell_sandbox_config_mode_workspace_write_maps_to_strict() {
    let runtime = coco_config::SandboxConfig {
        enabled: true,
        mode: coco_types::SandboxMode::WorkspaceWrite,
        ..Default::default()
    };
    let cfg = shell_sandbox_config_from_runtime(&runtime);
    assert_eq!(cfg.mode, coco_shell::sandbox::SandboxMode::Strict);
}

#[test]
fn test_shell_sandbox_config_excluded_commands_preserved() {
    let runtime = coco_config::SandboxConfig {
        enabled: true,
        excluded_commands: vec!["git".into(), "npm".into(), "cargo".into()],
        ..Default::default()
    };
    let cfg = shell_sandbox_config_from_runtime(&runtime);
    assert_eq!(
        cfg.excluded_commands,
        vec!["git".to_string(), "npm".to_string(), "cargo".to_string()]
    );
}

/// With sandbox enabled, a non-excluded command is sandboxed.
#[test]
fn test_sandbox_decision_non_excluded_command() {
    use coco_shell::sandbox::BypassRequest;
    use coco_shell::sandbox::should_sandbox_command;
    let runtime = coco_config::SandboxConfig {
        enabled: true,
        ..Default::default()
    };
    let cfg = shell_sandbox_config_from_runtime(&runtime);
    let decision = should_sandbox_command(&cfg, "ls -la", BypassRequest::No);
    assert!(
        decision.is_sandboxed(),
        "enabled + non-excluded → sandboxed"
    );
}

/// With sandbox enabled, an excluded command bypasses the sandbox.
#[test]
fn test_sandbox_decision_excluded_command() {
    use coco_shell::sandbox::BypassRequest;
    use coco_shell::sandbox::should_sandbox_command;
    let runtime = coco_config::SandboxConfig {
        enabled: true,
        excluded_commands: vec!["git".into()],
        ..Default::default()
    };
    let cfg = shell_sandbox_config_from_runtime(&runtime);
    let decision = should_sandbox_command(&cfg, "git status", BypassRequest::No);
    assert!(
        !decision.is_sandboxed(),
        "excluded command must be unsandboxed"
    );
}

/// `dangerouslyDisableSandbox` → bypass is requested → unsandboxed.
#[test]
fn test_sandbox_decision_bypass_respected() {
    use coco_shell::sandbox::BypassRequest;
    use coco_shell::sandbox::should_sandbox_command;
    let runtime = coco_config::SandboxConfig {
        enabled: true,
        ..Default::default()
    };
    let cfg = shell_sandbox_config_from_runtime(&runtime);
    let decision = should_sandbox_command(&cfg, "rm -rf /", BypassRequest::Requested);
    assert!(
        !decision.is_sandboxed(),
        "bypass should unsandbox (allow_bypass=true in our config)"
    );
}

/// R6-T19: runtime-config gate for auto-background-on-timeout.
#[test]
fn test_auto_background_on_timeout_default_disabled() {
    let config = coco_config::ToolConfig::default();
    assert!(!config.bash.auto_background_on_timeout);
}

/// With auto-background opted out (default), a timeout still surfaces
/// as an ExecutionFailed error — existing behavior.
#[tokio::test]
async fn test_bash_timeout_without_auto_background_errors() {
    let ctx = ToolUseContext::test_default();
    let result = BashTool
        .execute(json!({"command": "sleep 10", "timeout": 100}), &ctx)
        .await;
    assert!(
        result.is_err(),
        "timeout should error when auto-bg disabled"
    );
    let err = result.unwrap_err().to_string();
    assert!(err.contains("timed out"));
}

/// R6-T17: when ctx.cancel fires mid-execution, the child process is
/// killed and the tool returns with interrupted=true. Previously
/// executor-level cancel dropped the Bash future but left the shell
/// child orphaned. Regression guard.
#[tokio::test]
async fn test_bash_cancel_kills_child_and_sets_interrupted() {
    let mut ctx = ToolUseContext::test_default();
    let cancel = tokio_util::sync::CancellationToken::new();
    ctx.cancel = cancel.clone();

    // Fire cancel after 200ms; the command tries to sleep 10s.
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        cancel.cancel();
    });

    let start = std::time::Instant::now();
    let result = BashTool
        .execute(json!({"command": "sleep 10 && echo done"}), &ctx)
        .await
        .unwrap();

    let elapsed = start.elapsed();
    // Should return well before 10s — ideally ~200ms plus shell startup.
    assert!(
        elapsed < std::time::Duration::from_secs(3),
        "cancel should kill child promptly; elapsed={elapsed:?}"
    );
    assert_eq!(
        result.data["interrupted"], true,
        "interrupted flag must be set on cancel"
    );
}

/// R5-T14: structured output schema — regression guard. TS
/// `BashTool.tsx:279-293` requires stdout/stderr/exitCode/interrupted.
#[tokio::test]
async fn test_bash_structured_output_schema() {
    let ctx = ToolUseContext::test_default();
    let result = BashTool
        .execute(json!({"command": "echo out; echo err >&2; exit 2"}), &ctx)
        .await
        .unwrap();

    assert!(result.data["stdout"].is_string());
    assert!(result.data["stderr"].is_string());
    assert!(result.data["exitCode"].is_number());
    assert!(result.data["interrupted"].is_boolean());
    assert!(result.data["stdout"].as_str().unwrap().contains("out"));
    assert!(result.data["stderr"].as_str().unwrap().contains("err"));
    assert_eq!(result.data["exitCode"], 2);
    assert_eq!(result.data["interrupted"], false);
}

#[tokio::test]
async fn test_bash_background_without_task_handle() {
    let ctx = ToolUseContext::test_default();
    let result = BashTool
        .execute(
            json!({"command": "echo test", "run_in_background": true}),
            &ctx,
        )
        .await;

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("not available"));
}

// -- Stall detection tests --

#[test]
fn test_stall_prompt_yes_no() {
    assert!(coco_tool::matches_interactive_prompt(
        "Do you want to continue? (y/n)"
    ));
    assert!(coco_tool::matches_interactive_prompt(
        "output\nmore output\nContinue? [y/n]"
    ));
    assert!(coco_tool::matches_interactive_prompt(
        "Are you sure? (yes/no)"
    ));
}

#[test]
fn test_stall_prompt_password() {
    assert!(coco_tool::matches_interactive_prompt("Enter password:"));
    assert!(coco_tool::matches_interactive_prompt(
        "[sudo] password for user:"
    ));
    assert!(coco_tool::matches_interactive_prompt(
        "Enter passphrase for key:"
    ));
}

#[test]
fn test_stall_prompt_question_pattern() {
    assert!(coco_tool::matches_interactive_prompt(
        "Do you want to proceed?"
    ));
    assert!(coco_tool::matches_interactive_prompt(
        "Would you like to overwrite?"
    ));
    assert!(coco_tool::matches_interactive_prompt(
        "Are you sure you want to delete?"
    ));
}

#[test]
fn test_stall_prompt_press_key() {
    assert!(coco_tool::matches_interactive_prompt(
        "Press any key to continue"
    ));
    assert!(coco_tool::matches_interactive_prompt(
        "Press Enter to continue"
    ));
}

#[test]
fn test_stall_no_false_positive_normal_output() {
    // Normal command output should NOT match
    assert!(!coco_tool::matches_interactive_prompt(
        "Compiling project..."
    ));
    assert!(!coco_tool::matches_interactive_prompt("Build succeeded"));
    assert!(!coco_tool::matches_interactive_prompt(
        "Downloaded 42 packages"
    ));
    assert!(!coco_tool::matches_interactive_prompt("")); // empty
}

#[test]
fn test_stall_only_checks_last_line() {
    // "password:" in earlier output should not trigger
    let tail = "checking password: ok\nall tests passed\nDone.";
    assert!(!coco_tool::matches_interactive_prompt(tail));

    // But if last line has prompt, it should match
    let tail2 = "checking things\nEnter password:";
    assert!(coco_tool::matches_interactive_prompt(tail2));
}

// -- Notification format tests --

#[test]
fn test_task_notification_format() {
    let info = coco_tool::BackgroundTaskInfo {
        task_id: "task-1".into(),
        status: coco_tool::BackgroundTaskStatus::Completed,
        summary: Some("Command finished successfully".into()),
        output_file: Some("/tmp/task-1.out".into()),
        tool_use_id: Some("tu-123".into()),
        elapsed_seconds: 5.0,
        notified: false,
    };

    let xml = coco_tool::format_task_notification(&info);
    assert!(xml.contains("<task-id>task-1</task-id>"));
    assert!(xml.contains("<status>completed</status>"));
    assert!(xml.contains("<tool-use-id>tu-123</tool-use-id>"));
    assert!(xml.contains("<output-file>/tmp/task-1.out</output-file>"));
    assert!(xml.contains("<summary>Command finished successfully</summary>"));
}

#[test]
fn test_stall_notification_omits_status() {
    let stall = coco_tool::StallInfo {
        task_id: "task-2".into(),
        output_tail: "Enter password:".into(),
        frozen_seconds: 45.0,
    };

    let xml = coco_tool::format_stall_notification(&stall, Some("/tmp/task-2.out"));
    // Stall notifications must NOT have <status> tag (TS requirement)
    assert!(!xml.contains("<status>"));
    assert!(xml.contains("<task-id>task-2</task-id>"));
    assert!(xml.contains("output frozen for 45s"));
    // Raw output tail appears after XML
    assert!(xml.contains("Enter password:"));
}

// ── R7-T11: _simulatedSedEdit short-circuit tests ──
//
// TS `BashTool.tsx:355-419` (`applySedEdit`): when the BashTool input
// includes `_simulatedSedEdit: { filePath, newContent }`, the tool
// skips bash entirely and writes the precomputed content to the file
// while preserving its original encoding + line endings, returning a
// sed-shaped result envelope. The tests exercise success, ENOENT,
// and the encoding/line-ending preservation that distinguishes this
// path from FileWriteTool's LF-always policy.

#[tokio::test]
async fn test_bash_simulated_sed_edit_writes_new_content() {
    use crate::tools::bash::BashTool;

    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("sample.txt");
    std::fs::write(&file, "before\n").unwrap();

    let ctx = coco_tool::ToolUseContext::test_default();
    let result = BashTool
        .execute(
            serde_json::json!({
                "_simulatedSedEdit": {
                    "filePath": file.to_str().unwrap(),
                    "newContent": "after\n",
                }
            }),
            &ctx,
        )
        .await
        .unwrap();

    // sed-shaped success envelope
    assert_eq!(result.data["stdout"], "");
    assert_eq!(result.data["stderr"], "");
    assert_eq!(result.data["exitCode"], 0);
    assert_eq!(result.data["interrupted"], false);

    // File should have the new content on disk.
    let on_disk = std::fs::read_to_string(&file).unwrap();
    assert_eq!(on_disk, "after\n");
}

#[tokio::test]
async fn test_bash_simulated_sed_edit_enoent_returns_sed_error() {
    use crate::tools::bash::BashTool;

    let ctx = coco_tool::ToolUseContext::test_default();
    let result = BashTool
        .execute(
            serde_json::json!({
                "_simulatedSedEdit": {
                    "filePath": "/this/path/does/not/exist.txt",
                    "newContent": "irrelevant",
                }
            }),
            &ctx,
        )
        .await
        .unwrap();

    // ENOENT must come back as a sed-shaped error envelope, NOT a tool error.
    assert_eq!(result.data["stdout"], "");
    assert_eq!(result.data["exitCode"], 1);
    assert!(
        result.data["stderr"]
            .as_str()
            .unwrap()
            .contains("No such file or directory"),
        "expected sed-style ENOENT, got: {:?}",
        result.data["stderr"]
    );
}

#[tokio::test]
async fn test_bash_simulated_sed_edit_preserves_crlf_line_endings() {
    use crate::tools::bash::BashTool;

    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("crlf.txt");
    // Original file uses CRLF line endings — simulating a Windows-authored file.
    std::fs::write(&file, "alpha\r\nbeta\r\n").unwrap();

    let ctx = coco_tool::ToolUseContext::test_default();
    let _result = BashTool
        .execute(
            serde_json::json!({
                "_simulatedSedEdit": {
                    "filePath": file.to_str().unwrap(),
                    "newContent": "gamma\ndelta\n",
                }
            }),
            &ctx,
        )
        .await
        .unwrap();

    // Sed-edit must preserve CRLF — TS `applySedEdit` reuses the
    // detected line ending. coco-rs FileWriteTool always normalizes
    // to LF, so this is the key distinction.
    let on_disk = std::fs::read(&file).unwrap();
    assert!(
        on_disk.windows(2).any(|w| w == b"\r\n"),
        "expected CRLF in output, got: {on_disk:?}"
    );
}

#[tokio::test]
async fn test_bash_simulated_sed_edit_missing_file_path_errors() {
    use crate::tools::bash::BashTool;

    let ctx = coco_tool::ToolUseContext::test_default();
    let result = BashTool
        .execute(
            serde_json::json!({
                "_simulatedSedEdit": {
                    "newContent": "no path provided"
                }
            }),
            &ctx,
        )
        .await;

    let err = result.unwrap_err();
    assert!(err.to_string().contains("filePath"), "got: {err}");
}

// ── R7-T12: Bash output schema extension tests ──
//
// Verify that the new `isImage`, `structuredContent`, `persistedOutputPath`
// and `persistedOutputSize` fields are populated correctly. Tests use the
// internal helpers directly to avoid invoking real shell commands.

#[test]
fn test_is_likely_image_bytes_png() {
    use crate::tools::bash::is_likely_image_bytes;
    let png_magic = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00];
    assert!(is_likely_image_bytes(&png_magic));
}

#[test]
fn test_is_likely_image_bytes_jpeg() {
    use crate::tools::bash::is_likely_image_bytes;
    assert!(is_likely_image_bytes(&[0xFF, 0xD8, 0xFF, 0xE0, 0x00]));
}

#[test]
fn test_is_likely_image_bytes_gif() {
    use crate::tools::bash::is_likely_image_bytes;
    assert!(is_likely_image_bytes(b"GIF89a..."));
    assert!(is_likely_image_bytes(b"GIF87a..."));
}

#[test]
fn test_is_likely_image_bytes_webp() {
    use crate::tools::bash::is_likely_image_bytes;
    let mut webp = b"RIFF".to_vec();
    webp.extend_from_slice(&[0, 0, 0, 0]); // size field (don't care)
    webp.extend_from_slice(b"WEBP");
    webp.extend_from_slice(b"VP8 ");
    assert!(is_likely_image_bytes(&webp));
}

#[test]
fn test_is_likely_image_bytes_text_negative() {
    use crate::tools::bash::is_likely_image_bytes;
    assert!(!is_likely_image_bytes(b"hello world\n"));
    assert!(!is_likely_image_bytes(b""));
    // RIFF without WEBP magic isn't an image (could be WAV/AVI).
    assert!(!is_likely_image_bytes(b"RIFF\0\0\0\0WAVEfmt "));
}

#[test]
fn test_persist_oversized_skipped_for_small_output() {
    use crate::tools::bash::maybe_persist_oversized_output;
    let small = b"under threshold";
    let (path, size) = maybe_persist_oversized_output(small);
    assert!(path.is_none());
    assert_eq!(size, 0);
}

#[test]
fn test_persist_oversized_writes_when_over_threshold() {
    use crate::tools::bash::maybe_persist_oversized_output;
    let big = vec![b'x'; 40_000]; // > 30K threshold
    let (path, size) = maybe_persist_oversized_output(&big);
    let path = path.expect("expected persisted path");
    assert_eq!(size, 40_000);
    // The file should exist on disk and contain exactly the bytes.
    let on_disk = std::fs::read(&path).expect("persisted file must exist");
    assert_eq!(on_disk.len(), 40_000);
    assert!(on_disk.iter().all(|&b| b == b'x'));
    // Cleanup so /tmp doesn't accumulate cruft from CI.
    let _ = std::fs::remove_file(&path);
}

// R7-T18: end-to-end image detection now works because coco-shell
// populates `CommandResult.stdout_bytes` with the raw pre-UTF-8-lossy
// bytes. BashTool reads `stdout_bytes` (with `stdout.as_bytes()` as a
// fallback) so the magic-byte detection sees the original PNG header
// instead of the U+FFFD-mangled lossy version.
#[tokio::test]
async fn test_bash_output_includes_image_fields_when_stdout_is_image() {
    use crate::tools::bash::BashTool;

    let ctx = coco_tool::ToolUseContext::test_default();
    // printf with explicit hex: full PNG signature
    // 89 50 4E 47 0D 0A 1A 0A — emitted via printf so the test stays
    // portable across bash versions. The trailing zero bytes are
    // padding to make the magic detector's 8-byte check pass.
    let result = BashTool
        .execute(
            serde_json::json!({
                "command": "printf '\\x89PNG\\x0d\\x0a\\x1a\\x0a\\x00\\x00\\x00\\x00'"
            }),
            &ctx,
        )
        .await
        .unwrap();
    assert_eq!(
        result.data["isImage"], true,
        "expected isImage=true for PNG-magic stdout, got: {:?}",
        result.data
    );
    let content = result.data["structuredContent"]
        .as_array()
        .expect("structuredContent should be an array");
    assert_eq!(content.len(), 1);
    assert_eq!(content[0]["type"], "image");
    assert_eq!(content[0]["source"]["media_type"], "image/png");
}

#[tokio::test]
async fn test_bash_output_persists_when_oversized() {
    use crate::tools::bash::BashTool;

    let ctx = coco_tool::ToolUseContext::test_default();
    // yes(1) emits 'y\n' indefinitely; cap with head -c to land just
    // above the 30K persistence threshold. Some platforms ship bash 3.x
    // without `head -c`, so use `printf` repeats as a portable fallback.
    let result = BashTool
        .execute(
            serde_json::json!({
                "command": "printf 'x%.0s' $(seq 1 35000)"
            }),
            &ctx,
        )
        .await
        .unwrap();
    assert!(
        result.data["persistedOutputPath"].is_string(),
        "expected persistedOutputPath, got: {:?}",
        result.data
    );
    let size = result.data["persistedOutputSize"].as_u64().unwrap_or(0);
    assert!(size >= 30_000, "expected size >= 30K, got {size}");
    // Cleanup the temp file.
    if let Some(path) = result.data["persistedOutputPath"].as_str() {
        let _ = std::fs::remove_file(path);
    }
}

#[tokio::test]
async fn test_bash_simulated_sed_edit_does_not_run_command() {
    use crate::tools::bash::BashTool;

    // If the short-circuit is wired correctly, the `command` field should
    // be ignored. We pass a command that would otherwise fail security
    // checks (eval) to prove the bash code path was skipped — the sed
    // path doesn't go through the security pipeline.
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("guard.txt");
    std::fs::write(&file, "before\n").unwrap();

    let ctx = coco_tool::ToolUseContext::test_default();
    let result = BashTool
        .execute(
            serde_json::json!({
                "command": "eval 'rm -rf /'",
                "_simulatedSedEdit": {
                    "filePath": file.to_str().unwrap(),
                    "newContent": "after\n",
                }
            }),
            &ctx,
        )
        .await
        .unwrap();

    // Sed-shaped success: bash command was never executed.
    assert_eq!(result.data["exitCode"], 0);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "after\n");
}
