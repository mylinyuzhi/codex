use coco_types::PermissionMode;

use super::KillswitchCheck;
use super::check;
use super::check_transition;
use super::compute_bypass_capability;
use super::resolve_initial_permission_mode;

fn clear_env() {
    unsafe {
        std::env::remove_var(super::KILLSWITCH_ENV);
    }
}

/// All env-var tests run in a single case because cargo's default test
/// harness parallelizes tests and env-var mutation is process-global.
/// Splitting into separate `#[test]` functions caused flakes where a
/// sibling test observed the env var mid-flight (verified via CI race).
#[test]
fn killswitch_full_behavior() {
    // — Allowed when neither env nor policy is engaged.
    clear_env();
    assert_eq!(check(None), KillswitchCheck::Allowed);
    assert_eq!(check(Some(false)), KillswitchCheck::Allowed);

    // — Policy flag blocks.
    let policy = check(Some(true));
    assert_eq!(policy, KillswitchCheck::BlockedByPolicy);
    assert!(!policy.is_allowed());
    assert!(policy.reason().unwrap().contains("enterprise policy"));

    // — Env flag blocks and takes precedence over policy.
    unsafe {
        std::env::set_var(super::KILLSWITCH_ENV, "1");
    }
    assert_eq!(check(None), KillswitchCheck::BlockedByEnv);
    assert_eq!(check(Some(true)), KillswitchCheck::BlockedByEnv);
    clear_env();

    // — Transition gate: killswitch only blocks `BypassPermissions`.
    assert_eq!(
        check_transition(PermissionMode::Default, Some(true)),
        KillswitchCheck::Allowed,
    );
    assert_eq!(
        check_transition(PermissionMode::AcceptEdits, Some(true)),
        KillswitchCheck::Allowed,
    );
    assert_eq!(
        check_transition(PermissionMode::Plan, Some(true)),
        KillswitchCheck::Allowed,
    );
    assert_eq!(
        check_transition(PermissionMode::BypassPermissions, Some(true)),
        KillswitchCheck::BlockedByPolicy,
    );
    assert_eq!(
        check_transition(PermissionMode::BypassPermissions, None),
        KillswitchCheck::Allowed,
    );

    // — Env truthy-ness matches TS `isTruthyEnvValue` rules.
    for value in ["1", "true", "True", "YES", "on"] {
        unsafe {
            std::env::set_var(super::KILLSWITCH_ENV, value);
        }
        assert_eq!(
            check(None),
            KillswitchCheck::BlockedByEnv,
            "expected `{value}` to be truthy"
        );
    }
    for value in ["0", "false", "no", "off", ""] {
        unsafe {
            std::env::set_var(super::KILLSWITCH_ENV, value);
        }
        assert_eq!(
            check(None),
            KillswitchCheck::Allowed,
            "expected `{value}` to be falsy"
        );
    }
    clear_env();
}

/// TS parity matrix for `isBypassPermissionsModeAvailable`
/// (`permissionSetup.ts:939-943`). Every combination of the three
/// inputs exhausted so future refactors can't silently break the gate.
#[test]
fn compute_bypass_capability_matrix() {
    clear_env();

    // Neither trigger → never available.
    assert!(!compute_bypass_capability(false, false, None));
    assert!(!compute_bypass_capability(false, false, Some(false)));
    assert!(!compute_bypass_capability(false, false, Some(true)));

    // `starts_in_bypass_mode` alone → available iff killswitch allows.
    assert!(compute_bypass_capability(true, false, None));
    assert!(compute_bypass_capability(true, false, Some(false)));
    assert!(!compute_bypass_capability(true, false, Some(true))); // policy blocks

    // `allow_dangerously_skip_permissions` alone → same rule.
    assert!(compute_bypass_capability(false, true, None));
    assert!(compute_bypass_capability(false, true, Some(false)));
    assert!(!compute_bypass_capability(false, true, Some(true))); // policy blocks

    // Both triggers → still gated by killswitch (policy is a hard deny).
    assert!(compute_bypass_capability(true, true, None));
    assert!(!compute_bypass_capability(true, true, Some(true)));

    // Env killswitch overrides everything.
    unsafe {
        std::env::set_var(super::KILLSWITCH_ENV, "1");
    }
    assert!(!compute_bypass_capability(true, false, None));
    assert!(!compute_bypass_capability(false, true, None));
    assert!(!compute_bypass_capability(true, true, None));
    clear_env();
}

