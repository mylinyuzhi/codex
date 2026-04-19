use std::collections::HashMap;

use coco_types::PermissionMode;
use coco_types::ToolPermissionContext;

use super::*;

fn make_context(mode: PermissionMode, bypass: bool) -> ToolPermissionContext {
    ToolPermissionContext {
        mode,
        additional_dirs: HashMap::new(),
        allow_rules: HashMap::new(),
        deny_rules: HashMap::new(),
        ask_rules: HashMap::new(),
        bypass_available: bypass,
        pre_plan_mode: None,
        stripped_dangerous_rules: None,
        session_plan_file: None,
    }
}

// ── get_next_permission_mode ──

#[test]
fn test_default_to_accept_edits() {
    let ctx = make_context(PermissionMode::Default, false);
    assert_eq!(
        get_next_permission_mode(&ctx, false),
        PermissionMode::AcceptEdits
    );
}

#[test]
fn test_accept_edits_to_plan() {
    let ctx = make_context(PermissionMode::AcceptEdits, false);
    assert_eq!(get_next_permission_mode(&ctx, false), PermissionMode::Plan);
}

#[test]
fn test_plan_to_bypass_when_available() {
    let ctx = make_context(PermissionMode::Plan, true);
    assert_eq!(
        get_next_permission_mode(&ctx, false),
        PermissionMode::BypassPermissions
    );
}

#[test]
fn test_plan_to_auto_when_bypass_unavailable() {
    let ctx = make_context(PermissionMode::Plan, false);
    assert_eq!(get_next_permission_mode(&ctx, true), PermissionMode::Auto);
}

#[test]
fn test_plan_to_default_when_nothing_available() {
    let ctx = make_context(PermissionMode::Plan, false);
    assert_eq!(
        get_next_permission_mode(&ctx, false),
        PermissionMode::Default
    );
}

#[test]
fn test_bypass_to_auto_when_available() {
    let ctx = make_context(PermissionMode::BypassPermissions, false);
    assert_eq!(get_next_permission_mode(&ctx, true), PermissionMode::Auto);
}

#[test]
fn test_bypass_to_default() {
    let ctx = make_context(PermissionMode::BypassPermissions, false);
    assert_eq!(
        get_next_permission_mode(&ctx, false),
        PermissionMode::Default
    );
}

#[test]
fn test_auto_to_default() {
    let ctx = make_context(PermissionMode::Auto, false);
    assert_eq!(
        get_next_permission_mode(&ctx, false),
        PermissionMode::Default
    );
}

#[test]
fn test_dont_ask_to_default() {
    let ctx = make_context(PermissionMode::DontAsk, false);
    assert_eq!(
        get_next_permission_mode(&ctx, false),
        PermissionMode::Default
    );
}

// ── resolve_predefined_mode ──

#[test]
fn test_resolve_cli_wins() {
    assert_eq!(
        resolve_predefined_mode(Some(PermissionMode::Auto), Some(PermissionMode::Plan)),
        PermissionMode::Auto
    );
}

#[test]
fn test_resolve_settings_fallback() {
    assert_eq!(
        resolve_predefined_mode(None, Some(PermissionMode::Plan)),
        PermissionMode::Plan
    );
}

#[test]
fn test_resolve_default_fallback() {
    assert_eq!(resolve_predefined_mode(None, None), PermissionMode::Default);
}

// ── transition_context ──

#[test]
fn test_transition_stashes_pre_plan() {
    let ctx = make_context(PermissionMode::Default, false);
    let result = transition_context(ctx, PermissionMode::Default, PermissionMode::Plan);
    assert_eq!(result.mode, PermissionMode::Plan);
    assert_eq!(result.pre_plan_mode, Some(PermissionMode::Default));
}

#[test]
fn test_transition_from_accept_edits_to_plan() {
    let ctx = make_context(PermissionMode::AcceptEdits, false);
    let result = transition_context(ctx, PermissionMode::AcceptEdits, PermissionMode::Plan);
    assert_eq!(result.pre_plan_mode, Some(PermissionMode::AcceptEdits));
}

