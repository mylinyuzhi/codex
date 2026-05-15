//! Tests for EnterPlanMode + ExitPlanMode tools.

use super::EnterPlanModeTool;
use super::ExitPlanModeTool;
use coco_tool_runtime::Tool;
use coco_tool_runtime::ToolUseContext;
use coco_tool_runtime::ValidationResult;
use coco_types::AgentId;
use coco_types::PermissionMode;
use coco_types::ToolAppState;
use coco_types::ToolName;
use serde_json::Value;
use serde_json::json;

fn ctx_with_mode(mode: PermissionMode) -> ToolUseContext {
    let mut ctx = ToolUseContext::test_default();
    ctx.permission_context.mode = mode;
    ctx
}

/// Drive a tool's `execute` + apply its `app_state_patch` — the
/// unit-test equivalent of what the real executor does. Needed
/// because the shared `ToolAppState` lives behind
/// `AppStateReadHandle` on `ctx.app_state` (read-only by design —
/// the type system forbids writes), so mutations only hit the
/// store when the patch returned in `ToolResult::app_state_patch`
/// is actually applied.
async fn execute_and_apply_patch(
    tool: &(dyn Tool + Send + Sync),
    input: Value,
    ctx: &ToolUseContext,
    state: &std::sync::Arc<tokio::sync::RwLock<ToolAppState>>,
) -> Result<coco_messages::ToolResult<Value>, coco_tool_runtime::ToolError> {
    let mut result = tool.execute(input, ctx).await?;
    if let Some(patch) = result.app_state_patch.take() {
        let mut guard = state.write().await;
        patch(&mut guard);
    }
    Ok(result)
}

// ── EnterPlanModeTool ──

#[tokio::test]
async fn enter_plan_mode_rejects_in_agent_context() {
    let mut ctx = ctx_with_mode(PermissionMode::Default);
    ctx.agent_id = Some(AgentId::new("aabcdef0"));
    let result = EnterPlanModeTool.execute(json!({}), &ctx).await;
    assert!(result.is_err(), "agent contexts must be rejected");
    assert!(result.unwrap_err().to_string().contains("agent"));
}

#[tokio::test]
async fn enter_plan_mode_returns_confirmation_message() {
    let ctx = ctx_with_mode(PermissionMode::Default);
    let result = EnterPlanModeTool.execute(json!({}), &ctx).await.unwrap();
    let msg = result
        .data
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or_default();
    assert!(msg.contains("Entered plan mode"));
}

#[tokio::test]
async fn enter_plan_mode_stashes_previous_mode() {
    // TS parity: EnterPlanModeTool.ts:88-94 — `setAppState` flips
    // mode → Plan and stashes `prePlanMode: currentMode`. Rust writes
    // the same on app_state via execute().
    use std::sync::Arc;
    use tokio::sync::RwLock;
    let app_state = Arc::new(RwLock::new(ToolAppState {
        permission_mode: Some(PermissionMode::AcceptEdits),
        ..Default::default()
    }));
    let mut ctx = ctx_with_mode(PermissionMode::AcceptEdits);
    ctx.app_state = Some(app_state.clone().into());
    let _ = execute_and_apply_patch(&EnterPlanModeTool, json!({}), &ctx, &app_state)
        .await
        .unwrap();
    let guard = app_state.read().await;
    assert_eq!(guard.permission_mode, Some(PermissionMode::Plan));
    assert_eq!(guard.pre_plan_mode, Some(PermissionMode::AcceptEdits));
}

#[tokio::test]
async fn enter_plan_mode_idempotent_does_not_stash_self() {
    // Calling enter while already in plan mode must NOT overwrite the
    // stash with Plan itself — otherwise exit would have nowhere to
    // return to.
    use std::sync::Arc;
    use tokio::sync::RwLock;
    let app_state = Arc::new(RwLock::new(ToolAppState {
        permission_mode: Some(PermissionMode::Plan),
        pre_plan_mode: Some(PermissionMode::AcceptEdits),
        plan_mode_entry_ms: Some(42),
        ..Default::default()
    }));
    let mut ctx = ctx_with_mode(PermissionMode::Plan);
    ctx.app_state = Some(app_state.clone().into());
    let _ = execute_and_apply_patch(&EnterPlanModeTool, json!({}), &ctx, &app_state)
        .await
        .unwrap();
    let guard = app_state.read().await;
    assert_eq!(guard.pre_plan_mode, Some(PermissionMode::AcceptEdits));
    assert_eq!(guard.plan_mode_entry_ms, Some(42));
}

#[test]
fn enter_plan_mode_schema_has_no_parameters() {
    let schema = EnterPlanModeTool.input_schema();
    assert!(
        schema.properties.is_empty(),
        "EnterPlanMode takes no parameters"
    );
}

// ── ExitPlanModeTool ──

#[test]
fn exit_plan_mode_rejects_when_not_in_plan_mode() {
    let ctx = ctx_with_mode(PermissionMode::Default);
    let vr = ExitPlanModeTool.validate_input(&json!({}), &ctx);
    match vr {
        ValidationResult::Invalid {
            message,
            error_code,
        } => {
            assert!(message.contains("not in plan mode"));
            assert_eq!(error_code.as_deref(), Some("1"));
        }
        ValidationResult::Valid => panic!("expected Invalid outside plan mode"),
    }
}

#[test]
fn exit_plan_mode_allows_when_in_plan_mode() {
    let ctx = ctx_with_mode(PermissionMode::Plan);
    let vr = ExitPlanModeTool.validate_input(&json!({}), &ctx);
    assert!(matches!(vr, ValidationResult::Valid));
}

#[test]
fn exit_plan_mode_teammate_bypasses_validation() {
    let mut ctx = ctx_with_mode(PermissionMode::Default);
    ctx.is_teammate = true;
    let vr = ExitPlanModeTool.validate_input(&json!({}), &ctx);
    assert!(matches!(vr, ValidationResult::Valid));
}

