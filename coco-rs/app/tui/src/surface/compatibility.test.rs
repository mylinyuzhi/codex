use super::*;

#[test]
fn detects_native_scrollback_without_zellij_env() {
    let compatibility = TerminalCompatibility::detect_with(|_| None);

    assert_eq!(compatibility, TerminalCompatibility::NativeScrollback);
    assert!(compatibility.native_scrollback_enabled());
    assert_eq!(compatibility.status_message(), None);
}

#[test]
fn disables_native_scrollback_when_zellij_env_is_present() {
    let compatibility =
        TerminalCompatibility::detect_with(|name| (name == "ZELLIJ").then(|| "1".to_string()));

    assert_eq!(
        compatibility,
        TerminalCompatibility::ZellijNativeScrollbackDisabled
    );
    assert!(!compatibility.native_scrollback_enabled());
    assert_eq!(
        compatibility.status_message(),
        Some("native scrollback disabled in Zellij")
    );
}

#[test]
fn disables_native_scrollback_when_zellij_session_name_is_present() {
    let compatibility = TerminalCompatibility::detect_with(|name| {
        (name == "ZELLIJ_SESSION_NAME").then(|| "dev".to_string())
    });

    assert_eq!(
        compatibility,
        TerminalCompatibility::ZellijNativeScrollbackDisabled
    );
}

#[test]
fn disables_native_scrollback_when_zellij_version_is_present() {
    let compatibility = TerminalCompatibility::detect_with(|name| {
        (name == "ZELLIJ_VERSION").then(|| "0.43.1".to_string())
    });

    assert_eq!(
        compatibility,
        TerminalCompatibility::ZellijNativeScrollbackDisabled
    );
}
