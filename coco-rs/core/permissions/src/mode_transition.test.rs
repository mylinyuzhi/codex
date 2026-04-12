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