#[tokio::test]
async fn exit_plan_mode_teammate_bypasses_permission_prompt() {
    let mut ctx = ctx_with_mode(PermissionMode::Plan);
    ctx.is_teammate = true;
    let decision = ExitPlanModeTool.check_permissions(&json!({}), &ctx).await;
    assert!(matches!(
        decision,
        coco_types::ToolCheckResult::Allow { .. }
    ));
}

#[tokio::test]
async fn exit_plan_mode_non_teammate_asks_for_confirmation() {
    let ctx = ctx_with_mode(PermissionMode::Plan);
    let decision = ExitPlanModeTool.check_permissions(&json!({}), &ctx).await;
    match decision {
        coco_types::ToolCheckResult::Ask { message, choices } => {
            assert!(message.contains("Exit plan mode"));
            // Default `show_clear_context_on_exit = false` → no choices.
            assert!(choices.is_none(), "no choices when setting is off");
        }
        other => panic!("expected Ask, got {other:?}"),
    }
}

#[tokio::test]
async fn exit_plan_mode_offers_clear_context_choice_when_setting_enabled() {
    // TS parity: `ExitPlanModePermissionRequest.tsx:137` gates the
    // multi-choice dialog on `settings.showClearContextOnPlanAccept`.
    let mut ctx = ctx_with_mode(PermissionMode::Plan);
    ctx.plan_mode_settings.show_clear_context_on_exit = true;

    let decision = ExitPlanModeTool.check_permissions(&json!({}), &ctx).await;
    match decision {
        coco_types::ToolCheckResult::Ask { choices, .. } => {
            let choices = choices.expect("expected choices when setting is on");
            let values: Vec<&str> = choices.iter().map(|c| c.value.as_str()).collect();
            assert_eq!(
                values,
                vec!["yes-keep-context", "yes-clear-context", "no"],
                "choices must surface the keep/clear/cancel triad"
            );
        }
        other => panic!("expected Ask, got {other:?}"),
    }
}

#[tokio::test]
async fn exit_plan_mode_clear_context_choice_sets_pending_flag() {
    use tempfile::tempdir;
    let tmp = tempdir().unwrap();
    let session_id = "exit-clear-ctx";
    let plans_dir = coco_context::resolve_plans_directory(tmp.path(), None, None);
    coco_context::write_plan(session_id, &plans_dir, "# plan", None).unwrap();

    let app_state = plan_mode_app_state(Some(PermissionMode::Default), None);
    let mut ctx = ctx_with_mode(PermissionMode::Plan);
    ctx.config_home = Some(tmp.path().to_path_buf());
    ctx.session_id_for_history = Some(session_id.into());
    ctx.app_state = Some(app_state.clone().into());

    // Simulate the TUI rewriting input with the picked choice value.
    let input = json!({"user_choice": "yes-clear-context"});
    let _ = execute_and_apply_patch(&ExitPlanModeTool, input, &ctx, &app_state)
        .await
        .unwrap();
    let guard = app_state.read().await;
    assert!(
        guard.pending_clear_message_history,
        "yes-clear-context must schedule MessageHistory::clear()"
    );
}

#[tokio::test]
async fn exit_plan_mode_keep_context_choice_does_not_set_pending_flag() {
    use tempfile::tempdir;
    let tmp = tempdir().unwrap();
    let session_id = "exit-keep-ctx";
    let plans_dir = coco_context::resolve_plans_directory(tmp.path(), None, None);
    coco_context::write_plan(session_id, &plans_dir, "# plan", None).unwrap();

    let app_state = plan_mode_app_state(Some(PermissionMode::Default), None);
    let mut ctx = ctx_with_mode(PermissionMode::Plan);
    ctx.config_home = Some(tmp.path().to_path_buf());
    ctx.session_id_for_history = Some(session_id.into());
    ctx.app_state = Some(app_state.clone().into());

    let input = json!({"user_choice": "yes-keep-context"});
    let _ = execute_and_apply_patch(&ExitPlanModeTool, input, &ctx, &app_state)
        .await
        .unwrap();
    let guard = app_state.read().await;
    assert!(
        !guard.pending_clear_message_history,
        "yes-keep-context must NOT schedule a clear"
    );
}

/// Seed app_state for an ExitPlanMode test. TS parity: appState is
/// fully initialized at session bootstrap — we do the Rust equivalent
/// by writing the three mode-related fields up front.
fn plan_mode_app_state(
    pre_plan: Option<PermissionMode>,
    stripped: Option<coco_types::PermissionRulesBySource>,
) -> std::sync::Arc<tokio::sync::RwLock<ToolAppState>> {
    std::sync::Arc::new(tokio::sync::RwLock::new(ToolAppState {
        permission_mode: Some(PermissionMode::Plan),
        pre_plan_mode: pre_plan,
        stripped_dangerous_rules: stripped,
        ..Default::default()
    }))
}

#[tokio::test]
async fn exit_plan_mode_restores_previous_mode() {
    use tempfile::tempdir;
    let tmp = tempdir().unwrap();
    let session_id = "exit-restores-prev";
    let plans_dir = coco_context::resolve_plans_directory(tmp.path(), None, None);
    coco_context::write_plan(session_id, &plans_dir, "# plan", None).unwrap();

    let app_state = plan_mode_app_state(Some(PermissionMode::AcceptEdits), None);
    let mut ctx = ctx_with_mode(PermissionMode::Plan);
    ctx.config_home = Some(tmp.path().to_path_buf());
    ctx.session_id_for_history = Some(session_id.into());
    ctx.app_state = Some(app_state.clone().into());

    let _ = execute_and_apply_patch(&ExitPlanModeTool, json!({}), &ctx, &app_state)
        .await
        .unwrap();
    let guard = app_state.read().await;
    assert_eq!(guard.permission_mode, Some(PermissionMode::AcceptEdits));
    assert_eq!(guard.pre_plan_mode, None);
}

