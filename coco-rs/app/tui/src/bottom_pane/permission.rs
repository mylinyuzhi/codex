//! Permission-family prompt behavior: tool permission, sandbox permission,
//! and MCP-server approval (the three `ApprovalResponse`-carrying prompts).
//!
//! Owns the always-allow rule construction (disk-persisted `LocalSettings`
//! allow rules, read-path directory widening) and the multi-choice payload
//! splice.

use std::str::FromStr;

use tokio::sync::mpsc;

use crate::command::UserCommand;
use crate::state::AppState;
use crate::state::PanePromptState;
use crate::state::surface_payloads::PermissionAction;

/// Approve ('y' / approve choice) on a tool-permission prompt.
///
/// Multi-choice mode: commits the currently-focused choice (Enter takes the
/// same path via `confirm`). The chosen `value` is spliced into
/// `updated_input` so the tool's `execute()` can branch on it; a choice whose
/// value is "no" denies. Classic yes/no mode keeps the unconditional
/// `approved: true` path.
pub(crate) async fn approve_permission(
    p: &crate::state::PermissionPromptState,
    command_tx: &mpsc::Sender<UserCommand>,
) {
    let (approved, updated_input) = if p.choices.is_some() {
        let chosen_is_no = p
            .choices
            .as_ref()
            .and_then(|cs| cs.get(p.selected_choice))
            .map(|c| c.value.as_str())
            == Some("no");
        (!chosen_is_no, build_choice_payload(p))
    } else {
        (true, None)
    };
    tracing::info!(
        target: "coco_tui::permission",
        request_id = %p.request_id,
        tool_name = %p.tool_name,
        permission_decision = if approved { "approve" } else { "deny" },
        always_allow = false,
        multi_choice = p.choices.is_some(),
        "user permission decision",
    );
    if let Err(e) = command_tx
        .send(UserCommand::ApprovalResponse {
            request_id: p.request_id.clone(),
            approved,
            always_allow: false,
            feedback: None,
            updated_input,
            permission_updates: vec![],
            content_blocks: None,
        })
        .await
    {
        tracing::warn!(
            target: "coco_tui::permission",
            error = %e,
            "failed to dispatch ApprovalResponse (channel closed)",
        );
    }
}

/// Approve/deny a sandbox-permission prompt.
pub(crate) async fn respond_sandbox(
    s: &crate::state::SandboxPermissionPromptState,
    approved: bool,
    command_tx: &mpsc::Sender<UserCommand>,
) {
    tracing::info!(
        target: "coco_tui::permission",
        request_id = %s.request_id,
        kind = "sandbox",
        permission_decision = if approved { "approve" } else { "deny" },
        "user sandbox permission decision",
    );
    let _ = command_tx
        .send(UserCommand::ApprovalResponse {
            request_id: s.request_id.clone(),
            approved,
            always_allow: false,
            feedback: None,
            updated_input: None,
            permission_updates: vec![],
            content_blocks: None,
        })
        .await;
}

/// Approve/deny an MCP-server approval prompt.
pub(crate) async fn respond_mcp_server(
    m: &crate::state::McpServerApprovalPromptState,
    approved: bool,
    command_tx: &mpsc::Sender<UserCommand>,
) {
    tracing::info!(
        target: "coco_tui::permission",
        request_id = %m.request_id,
        kind = "mcp_server",
        permission_decision = if approved { "approve" } else { "deny" },
        "user MCP server approval decision",
    );
    let _ = command_tx
        .send(UserCommand::ApprovalResponse {
            request_id: m.request_id.clone(),
            approved,
            always_allow: false,
            feedback: None,
            updated_input: None,
            permission_updates: vec![],
            content_blocks: None,
        })
        .await;
}

/// Deny ('n') a tool-permission prompt.
pub(crate) async fn deny_permission(
    p: &crate::state::PermissionPromptState,
    command_tx: &mpsc::Sender<UserCommand>,
) {
    tracing::info!(
        target: "coco_tui::permission",
        request_id = %p.request_id,
        tool_name = %p.tool_name,
        permission_decision = "deny",
        always_allow = false,
        "user permission decision",
    );
    let _ = command_tx
        .send(UserCommand::ApprovalResponse {
            request_id: p.request_id.clone(),
            approved: false,
            always_allow: false,
            feedback: None,
            updated_input: None,
            permission_updates: vec![],
            content_blocks: None,
        })
        .await;
}

