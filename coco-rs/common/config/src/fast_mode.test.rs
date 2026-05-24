use super::*;

#[test]
fn test_cooldown_from_rate_limit() {
    trigger_cooldown(CooldownReason::RateLimit, 5000);
    let state = get_fast_mode_state().unwrap();
    assert!(!state.is_active());
    assert!(!state.is_cooldown_expired(4999));
    assert!(state.is_cooldown_expired(5000));
    assert_eq!(state.remaining_cooldown_ms(3000), 2000);
    assert_eq!(state.remaining_cooldown_ms(6000), 0);
    reset_fast_mode();
}

#[test]
fn test_cooldown_from_status_code() {
    let now = 100_000;
    assert!(trigger_cooldown_from_status(429, now));
    let state = get_fast_mode_state().unwrap();
    match state {
        FastModeState::Cooldown { reason, reset_at } => {
            assert_eq!(reason, CooldownReason::RateLimit);
            assert_eq!(reset_at, now + 60_000);
        }
        FastModeState::Active => panic!("expected cooldown"),
    }

    assert!(trigger_cooldown_from_status(503, now));
    let state = get_fast_mode_state().unwrap();
    match state {
        FastModeState::Cooldown { reason, reset_at } => {
            assert_eq!(reason, CooldownReason::Overloaded);
            assert_eq!(reset_at, now + 120_000);
        }
        FastModeState::Active => panic!("expected cooldown"),
    }

    // 404 should not trigger
    assert!(!trigger_cooldown_from_status(404, now));
    reset_fast_mode();
}

#[test]
fn test_reset_fast_mode() {
    trigger_cooldown(CooldownReason::RateLimit, 9999);
    reset_fast_mode();
    let state = get_fast_mode_state().unwrap();
    assert!(state.is_active());
}

#[test]
fn test_disabled_reason_display() {
    assert_eq!(
        DisabledReason::Free.to_string(),
        "Fast mode is not available on free accounts"
    );
    assert_eq!(
        DisabledReason::Preference.to_string(),
        "Fast mode is disabled by org preference"
    );
}

#[test]
fn test_org_status_lifecycle() {
    set_org_fast_mode_status(OrgFastModeStatus::Disabled {
        reason: DisabledReason::Free,
    });
    let status = get_org_fast_mode_status();
    assert!(matches!(status, OrgFastModeStatus::Disabled { .. }));

    set_org_fast_mode_status(OrgFastModeStatus::Enabled);
    let status = get_org_fast_mode_status();
    assert!(matches!(status, OrgFastModeStatus::Enabled));
}

#[test]
fn test_session_opt_in() {
    set_session_opted_in(false);
    assert!(!is_session_opted_in());
    set_session_opted_in(true);
    assert!(is_session_opted_in());
    set_session_opted_in(false);
}

#[test]
fn test_check_availability_first_party_required() {
    set_org_fast_mode_status(OrgFastModeStatus::Enabled);
    let (available, reason) =
        check_fast_mode_availability(/*is_first_party*/ false, /*per_session*/ false);
    assert!(!available);
    assert!(reason.unwrap().contains("first-party"));
}

#[test]
fn test_check_availability_org_disabled() {
    set_org_fast_mode_status(OrgFastModeStatus::Disabled {
        reason: DisabledReason::Preference,
    });
    let (available, _) =
        check_fast_mode_availability(/*is_first_party*/ true, /*per_session*/ false);
    assert!(!available);
    set_org_fast_mode_status(OrgFastModeStatus::Enabled);
}

#[test]
fn test_check_availability_per_session_opt_in() {
    set_org_fast_mode_status(OrgFastModeStatus::Enabled);
    set_session_opted_in(false);
    let (available, _) =
        check_fast_mode_availability(/*is_first_party*/ true, /*per_session*/ true);
    assert!(!available);

    set_session_opted_in(true);
    let (available, _) =
        check_fast_mode_availability(/*is_first_party*/ true, /*per_session*/ true);
    assert!(available);
    set_session_opted_in(false);
}