#[tokio::test]
async fn exit_plan_mode_restores_default_when_no_stash() {
    use tempfile::tempdir;
    let tmp = tempdir().unwrap();
    let session_id = "exit-restores-default";
    let plans_dir = coco_context::resolve_plans_directory(tmp.path(), None, None);
    coco_context::write_plan(session_id, &plans_dir, "# plan", None).unwrap();

    let app_state = plan_mode_app_state(None, None);
    let mut ctx = ctx_with_mode(PermissionMode::Plan);
    ctx.config_home = Some(tmp.path().to_path_buf());
    ctx.session_id_for_history = Some(session_id.into());
    ctx.app_state = Some(app_state.clone().into());

    let _ = execute_and_apply_patch(&ExitPlanModeTool, json!({}), &ctx, &app_state)
        .await
        .unwrap();
    assert_eq!(
        app_state.read().await.permission_mode,
        Some(PermissionMode::Default)
    );
}

#[tokio::test]
async fn exit_plan_mode_restoring_to_auto_strips_dangerous_rules() {
    // TS: ExitPlanModeV2Tool.ts:380-394 strips dangerous rules when
    // restoring to Auto. Rust mirrors by snapshotting + stashing on
    // app_state.stripped_dangerous_rules.
    use coco_types::PermissionRule;
    use coco_types::PermissionRuleSource;
    use coco_types::PermissionRuleValue;
    use tempfile::tempdir;
    let tmp = tempdir().unwrap();
    let session_id = "exit-to-auto-strips";
    let plans_dir = coco_context::resolve_plans_directory(tmp.path(), None, None);
    coco_context::write_plan(session_id, &plans_dir, "# plan", None).unwrap();

    let app_state = plan_mode_app_state(Some(PermissionMode::Auto), None);
    let mut ctx = ctx_with_mode(PermissionMode::Plan);
    ctx.config_home = Some(tmp.path().to_path_buf());
    ctx.session_id_for_history = Some(session_id.into());
    // Seed a dangerous allow rule — `sudo` is classifier-bypassing.
    ctx.permission_context.allow_rules.insert(
        PermissionRuleSource::UserSettings,
        vec![PermissionRule {
            behavior: coco_types::PermissionBehavior::Allow,
            value: PermissionRuleValue {
                tool_pattern: "Bash".into(),
                rule_content: Some("sudo *".into()),
            },
            source: PermissionRuleSource::UserSettings,
        }],
    );
    ctx.app_state = Some(app_state.clone().into());

    let _ = execute_and_apply_patch(&ExitPlanModeTool, json!({}), &ctx, &app_state)
        .await
        .unwrap();
    let guard = app_state.read().await;
    assert_eq!(guard.permission_mode, Some(PermissionMode::Auto));
    assert!(
        guard.stripped_dangerous_rules.is_some(),
        "dangerous rules must be stashed on Plan→Auto exit"
    );
}

#[tokio::test]
async fn exit_plan_mode_restoring_to_default_clears_stripped_rules() {
    // Inverse of the Auto case: if dangerous rules were stripped during
    // plan mode, exiting back to Default must clear the stash (so the
    // next ctx rebuild sees the un-stripped rules again).
    use coco_types::PermissionRule;
    use coco_types::PermissionRuleSource;
    use coco_types::PermissionRuleValue;
    use tempfile::tempdir;
    let tmp = tempdir().unwrap();
    let session_id = "exit-to-default-clears";
    let plans_dir = coco_context::resolve_plans_directory(tmp.path(), None, None);
    coco_context::write_plan(session_id, &plans_dir, "# plan", None).unwrap();

    let mut stashed = coco_types::PermissionRulesBySource::new();
    stashed.insert(
        PermissionRuleSource::UserSettings,
        vec![PermissionRule {
            behavior: coco_types::PermissionBehavior::Allow,
            value: PermissionRuleValue {
                tool_pattern: "Bash".into(),
                rule_content: Some("sudo *".into()),
            },
            source: PermissionRuleSource::UserSettings,
        }],
    );
    let app_state = plan_mode_app_state(Some(PermissionMode::Default), Some(stashed));
    let mut ctx = ctx_with_mode(PermissionMode::Plan);
    ctx.config_home = Some(tmp.path().to_path_buf());
    ctx.session_id_for_history = Some(session_id.into());
    ctx.app_state = Some(app_state.clone().into());

    let _ = execute_and_apply_patch(&ExitPlanModeTool, json!({}), &ctx, &app_state)
        .await
        .unwrap();
    let guard = app_state.read().await;
    assert_eq!(guard.permission_mode, Some(PermissionMode::Default));
    assert!(
        guard.stripped_dangerous_rules.is_none(),
        "stripped rules must be cleared on non-Auto exit"
    );
}

#[tokio::test]
async fn exit_plan_mode_execute_sets_exit_flags_on_app_state() {
    // ExitPlanModeTool.execute must set both `has_exited_plan_mode` and
    // `needs_plan_mode_exit_attachment` so the reminder orchestrator
    // fires Reentry (on next entry) + the exit banner (on the same turn).
    use std::sync::Arc;
    use tempfile::tempdir;
    use tokio::sync::RwLock;

    let tmp = tempdir().unwrap();
    let config_home = tmp.path().to_path_buf();
    let session_id = "test-exit-flags";
    let plans_dir = coco_context::resolve_plans_directory(&config_home, None, None);
    coco_context::write_plan(session_id, &plans_dir, "# plan", None).unwrap();

    let app_state = Arc::new(RwLock::new(ToolAppState::default()));
    let mut ctx = ctx_with_mode(PermissionMode::Plan);
    ctx.config_home = Some(config_home);
    ctx.session_id_for_history = Some(session_id.to_string());
    ctx.app_state = Some(app_state.clone().into());

    let _ = execute_and_apply_patch(&ExitPlanModeTool, json!({}), &ctx, &app_state)
        .await
        .unwrap();

    let guard = app_state.read().await;
    assert!(guard.has_exited_plan_mode);
    assert!(guard.needs_plan_mode_exit_attachment);
    // Default→Plan→Default cycle: auto wasn't active, no auto-exit banner.
    assert!(!guard.needs_auto_mode_exit_attachment);
}

