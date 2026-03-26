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
        Some("socks5h://localhost:1080")
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
        Some("socks5h://localhost:9001")
    );
}

#[test]
fn test_proxy_env_vars_extended_vars() {
    let state = make_active_state();
    state.activate_network(ProxyPorts {
        http_port: 3128,
        socks_port: 1080,
    });
    let vars = state.proxy_env_vars();

    // Docker proxy
    assert_eq!(
        vars.get("DOCKER_HTTP_PROXY").map(String::as_str),
        Some("http://localhost:3128")
    );
    assert_eq!(
        vars.get("DOCKER_HTTPS_PROXY").map(String::as_str),
        Some("http://localhost:3128")
    );

    // gRPC proxy
    assert_eq!(
        vars.get("GRPC_PROXY").map(String::as_str),
        Some("socks5h://localhost:1080")
    );

    // FTP / RSYNC
    assert_eq!(
        vars.get("FTP_PROXY").map(String::as_str),
        Some("socks5h://localhost:1080")
    );
    assert_eq!(
        vars.get("RSYNC_PROXY").map(String::as_str),
        Some("localhost:1080")
    );

    // gcloud SDK
    assert_eq!(
        vars.get("CLOUDSDK_PROXY_TYPE").map(String::as_str),
        Some("https")
    );
    assert_eq!(
        vars.get("CLOUDSDK_PROXY_ADDRESS").map(String::as_str),
        Some("localhost")
    );
    assert_eq!(
        vars.get("CLOUDSDK_PROXY_PORT").map(String::as_str),
        Some("3128")
    );

    // GIT_SSH_COMMAND
    let git_ssh = vars
        .get("GIT_SSH_COMMAND")
        .expect("GIT_SSH_COMMAND missing");
    assert!(
        git_ssh.contains("nc -X 5 -x localhost:1080"),
        "GIT_SSH_COMMAND should route via SOCKS proxy, got: {git_ssh}"
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

// ==========================================================================
// describe_filesystem / describe_network tests
// ==========================================================================

#[test]
fn test_describe_filesystem_with_writable_roots() {
    let config = SandboxConfig {
        enforcement: EnforcementLevel::WorkspaceWrite,
        writable_roots: vec![WritableRoot::new("/home/user/project")],
        denied_paths: vec![std::path::PathBuf::from("/etc/shadow")],
        denied_read_paths: vec![std::path::PathBuf::from("/root/.ssh")],
        deny_write_paths: vec![std::path::PathBuf::from("/usr")],
        ..Default::default()
    };
    let settings = SandboxSettings::enabled();
    let platform = crate::platform::create_platform();
    let state = SandboxState::new(EnforcementLevel::WorkspaceWrite, settings, config, platform);

    let desc = state.describe_filesystem();
    let parsed: serde_json::Value = serde_json::from_str(&desc).expect("valid JSON");

    // denied_paths should appear in both read.denyOnly and write.denyOnly
    let read_deny = &parsed["read"]["denyOnly"];
    assert!(
        read_deny
            .as_array()
            .expect("array")
            .iter()
            .any(|v| v.as_str() == Some("/etc/shadow")),
        "denied_paths should appear in read.denyOnly: {read_deny}"
    );
    assert!(
        read_deny
            .as_array()
            .expect("array")
            .iter()
            .any(|v| v.as_str() == Some("/root/.ssh")),
        "denied_read_paths should appear in read.denyOnly: {read_deny}"
    );

    let write_deny = &parsed["write"]["denyOnly"];
    assert!(
        write_deny
            .as_array()
            .expect("array")
            .iter()
            .any(|v| v.as_str() == Some("/etc/shadow")),
        "denied_paths should appear in write.denyOnly: {write_deny}"
    );
    assert!(
        write_deny
            .as_array()
            .expect("array")
            .iter()
            .any(|v| v.as_str() == Some("/usr")),
        "deny_write_paths should appear in write.denyOnly: {write_deny}"
    );

    // Writable roots should have readOnlySubpaths
    let allow_write = &parsed["write"]["allowOnly"];
    let first = &allow_write[0];
    assert!(first["path"].as_str().is_some());
    assert!(
        !first["readOnlySubpaths"]
            .as_array()
            .expect("array")
            .is_empty()
    );
}

#[test]
fn test_describe_filesystem_empty_config() {
    let config = SandboxConfig {
        enforcement: EnforcementLevel::ReadOnly,
        ..Default::default()
    };
    let settings = SandboxSettings::enabled();
    let platform = crate::platform::create_platform();
    let state = SandboxState::new(EnforcementLevel::ReadOnly, settings, config, platform);

    let desc = state.describe_filesystem();
    let parsed: serde_json::Value = serde_json::from_str(&desc).expect("valid JSON");

    assert!(
        parsed["read"]["denyOnly"]
            .as_array()
            .expect("array")
            .is_empty()
    );
    assert!(
        parsed["write"]["allowOnly"]
            .as_array()
            .expect("array")
            .is_empty()
    );
    assert!(
        parsed["write"]["denyOnly"]
            .as_array()
            .expect("array")
            .is_empty()
    );
}

#[test]
fn test_describe_network_blocked() {
    let config = SandboxConfig {
        enforcement: EnforcementLevel::WorkspaceWrite,
        allow_network: false,
        ..Default::default()
    };
    let settings = SandboxSettings::enabled();
    let platform = crate::platform::create_platform();
    let state = SandboxState::new(EnforcementLevel::WorkspaceWrite, settings, config, platform);

    let desc = state.describe_network();
    assert!(desc.contains("blocked"), "expected 'blocked', got: {desc}");
}

#[test]
fn test_describe_network_no_proxy() {
    let state = make_active_state();
    // network_active() is false (no proxy started), but allow_network is true
    let desc = state.describe_network();
    assert!(desc.contains("allowed"), "expected 'allowed', got: {desc}");
}

#[test]
fn test_describe_network_with_proxy_and_domains() {
    let mut settings = SandboxSettings::enabled();
    settings.network.allowed_domains = vec!["github.com".to_string(), "*.npmjs.org".to_string()];
    settings.network.denied_domains = vec!["evil.com".to_string()];
    let config = SandboxConfig {
        enforcement: EnforcementLevel::WorkspaceWrite,
        allow_network: true,
        ..Default::default()
    };
    let platform = crate::platform::create_platform();
    let state = SandboxState::new(EnforcementLevel::WorkspaceWrite, settings, config, platform);
    state.activate_network(ProxyPorts::default());

    let desc = state.describe_network();
    let parsed: serde_json::Value = serde_json::from_str(&desc).expect("valid JSON");

    let allowed = parsed["allowedHosts"].as_array().expect("array");
    assert_eq!(allowed.len(), 2);
    assert_eq!(allowed[0].as_str(), Some("github.com"));

    let denied = parsed["deniedHosts"].as_array().expect("array");
    assert_eq!(denied.len(), 1);
    assert_eq!(denied[0].as_str(), Some("evil.com"));
}