#[test]
fn test_transition_plan_to_plan_no_stash() {
    let mut ctx = make_context(PermissionMode::Plan, false);
    ctx.pre_plan_mode = Some(PermissionMode::Default);
    let result = transition_context(ctx, PermissionMode::Plan, PermissionMode::Plan);
    // Should NOT overwrite the stash
    assert_eq!(result.pre_plan_mode, Some(PermissionMode::Default));
}

// ── resolve_subagent_mode (TS runAgent.ts:412-434 parity) ──

#[test]
fn subagent_inherits_parent_default_when_no_request() {
    assert_eq!(
        resolve_subagent_mode(PermissionMode::Default, None),
        PermissionMode::Default
    );
}

#[test]
fn subagent_inherits_parent_plan_when_no_request() {
    assert_eq!(
        resolve_subagent_mode(PermissionMode::Plan, None),
        PermissionMode::Plan
    );
}

#[test]
fn subagent_request_wins_over_non_trust_parent() {
    // Parent Default, agent requests Plan → child uses Plan.
    assert_eq!(
        resolve_subagent_mode(PermissionMode::Default, Some(PermissionMode::Plan)),
        PermissionMode::Plan
    );
}

#[test]
fn subagent_request_ignored_when_parent_accept_edits() {
    // Trust mode — child always inherits parent, declaration ignored.
    assert_eq!(
        resolve_subagent_mode(PermissionMode::AcceptEdits, Some(PermissionMode::Plan)),
        PermissionMode::AcceptEdits
    );
}

#[test]
fn subagent_request_ignored_when_parent_bypass() {
    assert_eq!(
        resolve_subagent_mode(
            PermissionMode::BypassPermissions,
            Some(PermissionMode::Plan),
        ),
        PermissionMode::BypassPermissions
    );
}

#[test]
fn subagent_request_ignored_when_parent_auto() {
    assert_eq!(
        resolve_subagent_mode(PermissionMode::Auto, Some(PermissionMode::Default)),
        PermissionMode::Auto
    );
}

// ── apply_auto_transition_to_app_state ──

#[test]
fn auto_transition_clears_stash_on_leaving_auto() {
    // TS parity: `permissionSetup.ts:627-637` restores dangerous
    // permissions (clears the stash) when the classifier exits.
    let mut state = coco_types::ToolAppState {
        stripped_dangerous_rules: Some(coco_types::PermissionRulesBySource::default()),
        ..Default::default()
    };
    let modified = apply_auto_transition_to_app_state(
        &mut state,
        PermissionMode::Auto,
        PermissionMode::Default,
    );
    assert!(modified, "Auto→Default with stash should report modified");
    assert!(state.stripped_dangerous_rules.is_none());
}

#[test]
fn auto_transition_noop_when_entering_auto() {
    // Entering Auto: full rule-stashing is deferred (needs central
    // rules store). Helper returns false and leaves stash alone.
    let mut state = coco_types::ToolAppState::default();
    let modified = apply_auto_transition_to_app_state(
        &mut state,
        PermissionMode::Default,
        PermissionMode::Auto,
    );
    assert!(!modified);
    assert!(state.stripped_dangerous_rules.is_none());
}

#[test]
fn auto_transition_noop_when_no_stash_to_clear() {
    // Leaving Auto but no stash present (e.g. Auto was purely a mode
    // label with no rules stashed) → no-op.
    let mut state = coco_types::ToolAppState::default();
    let modified = apply_auto_transition_to_app_state(
        &mut state,
        PermissionMode::Auto,
        PermissionMode::Default,
    );
    assert!(!modified);
}

#[test]
fn auto_transition_noop_for_non_auto_boundary() {
    // Default → Plan shouldn't touch the stash either way.
    let mut state = coco_types::ToolAppState {
        stripped_dangerous_rules: Some(coco_types::PermissionRulesBySource::default()),
        ..Default::default()
    };
    let modified = apply_auto_transition_to_app_state(
        &mut state,
        PermissionMode::Default,
        PermissionMode::Plan,
    );
    assert!(!modified);
    // Stash preserved — non-Auto transitions don't manage it.
    assert!(state.stripped_dangerous_rules.is_some());
}