#[tokio::test]
async fn exit_plan_mode_from_auto_with_no_restore_target_fires_auto_exit_flag() {
    // TS parity: when auto was effectively active during the plan
    // (dangerous rules stripped) but we aren't restoring to Auto, set
    // `needs_auto_mode_exit_attachment`. This covers the
    // `autoWasUsedDuringPlan && !finalRestoringAuto` branch in
    // ExitPlanModeV2Tool.ts:370-378. Here we simulate it by starting
    // with pre_plan_mode=None (so restore→Default) AND pre-stripped
    // dangerous rules (as if Auto was active during plan).
    use std::sync::Arc;
    use tempfile::tempdir;
    use tokio::sync::RwLock;

    let tmp = tempdir().unwrap();
    let config_home = tmp.path().to_path_buf();
    let session_id = "test-auto-exit-from-plan";
    let plans_dir = coco_context::resolve_plans_directory(&config_home, None, None);
    coco_context::write_plan(session_id, &plans_dir, "# plan", None).unwrap();

    // Simulate "Auto was active during plan" on app_state — the
    // shared store is the source of truth (TS parity:
    // `appState.toolPermissionContext.strippedDangerousRules`).
    let app_state = Arc::new(RwLock::new(ToolAppState {
        permission_mode: Some(PermissionMode::Plan),
        stripped_dangerous_rules: Some(coco_types::PermissionRulesBySource::default()),
        ..Default::default()
    }));
    let mut ctx = ctx_with_mode(PermissionMode::Plan);
    ctx.config_home = Some(config_home);
    ctx.session_id_for_history = Some(session_id.to_string());
    ctx.app_state = Some(app_state.clone().into());

    let _ = execute_and_apply_patch(&ExitPlanModeTool, json!({}), &ctx, &app_state)
        .await
        .unwrap();

    let guard = app_state.read().await;
    assert!(
        guard.needs_auto_mode_exit_attachment,
        "auto-mode-exit flag must be set when auto was active during plan \
         and restore is not Auto"
    );
}

#[tokio::test]
async fn enter_plan_mode_execute_records_entry_timestamp() {
    // EnterPlanModeTool.execute must write `plan_mode_entry_ms` so the
    // ExitPlanMode stale-plan advisory can compare mtime on exit.
    use std::sync::Arc;
    use tokio::sync::RwLock;

    let app_state = Arc::new(RwLock::new(ToolAppState::default()));
    let mut ctx = ctx_with_mode(PermissionMode::Default);
    ctx.app_state = Some(app_state.clone().into());

    let before = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    let _ = execute_and_apply_patch(&EnterPlanModeTool, json!({}), &ctx, &app_state)
        .await
        .unwrap();
    let after = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;

    let guard = app_state.read().await;
    let entry = guard.plan_mode_entry_ms.expect("entry_ms was not set");
    assert!(entry >= before && entry <= after, "entry_ms out of bounds");
    // Re-entry should clear any stale exit-attachment flag.
    assert!(!guard.needs_plan_mode_exit_attachment);
}

#[tokio::test]
async fn exit_plan_mode_reads_plan_from_disk() {
    use std::sync::Arc;
    use tempfile::tempdir;
    use tokio::sync::RwLock;

    // Write a plan to disk, then verify ExitPlanMode.execute() reads it.
    let tmp = tempdir().unwrap();
    let config_home = tmp.path().to_path_buf();
    let session_id = "test-session-read-disk";
    let plans_dir = coco_context::resolve_plans_directory(&config_home, None, None);
    coco_context::write_plan(session_id, &plans_dir, "## Plan\n- step 1", None).unwrap();

    let mut ctx = ctx_with_mode(PermissionMode::Plan);
    ctx.config_home = Some(config_home);
    ctx.session_id_for_history = Some(session_id.to_string());
    ctx.app_state = Some(Arc::new(RwLock::new(ToolAppState::default())).into());

    let result = ExitPlanModeTool.execute(json!({}), &ctx).await.unwrap();
    let plan = result
        .data
        .get("plan")
        .and_then(Value::as_str)
        .unwrap_or_default();
    assert!(
        plan.contains("step 1"),
        "plan should be read from disk: {plan}"
    );

    let file_path = result
        .data
        .get("filePath")
        .and_then(Value::as_str)
        .unwrap_or_default();
    assert!(
        file_path.ends_with(".md"),
        "filePath should be set: {file_path}"
    );
}

#[tokio::test]
async fn exit_plan_mode_input_plan_wins_over_disk() {
    use std::sync::Arc;
    use tempfile::tempdir;
    use tokio::sync::RwLock;

    let tmp = tempdir().unwrap();
    let config_home = tmp.path().to_path_buf();
    let session_id = "test-session-input-wins";
    let plans_dir = coco_context::resolve_plans_directory(&config_home, None, None);
    coco_context::write_plan(session_id, &plans_dir, "on-disk plan", None).unwrap();

    let mut ctx = ctx_with_mode(PermissionMode::Plan);
    ctx.config_home = Some(config_home);
    ctx.session_id_for_history = Some(session_id.to_string());
    ctx.app_state = Some(Arc::new(RwLock::new(ToolAppState::default())).into());

    let result = ExitPlanModeTool
        .execute(json!({"plan": "edited plan from CCR"}), &ctx)
        .await
        .unwrap();
    let plan = result
        .data
        .get("plan")
        .and_then(Value::as_str)
        .unwrap_or_default();
    assert_eq!(plan, "edited plan from CCR");
    assert_eq!(
        result.data.get("planWasEdited").and_then(Value::as_bool),
        Some(true)
    );
}

