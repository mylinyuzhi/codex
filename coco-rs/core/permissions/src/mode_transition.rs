//! Permission mode transitions — cycling modes via Shift+Tab.
//!
//! TS: utils/permissions/getNextPermissionMode.ts
//!
//! Mode cycle: default → acceptEdits → plan → [bypass] → [auto] → default
//! Internal users skip acceptEdits and plan.

use coco_types::PermissionMode;
use coco_types::ToolPermissionContext;

use crate::auto_mode_state::AutoModeState;
use crate::dangerous_rules::restore_dangerous_rules;
use crate::dangerous_rules::strip_dangerous_rules;

/// Determine the next permission mode when cycling (Shift+Tab).
///
/// TS: `getNextPermissionMode()` in getNextPermissionMode.ts
///
/// Normal cycle: default → acceptEdits → plan → bypass/auto → default
/// If bypass is unavailable, skip it. If auto is unavailable, skip it.
pub fn get_next_permission_mode(
    context: &ToolPermissionContext,
    is_auto_available: bool,
) -> PermissionMode {
    match context.mode {
        PermissionMode::Default => PermissionMode::AcceptEdits,

        PermissionMode::AcceptEdits => PermissionMode::Plan,

        PermissionMode::Plan => {
            if context.bypass_available {
                return PermissionMode::BypassPermissions;
            }
            if is_auto_available {
                return PermissionMode::Auto;
            }
            PermissionMode::Default
        }

        PermissionMode::BypassPermissions => {
            if is_auto_available {
                return PermissionMode::Auto;
            }
            PermissionMode::Default
        }

        PermissionMode::Auto | PermissionMode::DontAsk => PermissionMode::Default,

        // Bubble and any future mode → default
        _ => PermissionMode::Default,
    }
}

/// Resolve the initial permission mode from CLI flags and settings.
///
/// Priority: explicit CLI flag > settings default > Default
pub fn resolve_predefined_mode(
    cli_mode: Option<PermissionMode>,
    settings_default: Option<PermissionMode>,
) -> PermissionMode {
    cli_mode
        .or(settings_default)
        .unwrap_or(PermissionMode::Default)
}

/// Prepare context for a mode transition (simple — no auto-mode state).
///
/// When entering Plan mode, stash the current mode so we can restore later.
/// When leaving Plan mode, restore the pre-plan mode.
pub fn transition_context(
    mut context: ToolPermissionContext,
    from: PermissionMode,
    to: PermissionMode,
) -> ToolPermissionContext {
    // Entering plan mode: stash current mode
    if to == PermissionMode::Plan && from != PermissionMode::Plan {
        context.pre_plan_mode = Some(from);
    }

    // Leaving plan mode: restore (but don't clear — caller manages lifecycle)
    context.mode = to;
    context
}

/// Prepare context for a mode transition with auto-mode state management.
///
/// Handles dangerous rule stripping on auto-mode entry and restoration on exit.
/// Also manages the `AutoModeState.active` flag.
///
/// Mirrors TS `transitionPermissionMode()` in permissionSetup.ts:
/// - Computes `from_uses_classifier` and `to_uses_classifier` as unified booleans
/// - Entering classifier: activate auto + strip dangerous rules
/// - Leaving classifier: deactivate auto + restore dangerous rules
/// - Plan mode entry handled specially (stash pre_plan_mode)
pub fn transition_context_with_auto(
    mut context: ToolPermissionContext,
    from: PermissionMode,
    to: PermissionMode,
    auto_state: &AutoModeState,
    is_ant_user: bool,
) -> ToolPermissionContext {
    if from == to {
        return context;
    }

    // ── Plan mode entry ──
    // Must happen before the classifier transition logic because
    // entering plan from auto requires stashing pre_plan_mode first.
    if to == PermissionMode::Plan && from != PermissionMode::Plan {
        context.pre_plan_mode = Some(from);
    }

    // ── Classifier transition (TS: lines 621-637) ──
    // "Uses classifier" means: auto mode, OR plan mode with auto active.
    let from_uses_classifier =
        from == PermissionMode::Auto || (from == PermissionMode::Plan && auto_state.is_active());
    // Plan entry with auto is handled by prepareContextForPlanMode (not here).
    let to_uses_classifier = to == PermissionMode::Auto;

    if to_uses_classifier && !from_uses_classifier {
        // Entering classifier territory: activate auto, strip dangerous rules.
        auto_state.set_active(true);
        strip_dangerous_rules(&mut context, is_ant_user);
    } else if from_uses_classifier && !to_uses_classifier {
        // Leaving classifier territory: deactivate auto, restore dangerous rules.
        auto_state.set_active(false);
        restore_dangerous_rules(&mut context);
    }

    // ── Plan mode exit ──
    if from == PermissionMode::Plan && to != PermissionMode::Plan {
        context.pre_plan_mode = None;
    }

    context.mode = to;
    context
}

#[cfg(test)]
#[path = "mode_transition.test.rs"]
mod tests;
