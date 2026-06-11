//! Permission-family prompt behavior: tool permission, sandbox permission,
//! and MCP-server approval (the three `ApprovalResponse`-carrying prompts).
//!
//! Owns the always-allow rule construction (disk-persisted `LocalSettings`
//! allow rules, read-path directory widening) and the multi-choice payload
//! splice.

use std::str::FromStr;

use rust_i18n::t;
use tokio::sync::mpsc;

use crate::command::UserCommand;
use crate::state::AppState;
use crate::state::PanePromptState;
use crate::state::Toast;
use crate::state::surface_payloads::PermissionAction;

/// Single resolution chokepoint for classic (non-choice) tool-permission
/// prompts. Every classic decision — `y` / `n` / `a` hotkeys, Enter on the
/// focused row, digit shortcuts — funnels through here so the
/// `ApprovalResponse` construction and the structured decision log exist
/// exactly once.
pub(crate) async fn resolve_classic_permission(
    p: &crate::state::PermissionPromptState,
    action: PermissionAction,
    command_tx: &mpsc::Sender<UserCommand>,
) {
    let (approved, always_allow, permission_updates) = match action {
        PermissionAction::ApproveOnce => (true, false, vec![]),
        PermissionAction::AlwaysAllow => (
            true,
            true,
            always_allow_updates(
                &p.tool_name,
                p.original_input.as_ref(),
                &p.permission_suggestions,
            ),
        ),
        PermissionAction::Deny => (false, false, vec![]),
    };
    tracing::info!(
        target: "coco_tui::permission",
        request_id = %p.request_id,
        tool_name = %p.tool_name,
        permission_decision = if approved { "approve" } else { "deny" },
        always_allow,
        rules = permission_updates.len(),
        multi_choice = false,
        "user permission decision",
    );
    if let Err(e) = command_tx
        .send(UserCommand::ApprovalResponse {
            request_id: p.request_id.clone(),
            approved,
            always_allow,
            feedback: None,
            updated_input: None,
            permission_updates,
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

/// Approve ('y' / approve choice) on a tool-permission prompt.
///
/// Multi-choice mode: commits the currently-focused choice (Enter takes the
/// same path via `confirm`). The chosen `value` is spliced into
/// `updated_input` so the tool's `execute()` can branch on it; a choice whose
/// value is "no" denies. Classic mode commits a one-shot approve regardless
/// of the focused row — `y` is the ApproveOnce hotkey, not "confirm
/// selection" (that's Enter); the rendered rows carry their hotkeys so the
/// mapping is visible.
pub(crate) async fn approve_permission(
    p: &crate::state::PermissionPromptState,
    command_tx: &mpsc::Sender<UserCommand>,
) {
    let Some(choices) = &p.choices else {
        resolve_classic_permission(p, PermissionAction::ApproveOnce, command_tx).await;
        return;
    };
    let chosen_is_no = choices
        .get(p.selected_choice)
        .map(|c| c.value.as_str() == "no")
        .unwrap_or(false);
    let approved = !chosen_is_no;
    tracing::info!(
        target: "coco_tui::permission",
        request_id = %p.request_id,
        tool_name = %p.tool_name,
        permission_decision = if approved { "approve" } else { "deny" },
        always_allow = false,
        multi_choice = true,
        "user permission decision",
    );
    if let Err(e) = command_tx
        .send(UserCommand::ApprovalResponse {
            request_id: p.request_id.clone(),
            approved,
            always_allow: false,
            feedback: rejection_feedback(p, chosen_is_no),
            updated_input: build_choice_payload(p),
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

fn rejection_feedback(
    p: &crate::state::PermissionPromptState,
    chosen_is_no: bool,
) -> Option<String> {
    (chosen_is_no && p.tool_name == coco_types::ToolName::ExitPlanMode.as_str())
        .then(|| "User rejected the plan. Stay in plan mode and continue planning.".to_string())
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
    resolve_classic_permission(p, PermissionAction::Deny, command_tx).await;
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
    let Some(PanePromptState::Permission(p)) = state.ui.interaction.active_prompt.as_ref() else {
        return;
    };
    // Choice dialogs have no always-allow affordance ('a' is not a decision
    // key there); ignore silently like any other unmapped key.
    if p.choices.is_some() {
        return;
    }
    if !p.show_always_allow {
        // Gated off (managed settings allow only managed permission rules).
        // Never no-op silently: tell the user why their keypress did
        // nothing and leave the prompt open for an explicit y/n.
        tracing::info!(
            target: "coco_tui::permission",
            request_id = %p.request_id,
            tool_name = %p.tool_name,
            "always-allow requested but disabled by managed settings",
        );
        state
            .ui
            .add_toast(Toast::warning(t!("toast.always_allow_disabled")));
        return;
    }
    resolve_classic_permission(p, PermissionAction::AlwaysAllow, command_tx).await;
    state.ui.dismiss_prompt();
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
    if p.choices.is_some() {
        // Multi-choice commit shares `approve_permission`'s splice + log.
        approve_permission(p, command_tx).await;
        return;
    }
    resolve_classic_permission(p, p.selected_classic_action(), command_tx).await;
}

/// Digit shortcut (`1`-`3`) on a classic tool-permission prompt: commit the
/// numbered row directly. Returns `false` when the digit doesn't address a
/// row (multi-choice mode or out of range) — the caller keeps the prompt
/// open.
pub(crate) async fn commit_permission_digit(
    p: &crate::state::PermissionPromptState,
    digit: usize,
    command_tx: &mpsc::Sender<UserCommand>,
) -> bool {
    if p.choices.is_some() {
        return false;
    }
    let Some(index) = digit.checked_sub(1) else {
        return false;
    };
    if index >= p.classic_action_count() {
        return false;
    }
    resolve_classic_permission(p, p.classic_action_at(index), command_tx).await;
    true
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
    if let Some(update) = edit_path_allow_update(tool_name, original_input) {
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

/// Directory-scoped `Edit(dir/**)` allow rule for write-capable tools.
///
/// "Don't ask again" on a file-modifying tool must never grant a TOOL-WIDE
/// allow (which would silently approve writes anywhere on disk). When the
/// engine attached no scoped suggestions, derive the target directory from
/// the tool input instead: `file_path`/`notebook_path` fields for
/// Edit/Write/NotebookEdit, the `*** Add/Update/Delete File:` headers for
/// apply_patch. Returns `None` for non-write tools and for write tools whose
/// target paths can't be derived — apply_patch then falls back to tool-wide
/// like before, which is still gated behind an explicit user action.
fn edit_path_allow_update(
    tool_name: &str,
    original_input: Option<&serde_json::Value>,
) -> Option<coco_types::PermissionUpdate> {
    let tool = coco_types::ToolName::from_str(tool_name).ok()?;
    let input = original_input?;
    let paths: Vec<&str> = match tool {
        coco_types::ToolName::Edit | coco_types::ToolName::Write => input
            .get("file_path")
            .and_then(|v| v.as_str())
            .into_iter()
            .collect(),
        coco_types::ToolName::NotebookEdit => input
            .get("notebook_path")
            .and_then(|v| v.as_str())
            .into_iter()
            .collect(),
        coco_types::ToolName::ApplyPatch => {
            let patch = input.get("patch").and_then(|v| v.as_str())?;
            crate::tool_display::apply_patch_target_paths(patch)
        }
        _ => return None,
    };
    if paths.is_empty() {
        return None;
    }
    let mut rule_contents = std::collections::BTreeSet::new();
    for path in paths {
        let dir = directory_for_permission_rule(path)?;
        rule_contents.insert(format!("{}/**", path_for_permission_rule(&dir)));
    }
    let rules = rule_contents
        .into_iter()
        .map(|rule_content| coco_types::PermissionRule {
            source: coco_types::PermissionRuleSource::LocalSettings,
            behavior: coco_types::PermissionBehavior::Allow,
            value: coco_types::PermissionRuleValue {
                tool_pattern: coco_types::ToolName::Edit.as_str().to_string(),
                rule_content: Some(rule_content),
            },
        })
        .collect();
    Some(coco_types::PermissionUpdate::AddRules {
        rules,
        destination: coco_types::PermissionUpdateDestination::LocalSettings,
    })
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

#[cfg(test)]
#[path = "permission.test.rs"]
mod tests;