#[tokio::test]
async fn exit_plan_mode_injected_disk_plan_not_marked_as_user_edit() {
    use std::sync::Arc;
    use tempfile::tempdir;
    use tokio::sync::RwLock;

    let tmp = tempdir().unwrap();
    let config_home = tmp.path().to_path_buf();
    let session_id = "test-session-injected-plan";
    let plans_dir = coco_context::resolve_plans_directory(&config_home, None, None);
    coco_context::write_plan(session_id, &plans_dir, "on-disk plan", None).unwrap();

    let mut ctx = ctx_with_mode(PermissionMode::Plan);
    ctx.config_home = Some(config_home);
    ctx.session_id_for_history = Some(session_id.to_string());
    ctx.app_state = Some(Arc::new(RwLock::new(ToolAppState::default())).into());

    let result = ExitPlanModeTool
        .execute(json!({"plan": "on-disk plan"}), &ctx)
        .await
        .unwrap();

    assert_eq!(result.data.get("planWasEdited"), None);
}

#[test]
fn exit_plan_mode_schema_exposes_allowed_prompts() {
    let schema = ExitPlanModeTool.input_schema();
    assert!(schema.properties.contains_key("allowedPrompts"));
}

#[test]
fn exit_plan_mode_name_matches_registry() {
    assert_eq!(ExitPlanModeTool.name(), ToolName::ExitPlanMode.as_str());
}

#[test]
fn enter_plan_mode_name_matches_registry() {
    assert_eq!(EnterPlanModeTool.name(), ToolName::EnterPlanMode.as_str());
}

// ── Teammate approval flow ──

use std::sync::Arc;
use tokio::sync::Mutex as TokioMutex;

/// Capture mailbox writes for assertion without hitting disk.
#[derive(Default)]
struct CapturingMailbox {
    captured: TokioMutex<Vec<(String, String, coco_tool_runtime::MailboxEnvelope)>>,
}

#[async_trait::async_trait]
impl coco_tool_runtime::MailboxHandle for CapturingMailbox {
    async fn write_to_mailbox(
        &self,
        recipient: &str,
        team_name: &str,
        message: coco_tool_runtime::MailboxEnvelope,
    ) -> Result<(), coco_error::BoxedError> {
        self.captured
            .lock()
            .await
            .push((recipient.into(), team_name.into(), message));
        Ok(())
    }
    async fn read_unread(
        &self,
        _agent: &str,
        _team: &str,
    ) -> Result<Vec<coco_tool_runtime::InboxMessage>, coco_error::BoxedError> {
        Ok(Vec::new())
    }
    async fn mark_read(
        &self,
        _agent: &str,
        _team: &str,
        _index: usize,
    ) -> Result<(), coco_error::BoxedError> {
        Ok(())
    }
}

#[tokio::test]
async fn teammate_exit_plan_writes_approval_request_to_team_lead() {
    use tempfile::tempdir;
    let tmp = tempdir().unwrap();
    let config_home = tmp.path().to_path_buf();
    let session_id = "test-teammate-exit";
    let plans_dir = coco_context::resolve_plans_directory(&config_home, None, None);
    // Teammate has agent_id=alice → plan file is `{slug}-agent-alice.md`.
    coco_context::write_plan(session_id, &plans_dir, "# plan body", Some("alice")).unwrap();

    let capture = Arc::new(CapturingMailbox::default());

    let mut ctx = ctx_with_mode(PermissionMode::Plan);
    ctx.is_teammate = true;
    // TS parity: `isPlanModeRequired()` must be true for the mailbox
    // path to fire. Voluntary teammates (required=false) fall through
    // to normal exit.
    ctx.plan_mode_required = true;
    ctx.config_home = Some(config_home);
    ctx.session_id_for_history = Some(session_id.to_string());
    ctx.app_state = Some(Arc::new(tokio::sync::RwLock::new(ToolAppState::default())).into());
    ctx.mailbox = capture.clone();
    // The tool falls back to `ctx.agent_id` when env vars aren't set,
    // so we can control identity without mutating the global env
    // (`env::set_var` is unsafe in newer Rust + banned by CLAUDE.md).
    ctx.agent_id = Some(coco_types::AgentId::new("alice"));

    let result = ExitPlanModeTool.execute(json!({}), &ctx).await.unwrap();

    // Result shape signals "awaiting leader approval".
    assert_eq!(
        result
            .data
            .get("awaitingLeaderApproval")
            .and_then(Value::as_bool),
        Some(true)
    );
    assert!(
        result
            .data
            .get("requestId")
            .and_then(Value::as_str)
            .is_some()
    );

    // One mailbox write to team-lead under the fallback team name.
    let captured = capture.captured.lock().await;
    assert_eq!(captured.len(), 1, "exactly one plan_approval_request");
    let (recipient, team, env) = &captured[0];
    assert_eq!(recipient, "team-lead");
    // `COCO_TEAM_NAME` unset in-test (ctx.team_name=None) → fallback "default".
    assert_eq!(team, "default");
    // Body is a JSON-serialized PlanApprovalRequest.
    let parsed: serde_json::Value = serde_json::from_str(&env.text).unwrap();
    assert_eq!(parsed["type"], "plan_approval_request");
    assert_eq!(parsed["from"], "alice");
    assert!(
        parsed["planContent"]
            .as_str()
            .unwrap()
            .contains("plan body")
    );
    assert!(
        parsed["requestId"]
            .as_str()
            .unwrap()
            .starts_with("plan_approval-alice-default-")
    );

    // Teammate exit does NOT flip live mode — teammate stays in Plan
    // until the leader responds. ExitPlanModeTool::execute returns
    // early on the teammate branch (ExitPlanModeV2Tool.ts:264-313),
    // leaving app_state.permission_mode untouched.
    drop(captured);
    // No post-processing needed — the execute above already captured
    // the final state. Verify app_state wasn't mode-flipped (we
    // didn't set it, so it's whatever Default is — the point is that
    // no Plan→Default write happened on the teammate branch).
}

#[tokio::test]
async fn teammate_exit_plan_with_empty_plan_errors() {
    use tempfile::tempdir;
    let tmp = tempdir().unwrap();
    let config_home = tmp.path().to_path_buf();
    let session_id = "test-teammate-empty";
    // No plan written.

    let mut ctx = ctx_with_mode(PermissionMode::Plan);
    ctx.is_teammate = true;
    ctx.plan_mode_required = true;
    ctx.config_home = Some(config_home);
    ctx.session_id_for_history = Some(session_id.to_string());
    ctx.mailbox = Arc::new(CapturingMailbox::default());

    let result = ExitPlanModeTool.execute(json!({}), &ctx).await;
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("No plan file found")
    );
}