/// Handle `ApproveAll` (always-allow) for permission prompts.
///
/// Builds a `LocalSettings`-scoped allow rule for the tool (mirrors TS
/// `FallbackPermissionRequest` `destination:'localSettings'`). `tui_runner`
/// both applies the update to the live `engine_config` via
/// `coco_permissions::apply_permission_updates` (so subsequent same-tool
/// calls in the session don't re-prompt) and persists it to
/// `.coco/settings.local.json` via `SettingsPermissionStore::persist_update`
/// (so the grant survives restart). `LocalSettings` is the gitignored,
/// per-developer file — a reflexive "don't ask again" must never silently
/// edit team-shared (`ProjectSettings`) or global (`UserSettings`) config.
///
/// Picking `Project` / `User` destinations lives in the dedicated
/// `/permissions` rule-editor overlay (TS `AddPermissionRules`), not this
/// inline popup.
pub(crate) async fn approve_all(state: &mut AppState, command_tx: &mpsc::Sender<UserCommand>) {
    if let Some(PanePromptState::Permission(p)) = state.ui.interaction.active_prompt.as_ref()
        && p.show_always_allow
    {
        let updates = always_allow_updates(
            &p.tool_name,
            p.original_input.as_ref(),
            &p.permission_suggestions,
        );
        tracing::info!(
            target: "coco_tui::permission",
            request_id = %p.request_id,
            tool_name = %p.tool_name,
            permission_decision = "approve",
            always_allow = true,
            rules = updates.len(),
            "user always-allow decision",
        );
        let _ = command_tx
            .send(UserCommand::ApprovalResponse {
                request_id: p.request_id.clone(),
                approved: true,
                always_allow: true,
                feedback: None,
                updated_input: None,
                permission_updates: updates,
                content_blocks: None,
            })
            .await;
        state.ui.dismiss_prompt();
    }
}

/// Handle `ClassifierAutoApprove` — background classifier approved the pending
/// request before the user responded.
pub(crate) async fn classifier_auto_approve(
    state: &mut AppState,
    command_tx: &mpsc::Sender<UserCommand>,
    request_id: String,
) {
    if let Some(PanePromptState::Permission(p)) = state.ui.interaction.active_prompt.as_ref()
        && p.request_id == request_id
    {
        tracing::info!(
            target: "coco_tui::permission",
            request_id = %p.request_id,
            tool_name = %p.tool_name,
            permission_decision = "approve",
            source = "classifier",
            "classifier auto-approve",
        );
        let _ = command_tx
            .send(UserCommand::ApprovalResponse {
                request_id: p.request_id.clone(),
                approved: true,
                always_allow: false,
                feedback: None,
                updated_input: None,
                permission_updates: vec![],
                content_blocks: None,
            })
            .await;
        state.ui.dismiss_prompt();
    }
}

/// Confirm (Enter) on a tool-permission prompt: commit the focused choice
/// (multi-choice) or the focused classic action.
pub(crate) async fn confirm_permission(
    p: &crate::state::PermissionPromptState,
    command_tx: &mpsc::Sender<UserCommand>,
) {
    let (approved, always_allow, updated_input, permission_updates) = if p.choices.is_some() {
        let chosen_is_no = p
            .choices
            .as_ref()
            .and_then(|cs| cs.get(p.selected_choice))
            .map(|c| c.value.as_str())
            == Some("no");
        (!chosen_is_no, false, build_choice_payload(p), vec![])
    } else {
        match p.selected_classic_action() {
            PermissionAction::ApproveOnce => (true, false, None, vec![]),
            PermissionAction::AlwaysAllow => (
                true,
                true,
                None,
                always_allow_updates(
                    &p.tool_name,
                    p.original_input.as_ref(),
                    &p.permission_suggestions,
                ),
            ),
            PermissionAction::Deny => (false, false, None, vec![]),
        }
    };
    let _ = command_tx
        .send(UserCommand::ApprovalResponse {
            request_id: p.request_id.clone(),
            approved,
            always_allow,
            feedback: None,
            updated_input,
            permission_updates,
            content_blocks: None,
        })
        .await;
}

/// Move the choice cursor on a permission prompt (wrapping).
pub(crate) fn nav_permission(p: &mut crate::state::PermissionPromptState, delta: i32) {
    let count = p
        .choices
        .as_ref()
        .map(Vec::len)
        .unwrap_or_else(|| p.classic_action_count()) as i32;
    if count > 0 {
        let current = p.selected_choice as i32;
        let next = (current + delta).rem_euclid(count);
        p.selected_choice = next as usize;
    }
}

