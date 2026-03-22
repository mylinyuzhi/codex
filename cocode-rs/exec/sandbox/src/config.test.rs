use super::*;

#[test]
fn test_sandbox_mode_default() {
    assert_eq!(SandboxMode::default(), SandboxMode::None);
}

#[test]
fn test_sandbox_config_default() {
    let config = SandboxConfig::default();
    assert_eq!(config.mode, SandboxMode::None);
    assert!(config.allowed_paths.is_empty());
    assert!(config.denied_paths.is_empty());
    assert!(!config.allow_network);
}

#[test]
fn test_sandbox_mode_serde_roundtrip() {
    for mode in [
        SandboxMode::None,
        SandboxMode::ReadOnly,
        SandboxMode::Strict,
    ] {
        let json = serde_json::to_string(&mode).expect("serialize");
        let parsed: SandboxMode = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed, mode);
    }
}

#[test]
fn test_sandbox_mode_kebab_case() {
    assert_eq!(
        serde_json::to_string(&SandboxMode::None).expect("serialize"),
        "\"none\""
    );
    assert_eq!(
        serde_json::to_string(&SandboxMode::ReadOnly).expect("serialize"),
        "\"read-only\""
    );
    assert_eq!(
        serde_json::to_string(&SandboxMode::Strict).expect("serialize"),
        "\"strict\""
    );
}

#[test]
fn test_sandbox_config_serde_roundtrip() {
    let config = SandboxConfig {
        mode: SandboxMode::Strict,
        allowed_paths: vec![PathBuf::from("/home/user/project")],
        denied_paths: vec![PathBuf::from("/etc/passwd")],
        allow_network: true,
    };

    let json = serde_json::to_string(&config).expect("serialize");
    let parsed: SandboxConfig = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(parsed.mode, SandboxMode::Strict);
    assert_eq!(parsed.allowed_paths.len(), 1);
    assert_eq!(parsed.denied_paths.len(), 1);
    assert!(parsed.allow_network);
}

#[test]
fn test_sandbox_config_from_empty_json() {
    let config: SandboxConfig = serde_json::from_str("{}").expect("parse");
    assert_eq!(config.mode, SandboxMode::None);
    assert!(config.allowed_paths.is_empty());
    assert!(config.denied_paths.is_empty());
    assert!(!config.allow_network);
}

#[test]
fn test_sandbox_config_partial_json() {
    let config: SandboxConfig = serde_json::from_str(r#"{"mode":"strict"}"#).expect("parse");
    assert_eq!(config.mode, SandboxMode::Strict);
    assert!(config.allowed_paths.is_empty());
    assert!(!config.allow_network);
}

// ==========================================================================
// SandboxSettings tests
// ==========================================================================

#[test]
fn test_sandbox_settings_default_disabled() {
    let settings = SandboxSettings::default();
    assert!(!settings.enabled);
    assert!(settings.auto_allow_bash_if_sandboxed);
    assert!(settings.allow_unsandboxed_commands);
}

#[test]
fn test_sandbox_settings_enabled_constructor() {
    let settings = SandboxSettings::enabled();
    assert!(settings.enabled);
    assert!(settings.auto_allow_bash_if_sandboxed);
    assert!(settings.allow_unsandboxed_commands);
}

#[test]
fn test_sandbox_settings_disabled_constructor() {
    let settings = SandboxSettings::disabled();
    assert!(!settings.enabled);
}

#[test]
fn test_is_sandboxed_disabled_by_default() {
    let settings = SandboxSettings::default();
    // When sandbox is disabled, all commands return false
    assert!(!settings.is_sandboxed("echo hello", false));
    assert!(!settings.is_sandboxed("rm -rf /", false));
    assert!(!settings.is_sandboxed("echo hello", true));
}

#[test]
fn test_is_sandboxed_enabled() {
    let settings = SandboxSettings::enabled();
    // When sandbox is enabled, normal commands return true
    assert!(settings.is_sandboxed("echo hello", false));
    assert!(settings.is_sandboxed("rm -rf /", false));
}

#[test]
fn test_is_sandboxed_bypass_allowed() {
    let settings = SandboxSettings::enabled();
    // When bypass is requested and allowed, returns false
    assert!(!settings.is_sandboxed("echo hello", true));
}

#[test]
fn test_is_sandboxed_bypass_disallowed() {
    let mut settings = SandboxSettings::enabled();
    settings.allow_unsandboxed_commands = false;
    // When bypass is requested but not allowed, returns true
    assert!(settings.is_sandboxed("echo hello", true));
}

#[test]
fn test_is_sandboxed_empty_command() {
    let settings = SandboxSettings::enabled();
    // Empty commands are never sandboxed
    assert!(!settings.is_sandboxed("", false));
    assert!(!settings.is_sandboxed("   ", false));
    assert!(!settings.is_sandboxed("\t\n", false));
}

#[test]
fn test_sandbox_settings_serde_roundtrip() {
    let settings = SandboxSettings {
        enabled: true,
        auto_allow_bash_if_sandboxed: false,
        allow_unsandboxed_commands: false,
    };

    let json = serde_json::to_string(&settings).expect("serialize");
    let parsed: SandboxSettings = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(parsed.enabled, settings.enabled);
    assert_eq!(
        parsed.auto_allow_bash_if_sandboxed,
        settings.auto_allow_bash_if_sandboxed
    );
    assert_eq!(
        parsed.allow_unsandboxed_commands,
        settings.allow_unsandboxed_commands
    );
}

#[test]
fn test_sandbox_settings_from_empty_json() {
    // Empty JSON should use defaults
    let settings: SandboxSettings = serde_json::from_str("{}").expect("parse");
    assert!(!settings.enabled);
    assert!(settings.auto_allow_bash_if_sandboxed);
    assert!(settings.allow_unsandboxed_commands);
}

#[test]
fn test_sandbox_settings_partial_json() {
    // Only enabled=true, rest should default
    let settings: SandboxSettings = serde_json::from_str(r#"{"enabled":true}"#).expect("parse");
    assert!(settings.enabled);
    assert!(settings.auto_allow_bash_if_sandboxed);
    assert!(settings.allow_unsandboxed_commands);
}