#[tokio::test]
async fn voluntary_teammate_exits_locally_without_mailbox_write() {
    // TS parity: `isTeammate() && !isPlanModeRequired()` →
    // ExitPlanMode exits locally (like a non-swarm session); no
    // plan_approval_request is sent to the leader.
    use tempfile::tempdir;
    let tmp = tempdir().unwrap();
    let config_home = tmp.path().to_path_buf();
    let session_id = "voluntary-teammate";
    let plans_dir = coco_context::resolve_plans_directory(&config_home, None, None);
    coco_context::write_plan(session_id, &plans_dir, "# vol plan", None).unwrap();

    let capture = Arc::new(CapturingMailbox::default());
    let mut ctx = ctx_with_mode(PermissionMode::Plan);
    ctx.is_teammate = true;
    ctx.plan_mode_required = false; // voluntary
    ctx.config_home = Some(config_home);
    ctx.session_id_for_history = Some(session_id.to_string());
    ctx.app_state = Some(Arc::new(tokio::sync::RwLock::new(ToolAppState::default())).into());
    ctx.mailbox = capture.clone();

    let result = ExitPlanModeTool.execute(json!({}), &ctx).await.unwrap();

    // No awaiting flag — normal exit semantics.
    assert_eq!(
        result
            .data
            .get("awaitingLeaderApproval")
            .and_then(Value::as_bool),
        None
    );
    // No mailbox write.
    assert!(
        capture.captured.lock().await.is_empty(),
        "voluntary teammate must NOT write to mailbox"
    );
}

#[tokio::test]
async fn verify_execution_disabled_by_default_skips_verification() {
    use tempfile::tempdir;
    let tmp = tempdir().unwrap();
    let config_home = tmp.path().to_path_buf();
    let session_id = "test-verify-off";
    let plans_dir = coco_context::resolve_plans_directory(&config_home, None, None);
    coco_context::write_plan(session_id, &plans_dir, "# plan", None).unwrap();

    let mut ctx = ctx_with_mode(PermissionMode::Plan);
    ctx.config_home = Some(config_home);
    ctx.session_id_for_history = Some(session_id.to_string());
    // Fake an entry timestamp AFTER the plan's mtime so verification
    // *would* flag "not edited" if it ran — which it mustn't.
    ctx.app_state = Some(
        Arc::new(tokio::sync::RwLock::new(ToolAppState {
            plan_mode_entry_ms: Some(i64::MAX),
            ..Default::default()
        }))
        .into(),
    );
    ctx.plan_verify_execution = false;

    let result = ExitPlanModeTool.execute(json!({}), &ctx).await.unwrap();
    assert_eq!(
        result.data.get("planVerification").and_then(Value::as_str),
        None,
        "verification must not run when plan_verify_execution=false"
    );
}

#[tokio::test]
async fn verify_execution_enabled_flags_stale_plan() {
    use tempfile::tempdir;
    let tmp = tempdir().unwrap();
    let config_home = tmp.path().to_path_buf();
    let session_id = "test-verify-on";
    let plans_dir = coco_context::resolve_plans_directory(&config_home, None, None);
    coco_context::write_plan(session_id, &plans_dir, "# plan", None).unwrap();

    let mut ctx = ctx_with_mode(PermissionMode::Plan);
    ctx.config_home = Some(config_home);
    ctx.session_id_for_history = Some(session_id.to_string());
    ctx.app_state = Some(
        Arc::new(tokio::sync::RwLock::new(ToolAppState {
            plan_mode_entry_ms: Some(i64::MAX),
            ..Default::default()
        }))
        .into(),
    );
    ctx.plan_verify_execution = true;

    let result = ExitPlanModeTool.execute(json!({}), &ctx).await.unwrap();
    assert_eq!(
        result.data.get("planVerification").and_then(Value::as_str),
        Some("not_edited"),
        "verification runs when enabled and flags stale plan file"
    );
}

#[test]
fn build_instructions_awaiting_leader_approval_variant() {
    let out = ExitPlanModeTool::build_instructions(&json!({
        "awaitingLeaderApproval": true,
        "requestId": "plan_approval-alice-team-a-deadbeef",
        "filePath": "/tmp/plan.md",
    }));
    assert!(out.contains("submitted to the team lead"));
    assert!(out.contains("plan_approval-alice-team-a-deadbeef"));
    assert!(out.contains("/tmp/plan.md"));
    assert!(out.contains("Do NOT proceed"));
}

// ── build_instructions ──

#[test]
fn build_instructions_agent_variant() {
    let out = ExitPlanModeTool::build_instructions(&json!({"isAgent": true, "plan": "x"}));
    assert!(out.contains("respond with"));
}

#[test]
fn build_instructions_empty_plan() {
    let out = ExitPlanModeTool::build_instructions(&json!({"plan": "   "}));
    assert!(out.contains("You can now proceed"));
}

#[test]
fn build_instructions_with_plan_and_edited_flag() {
    let out = ExitPlanModeTool::build_instructions(&json!({
        "plan": "step 1",
        "filePath": "/tmp/plan.md",
        "planWasEdited": true,
    }));
    assert!(out.contains("(edited by user)"));
    assert!(out.contains("/tmp/plan.md"));
    assert!(out.contains("step 1"));
}

// ── Prompt + post-execute parity tests (G5.1) ──
//
// Byte-precise comparisons against the TS reference at
// `tools/EnterPlanModeTool/prompt.ts` and `tools/ExitPlanModeTool/prompt.ts`.
// Any drift (a missing newline, a moved bullet, a renamed tool reference)
// will fail this test rather than silently change what the model sees.

use coco_tool_runtime::PromptOptions;
use pretty_assertions::assert_eq as ts_assert_eq;