/// TS parity matrix for `initialPermissionModeFromCLI`
/// (`permissionSetup.ts:689-811`). The walk-and-skip behavior is
/// load-bearing: when the killswitch blocks `BypassPermissions`,
/// the walk must fall through to the next candidate, not collapse
/// straight to `Default`.
#[test]
fn resolve_initial_permission_mode_matrix() {
    clear_env();

    // — No candidates → Default, no notification.
    let r = resolve_initial_permission_mode(false, None, None, None);
    assert_eq!(r.mode, PermissionMode::Default);
    assert!(r.notification.is_none());

    // — Only --dangerously-skip, killswitch off → Bypass, no notification.
    let r = resolve_initial_permission_mode(true, None, None, None);
    assert_eq!(r.mode, PermissionMode::BypassPermissions);
    assert!(r.notification.is_none());

    // — Only --dangerously-skip, killswitch on → Default, notification set.
    let r = resolve_initial_permission_mode(true, None, None, Some(true));
    assert_eq!(r.mode, PermissionMode::Default);
    assert!(r.notification.is_some());

    // — --dangerously-skip + --permission-mode acceptEdits, killswitch on
    //   → AcceptEdits (TS walk-and-skip). This is the case my old code
    //   collapsed to Default instead of falling through.
    let r =
        resolve_initial_permission_mode(true, Some(PermissionMode::AcceptEdits), None, Some(true));
    assert_eq!(r.mode, PermissionMode::AcceptEdits);
    assert!(r.notification.is_some()); // downgrade still surfaced

    // — --dangerously-skip + settings.default_mode = acceptEdits,
    //   killswitch on → AcceptEdits (second fallback in the walk).
    let r =
        resolve_initial_permission_mode(true, None, Some(PermissionMode::AcceptEdits), Some(true));
    assert_eq!(r.mode, PermissionMode::AcceptEdits);
    assert!(r.notification.is_some());

    // — --dangerously-skip + --permission-mode + settings default, all
    //   with killswitch on, --permission-mode also bypassPermissions.
    //   The walk skips bypass twice, falls to settings default.
    let r = resolve_initial_permission_mode(
        true,
        Some(PermissionMode::BypassPermissions),
        Some(PermissionMode::Plan),
        Some(true),
    );
    assert_eq!(r.mode, PermissionMode::Plan);

    // — All three are bypass, killswitch on → Default (walk exhausts).
    let r = resolve_initial_permission_mode(
        true,
        Some(PermissionMode::BypassPermissions),
        Some(PermissionMode::BypassPermissions),
        Some(true),
    );
    assert_eq!(r.mode, PermissionMode::Default);
    assert!(r.notification.is_some());

    // — --permission-mode alone wins over settings default (first in order).
    let r = resolve_initial_permission_mode(
        false,
        Some(PermissionMode::Plan),
        Some(PermissionMode::AcceptEdits),
        None,
    );
    assert_eq!(r.mode, PermissionMode::Plan);

    // — Only settings default, no CLI → use it.
    let r = resolve_initial_permission_mode(false, None, Some(PermissionMode::AcceptEdits), None);
    assert_eq!(r.mode, PermissionMode::AcceptEdits);

    // — Settings default is bypass + killswitch → Default (no other
    //   candidate to fall through to).
    let r = resolve_initial_permission_mode(
        false,
        None,
        Some(PermissionMode::BypassPermissions),
        Some(true),
    );
    assert_eq!(r.mode, PermissionMode::Default);

    // — Env killswitch also blocks (same walk behavior).
    unsafe {
        std::env::set_var(super::KILLSWITCH_ENV, "1");
    }
    let r = resolve_initial_permission_mode(true, Some(PermissionMode::AcceptEdits), None, None);
    assert_eq!(r.mode, PermissionMode::AcceptEdits);
    assert!(r.notification.is_some());
    clear_env();
}