fn always_allow_updates(
    tool_name: &str,
    original_input: Option<&serde_json::Value>,
    permission_suggestions: &[coco_types::PermissionUpdate],
) -> Vec<coco_types::PermissionUpdate> {
    if !permission_suggestions.is_empty() {
        return permission_suggestions.to_vec();
    }
    if let Some(update) = read_path_allow_update(tool_name, original_input) {
        return vec![update];
    }
    vec![coco_types::PermissionUpdate::AddRules {
        rules: vec![coco_types::PermissionRule {
            source: coco_types::PermissionRuleSource::LocalSettings,
            behavior: coco_types::PermissionBehavior::Allow,
            value: coco_types::PermissionRuleValue {
                tool_pattern: tool_name.to_string(),
                rule_content: None,
            },
        }],
        destination: coco_types::PermissionUpdateDestination::LocalSettings,
    }]
}

fn read_path_allow_update(
    tool_name: &str,
    original_input: Option<&serde_json::Value>,
) -> Option<coco_types::PermissionUpdate> {
    let tool = coco_types::ToolName::from_str(tool_name).ok()?;
    if !matches!(
        tool,
        coco_types::ToolName::Read | coco_types::ToolName::Grep | coco_types::ToolName::Glob
    ) {
        return None;
    }
    let input = original_input?;
    let raw_path = match tool {
        coco_types::ToolName::Read => input.get("file_path").and_then(|v| v.as_str())?,
        coco_types::ToolName::Grep | coco_types::ToolName::Glob => {
            input.get("path").and_then(|v| v.as_str())?
        }
        _ => return None,
    };
    let dir = directory_for_permission_rule(raw_path)?;
    let rule_content = format!("{}/**", path_for_permission_rule(&dir));
    Some(coco_types::PermissionUpdate::AddRules {
        rules: vec![coco_types::PermissionRule {
            source: coco_types::PermissionRuleSource::LocalSettings,
            behavior: coco_types::PermissionBehavior::Allow,
            value: coco_types::PermissionRuleValue {
                tool_pattern: coco_types::ToolName::Read.as_str().to_string(),
                rule_content: Some(rule_content),
            },
        }],
        destination: coco_types::PermissionUpdateDestination::LocalSettings,
    })
}

fn directory_for_permission_rule(raw_path: &str) -> Option<std::path::PathBuf> {
    let path = shellexpand_read_path(raw_path);
    let absolute = if path.is_absolute() {
        path
    } else {
        std::env::current_dir().ok()?.join(path)
    };
    let dir = if absolute.is_dir() {
        absolute
    } else {
        absolute.parent()?.to_path_buf()
    };
    (dir.parent().is_some()).then_some(dir)
}

fn shellexpand_read_path(raw_path: &str) -> std::path::PathBuf {
    if raw_path == "~" {
        return dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from(raw_path));
    }
    if let Some(rest) = raw_path.strip_prefix("~/")
        && let Some(home) = dirs::home_dir()
    {
        return home.join(rest);
    }
    std::path::PathBuf::from(raw_path)
}

fn path_for_permission_rule(path: &std::path::Path) -> String {
    let path = path.to_string_lossy().replace('\\', "/");
    if path.starts_with('/') {
        format!("/{path}")
    } else {
        path
    }
}

/// Build the `{...original_input, user_choice}` payload shipped via
/// `UserCommand::ApprovalResponse.updated_input` when the user commits
/// a multi-choice permission selection. Carries every field the tool
/// originally supplied so its `execute()` can read both the new
/// `user_choice` field and the original args. Returns `None` when the
/// state has no choices or the cursor is out of range — caller falls
/// back to `updated_input: None`.
///
/// TS parity: `ExitPlanModePermissionRequest.tsx:691-704` — the
/// option's `value` is merged into the original tool input on commit.
pub(crate) fn build_choice_payload(
    p: &crate::state::PermissionPromptState,
) -> Option<serde_json::Value> {
    let choice = p.choices.as_ref()?.get(p.selected_choice)?;
    let mut payload = p
        .original_input
        .as_ref()
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default();
    payload.insert(
        "user_choice".into(),
        serde_json::Value::String(choice.value.clone()),
    );
    Some(serde_json::Value::Object(payload))
}