/// External-arm `EnterPlanMode` prompt with `whatHappens=WHAT_HAPPENS_SECTION`.
/// TS source: `tools/EnterPlanModeTool/prompt.ts:23-98` with
/// `${ASK_USER_QUESTION_TOOL_NAME}` substituted to `AskUserQuestion`.
const TS_ENTER_PLAN_MODE_PROMPT_FIVE_PHASE: &str =
"Use this tool proactively when you're about to start a non-trivial implementation task. Getting user sign-off on your approach before writing code prevents wasted effort and ensures alignment. This tool transitions you into plan mode where you can explore the codebase and design an implementation approach for user approval.

## When to Use This Tool

**Prefer using EnterPlanMode** for implementation tasks unless they're simple. Use it when ANY of these conditions apply:

1. **New Feature Implementation**: Adding meaningful new functionality
   - Example: \"Add a logout button\" - where should it go? What should happen on click?
   - Example: \"Add form validation\" - what rules? What error messages?

2. **Multiple Valid Approaches**: The task can be solved in several different ways
   - Example: \"Add caching to the API\" - could use Redis, in-memory, file-based, etc.
   - Example: \"Improve performance\" - many optimization strategies possible

3. **Code Modifications**: Changes that affect existing behavior or structure
   - Example: \"Update the login flow\" - what exactly should change?
   - Example: \"Refactor this component\" - what's the target architecture?

4. **Architectural Decisions**: The task requires choosing between patterns or technologies
   - Example: \"Add real-time updates\" - WebSockets vs SSE vs polling
   - Example: \"Implement state management\" - Redux vs Context vs custom solution

5. **Multi-File Changes**: The task will likely touch more than 2-3 files
   - Example: \"Refactor the authentication system\"
   - Example: \"Add a new API endpoint with tests\"

6. **Unclear Requirements**: You need to explore before understanding the full scope
   - Example: \"Make the app faster\" - need to profile and identify bottlenecks
   - Example: \"Fix the bug in checkout\" - need to investigate root cause

7. **User Preferences Matter**: The implementation could reasonably go multiple ways
   - If you would use AskUserQuestion to clarify the approach, use EnterPlanMode instead
   - Plan mode lets you explore first, then present options with context

## When NOT to Use This Tool

Only skip EnterPlanMode for simple tasks:
- Single-line or few-line fixes (typos, obvious bugs, small tweaks)
- Adding a single function with clear requirements
- Tasks where the user has given very specific, detailed instructions
- Pure research/exploration tasks (use the Agent tool with explore agent instead)

## What Happens in Plan Mode

In plan mode, you'll:
1. Thoroughly explore the codebase using Glob, Grep, and Read tools
2. Understand existing patterns and architecture
3. Design an implementation approach
4. Present your plan to the user for approval
5. Use AskUserQuestion if you need to clarify approaches
6. Exit plan mode with ExitPlanMode when ready to implement

## Examples

### GOOD - Use EnterPlanMode:
User: \"Add user authentication to the app\"
- Requires architectural decisions (session vs JWT, where to store tokens, middleware structure)

User: \"Optimize the database queries\"
- Multiple approaches possible, need to profile first, significant impact

User: \"Implement dark mode\"
- Architectural decision on theme system, affects many components

User: \"Add a delete button to the user profile\"
- Seems simple but involves: where to place it, confirmation dialog, API call, error handling, state updates

User: \"Update the error handling in the API\"
- Affects multiple files, user should approve the approach

### BAD - Don't use EnterPlanMode:
User: \"Fix the typo in the README\"
- Straightforward, no planning needed

User: \"Add a console.log to debug this function\"
- Simple, obvious implementation

User: \"What files handle routing?\"
- Research task, not implementation planning

## Important Notes

- This tool REQUIRES user approval - they must consent to entering plan mode
- If unsure whether to use it, err on the side of planning - it's better to get alignment upfront than to redo work
- Users appreciate being consulted before significant changes are made to their codebase
";

#[tokio::test]
async fn enter_plan_mode_prompt_five_phase_matches_ts_byte_precise() {
    let opts = PromptOptions {
        is_plan_interview_phase: false,
        ..Default::default()
    };
    let actual = EnterPlanModeTool.prompt(&opts).await;
    ts_assert_eq!(actual, TS_ENTER_PLAN_MODE_PROMPT_FIVE_PHASE);
}

#[tokio::test]
async fn enter_plan_mode_prompt_interview_omits_what_happens() {
    // TS `tools/EnterPlanModeTool/prompt.ts:19-21`: when
    // `isPlanModeInterviewPhaseEnabled()`, `whatHappens` is `''`,
    // so the `## What Happens in Plan Mode` block disappears. The
    // surrounding structure (Examples, Important Notes) stays.
    let opts = PromptOptions {
        is_plan_interview_phase: true,
        ..Default::default()
    };
    let actual = EnterPlanModeTool.prompt(&opts).await;
    assert!(
        !actual.contains("## What Happens in Plan Mode"),
        "interview-phase prompt must omit 'What Happens' section"
    );
    assert!(
        !actual.contains("Exit plan mode with ExitPlanMode when ready to implement"),
        "interview-phase prompt must omit the 6-step list inside 'What Happens'"
    );
    // Surrounding structure still present.
    assert!(actual.contains("## Examples"));
    assert!(actual.contains("## Important Notes"));
    assert!(actual.contains("- This tool REQUIRES user approval"));
    // The condition #7 mention of AskUserQuestion is in the upper
    // section (NOT inside WHAT_HAPPENS_SECTION) so it stays.
    assert!(actual.contains("If you would use AskUserQuestion to clarify"));
}

/// TS source: `tools/ExitPlanModeTool/prompt.ts` with
/// `${ASK_USER_QUESTION_TOOL_NAME}` substituted to `AskUserQuestion`.
const TS_EXIT_PLAN_MODE_PROMPT: &str =
"Use this tool when you are in plan mode and have finished writing your plan to the plan file and are ready for user approval.

