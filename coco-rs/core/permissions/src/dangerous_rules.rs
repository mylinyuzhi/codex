//! Dangerous rule stripping for auto-mode.
//!
//! TS: permissionSetup.ts — `stripDangerousPermissionsForAutoMode()` and
//!     `restoreDangerousPermissions()`
//!
//! When entering auto-mode, certain user-configured allow rules that could
//! bypass the classifier's safety checks are temporarily removed and stashed.
//! They are restored when exiting auto-mode.

use coco_types::PermissionBehavior;
use coco_types::PermissionRuleSource;
use coco_types::PermissionRulesBySource;
use coco_types::ToolName;
use coco_types::ToolPermissionContext;

use crate::setup::is_dangerous_bash_permission;
use crate::setup::is_dangerous_powershell_permission;

/// Strip dangerous classifier-bypassing rules from the permission context.
///
/// Scans `allow_rules` for dangerous patterns (python:*, node:*, eval, ssh,
/// curl, git, sudo, etc.) and moves them to `stripped_dangerous_rules`.
///
/// TS: `stripDangerousPermissionsForAutoMode(context)`
pub fn strip_dangerous_rules(context: &mut ToolPermissionContext, is_ant_user: bool) {
    let mut stripped = PermissionRulesBySource::new();

    for (source, rules) in &mut context.allow_rules {
        let mut safe = Vec::new();
        let mut dangerous = Vec::new();

        for rule in rules.drain(..) {
            if rule.behavior == PermissionBehavior::Allow
                && (is_dangerous_bash_permission(
                    &rule.value.tool_pattern,
                    rule.value.rule_content.as_deref(),
                    is_ant_user,
                ) || is_dangerous_powershell_permission(
                    &rule.value.tool_pattern,
                    rule.value.rule_content.as_deref(),
                ) || is_dangerous_agent_permission(&rule.value.tool_pattern))
            {
                dangerous.push(rule);
            } else {
                safe.push(rule);
            }
        }

        *rules = safe;
        // Only stash rules from restorable sources.
        // Policy/flag/command rules are removed but NOT restored on exit.
        // TS: isPermissionUpdateDestination() filters to user/project/local/session/cliArg.
        if !dangerous.is_empty() && is_restorable_source(*source) {
            stripped.entry(*source).or_default().extend(dangerous);
        }
    }

    if !stripped.is_empty() {
        context.stripped_dangerous_rules = Some(stripped);
    }
}

/// Restore previously stripped dangerous rules.
///
/// Moves rules from `stripped_dangerous_rules` back into `allow_rules`.
/// Clears the stash after restoration.
///
/// TS: `restoreDangerousPermissions(context)`
pub fn restore_dangerous_rules(context: &mut ToolPermissionContext) {
    if let Some(stripped) = context.stripped_dangerous_rules.take() {
        for (source, rules) in stripped {
            context.allow_rules.entry(source).or_default().extend(rules);
        }
    }
}

/// Any Agent allow rule bypasses the classifier's sub-agent evaluation.
///
/// TS: `isDangerousTaskPermission()` — returns true if tool is Agent.
fn is_dangerous_agent_permission(tool_name: &str) -> bool {
    tool_name == ToolName::Agent.as_str()
}

/// Whether a rule source is restorable on auto-mode exit.
///
/// Policy/flag/command rules are removed but not stashed for restoration,
/// since restoring them would bypass enterprise controls.
///
/// TS: `isPermissionUpdateDestination()` — filters to user/project/local/session/cliArg.
fn is_restorable_source(source: PermissionRuleSource) -> bool {
    matches!(
        source,
        PermissionRuleSource::UserSettings
            | PermissionRuleSource::ProjectSettings
            | PermissionRuleSource::LocalSettings
            | PermissionRuleSource::Session
            | PermissionRuleSource::CliArg
    )
}

#[cfg(test)]
#[path = "dangerous_rules.test.rs"]
mod tests;
