use crate::config::EnforcementLevel;
use crate::config::SandboxBypass;
use crate::config::SandboxConfig;
use crate::config::SandboxSettings;
use crate::config::WritableRoot;

use super::*;

fn make_active_state() -> SandboxState {
    let settings = SandboxSettings::enabled();
    let config = SandboxConfig {
        enforcement: EnforcementLevel::WorkspaceWrite,
        writable_roots: vec![WritableRoot::new("/home/user/project")],
        denied_paths: vec![],
        allow_network: true,
        ..Default::default()
    };
    let platform = crate::platform::create_platform();
    SandboxState::new(EnforcementLevel::WorkspaceWrite, settings, config, platform)
}

#[test]
fn test_disabled_state() {
    let state = SandboxState::disabled();
    assert!(!state.is_active());
    assert_eq!(state.enforcement(), EnforcementLevel::Disabled);
    assert!(!state.auto_allow_enabled());
    assert!(!state.network_active());
}

#[test]
fn test_should_sandbox_command_when_disabled() {
    let state = SandboxState::disabled();
    assert!(!state.should_sandbox_command("echo hello", SandboxBypass::No));
}

#[test]
fn test_should_sandbox_command_when_active() {
    let state = make_active_state();
    if state.is_active() {
        assert!(state.should_sandbox_command("echo hello", SandboxBypass::No));
        // With bypass requested
        assert!(!state.should_sandbox_command("echo hello", SandboxBypass::Requested));
        // Empty command
        assert!(!state.should_sandbox_command("", SandboxBypass::No));
    }
}

#[test]
fn test_auto_allow_enabled() {
    let state = make_active_state();
    if state.is_active() {
        assert!(state.auto_allow_enabled());
    }
}

#[test]
fn test_auto_allow_disabled_when_setting_off() {
    let mut settings = SandboxSettings::enabled();
    settings.auto_allow_bash_if_sandboxed = false;
    let config = SandboxConfig {
        enforcement: EnforcementLevel::ReadOnly,
        ..Default::default()
    };
    let platform = crate::platform::create_platform();
    let state = SandboxState::new(EnforcementLevel::ReadOnly, settings, config, platform);
    if state.is_active() {
        assert!(!state.auto_allow_enabled());
    }
}

#[test]
fn test_proxy_env_vars_empty_when_no_network() {
    let state = SandboxState::disabled();
    assert!(state.proxy_env_vars().is_empty());
}

#[test]
fn test_proxy_env_vars_populated_with_network() {
    let state = make_active_state();
    state.activate_network(ProxyPorts::default());
    let vars = state.proxy_env_vars();
    assert_eq!(
        vars.get("HTTP_PROXY").map(String::as_str),
        Some("http://localhost:3128")
    );
    assert_eq!(
        vars.get("HTTPS_PROXY").map(String::as_str),
        Some("http://localhost:3128")
    );
    assert_eq!(
        vars.get("ALL_PROXY").map(String::as_str),
        Some("socks5://localhost:1080")
    );
    assert!(vars.get("NO_PROXY").is_some());
}

#[test]
fn test_activate_network() {
    let state = make_active_state();
    assert!(!state.network_active());
    state.activate_network(ProxyPorts {
        http_port: 9000,
        socks_port: 9001,
    });
    assert!(state.network_active());
    let vars = state.proxy_env_vars();
    assert_eq!(
        vars.get("HTTP_PROXY").map(String::as_str),
        Some("http://localhost:9000")
    );
    assert_eq!(
        vars.get("ALL_PROXY").map(String::as_str),
        Some("socks5://localhost:9001")
    );
}

#[tokio::test]
async fn test_violation_count() {
    let state = SandboxState::disabled();
    assert_eq!(state.violation_count().await, 0);
}

#[test]
fn test_config_and_settings_accessors() {
    let state = make_active_state();
    assert_eq!(state.config().enforcement, EnforcementLevel::WorkspaceWrite);
    assert!(state.settings().enabled);
}

#[test]
fn test_update_config_hot_reload() {
    let state = make_active_state();
    assert_eq!(state.enforcement(), EnforcementLevel::WorkspaceWrite);
    assert!(state.settings().auto_allow_bash_if_sandboxed);

    // Hot-reload with new settings
    let new_settings = SandboxSettings {
        enabled: true,
        auto_allow_bash_if_sandboxed: false,
        ..SandboxSettings::default()
    };
    let new_config = SandboxConfig {
        enforcement: EnforcementLevel::ReadOnly,
        ..SandboxConfig::default()
    };
    state.update_config(EnforcementLevel::ReadOnly, new_settings, new_config);

    assert_eq!(state.enforcement(), EnforcementLevel::ReadOnly);
    assert!(!state.settings().auto_allow_bash_if_sandboxed);
    assert_eq!(state.config().enforcement, EnforcementLevel::ReadOnly);
}

#[test]
fn test_update_config_preserves_violations_and_network() {
    let state = make_active_state();
    state.activate_network(ProxyPorts::default());
    assert!(state.network_active());

    // Hot-reload config
    state.update_config(
        EnforcementLevel::Strict,
        SandboxSettings::enabled(),
        SandboxConfig::default(),
    );

    // Network and violations should be preserved
    assert!(state.network_active());
    assert_eq!(state.enforcement(), EnforcementLevel::Strict);
}