## How This Tool Works
- You should have already written your plan to the plan file specified in the plan mode system message
- This tool does NOT take the plan content as a parameter - it will read the plan from the file you wrote
- This tool simply signals that you're done planning and ready for the user to review and approve
- The user will see the contents of your plan file when they review it

## When to Use This Tool
IMPORTANT: Only use this tool when the task requires planning the implementation steps of a task that requires writing code. For research tasks where you're gathering information, searching files, reading files or in general trying to understand the codebase - do NOT use this tool.

## Before Using This Tool
Ensure your plan is complete and unambiguous:
- If you have unresolved questions about requirements or approach, use AskUserQuestion first (in earlier phases)
- Once your plan is finalized, use THIS tool to request approval

**Important:** Do NOT use AskUserQuestion to ask \"Is this plan okay?\" or \"Should I proceed?\" - that's exactly what THIS tool does. ExitPlanMode inherently requests user approval of your plan.
";

#[tokio::test]
async fn exit_plan_mode_prompt_matches_ts_byte_precise() {
    let opts = PromptOptions::default();
    let actual = ExitPlanModeTool.prompt(&opts).await;
    ts_assert_eq!(actual, TS_EXIT_PLAN_MODE_PROMPT);
}

#[test]
fn enter_plan_mode_build_instructions_five_phase_matches_ts() {
    // TS `EnterPlanModeTool.ts:108-118` — the non-interview branch.
    let confirmation = "Hello.";
    let expected = "Hello.\n\nIn plan mode, you should:\n\
                    1. Thoroughly explore the codebase to understand existing patterns\n\
                    2. Identify similar features and architectural approaches\n\
                    3. Consider multiple approaches and their trade-offs\n\
                    4. Use AskUserQuestion if you need to clarify the approach\n\
                    5. Design a concrete implementation strategy\n\
                    6. When ready, use ExitPlanMode to present your plan for approval\n\n\
                    Remember: DO NOT write or edit any files yet. \
                    This is a read-only exploration and planning phase.";
    let actual = EnterPlanModeTool::build_instructions(confirmation, false);
    ts_assert_eq!(actual, expected);
}

#[test]
fn enter_plan_mode_build_instructions_interview_matches_ts() {
    // TS `EnterPlanModeTool.ts:104-107` — the interview branch.
    let confirmation = "Hello.";
    let expected = "Hello.\n\nDO NOT write or edit any files except the plan file. \
                    Detailed workflow instructions will follow.";
    let actual = EnterPlanModeTool::build_instructions(confirmation, true);
    ts_assert_eq!(actual, expected);
}

#[tokio::test]
async fn enter_plan_mode_execute_data_carries_short_confirmation_and_flag() {
    // Post `Tool::render_for_model` migration: `execute` writes ONLY
    // the short confirmation + the `isInterviewPhase` flag into
    // `data`. The full workflow splice now lives in `render_for_model`
    // (covered by `enter_plan_mode_render_for_model_*` below). This
    // matches TS `EnterPlanModeTool.ts::call` shape exactly.
    let mut ctx = ctx_with_mode(PermissionMode::Default);
    ctx.is_plan_interview_phase = false;
    let result = EnterPlanModeTool.execute(json!({}), &ctx).await.unwrap();
    let msg = result
        .data
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or_default();
    assert_eq!(
        msg,
        "Entered plan mode. You should now focus on exploring the codebase and designing an implementation approach."
    );
    assert_eq!(
        result.data.get("isInterviewPhase").and_then(Value::as_bool),
        Some(false)
    );

    let mut ctx = ctx_with_mode(PermissionMode::Default);
    ctx.is_plan_interview_phase = true;
    let result = EnterPlanModeTool.execute(json!({}), &ctx).await.unwrap();
    assert_eq!(
        result.data.get("isInterviewPhase").and_then(Value::as_bool),
        Some(true)
    );
}

#[test]
fn enter_plan_mode_render_for_model_five_phase_branch() {
    // Renderer pulls the workflow flag out of `data` (written by
    // `execute`) and produces a single Text part with the full TS
    // 6-step splice.
    let data = json!({
        "message": "Entered plan mode. You should now focus on exploring the codebase and designing an implementation approach.",
        "isInterviewPhase": false,
    });
    let parts = EnterPlanModeTool.render_for_model(&data);
    let [coco_tool_runtime::ToolResultContentPart::Text { text, .. }] = parts.as_slice() else {
        panic!("expected single Text part, got {parts:?}");
    };
    assert!(text.starts_with("Entered plan mode."));
    assert!(text.contains("In plan mode, you should:"));
    assert!(text.contains("6. When ready, use ExitPlanMode"));
}

#[test]
fn enter_plan_mode_render_for_model_interview_branch() {
    let data = json!({
        "message": "Entered plan mode. You should now focus on exploring the codebase and designing an implementation approach.",
        "isInterviewPhase": true,
    });
    let parts = EnterPlanModeTool.render_for_model(&data);
    let [coco_tool_runtime::ToolResultContentPart::Text { text, .. }] = parts.as_slice() else {
        panic!("expected single Text part, got {parts:?}");
    };
    assert!(text.starts_with("Entered plan mode."));
    assert!(text.contains("DO NOT write or edit any files except the plan file"));
    assert!(!text.contains("In plan mode, you should:"));
}

#[test]
fn exit_plan_mode_render_for_model_routes_through_build_instructions() {
    // ExitPlanMode `render_for_model` must thread `data` directly
    // through `build_instructions` — the executor's `tool_outcome_builder`
    // calls this method, no longer JSON-stringifies.
    let data = json!({
        "plan": "step 1",
        "filePath": "/tmp/plan.md",
        "planWasEdited": true,
    });
    let parts = ExitPlanModeTool.render_for_model(&data);
    let [coco_tool_runtime::ToolResultContentPart::Text { text, .. }] = parts.as_slice() else {
        panic!("expected single Text part, got {parts:?}");
    };
    assert!(text.contains("(edited by user)"));
    assert!(text.contains("/tmp/plan.md"));
    assert!(text.contains("step 1"));
}
