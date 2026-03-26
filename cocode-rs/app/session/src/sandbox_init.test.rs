use cocode_protocol::SandboxMode;
use cocode_sandbox::SandboxSettings;
use cocode_sandbox::config::NetworkConfig;

use super::initialize_sandbox;
use super::start_sandbox_proxy;

/// Build a Config with the given sandbox mode and settings.
/// Uses ConfigOverrides to avoid accessing private fields.
fn make_config(
    mode: SandboxMode,
    settings: SandboxSettings,
    writable_roots: Vec<std::path::PathBuf>,
) -> cocode_config::Config {
    let mgr = cocode_config::ConfigManager::from_default()
        .unwrap_or_else(|_| cocode_config::ConfigManager::empty());
    let overrides = cocode_config::ConfigOverrides::new()
        .with_sandbox_mode(mode)
        .with_sandbox_settings(settings)
        .with_writable_roots(writable_roots);
    mgr.build_config(overrides).expect("build config")
}

#[test]
fn test_sandbox_disabled_by_default() {
    let config = make_config(
        SandboxMode::default(),
        SandboxSettings::default(),
        Vec::new(),
    );
    assert!(initialize_sandbox(&config).is_none());
}

#[test]
fn test_sandbox_disabled_when_full_access() {
    let config = make_config(
        SandboxMode::FullAccess,
        SandboxSettings::enabled(),
        Vec::new(),
    );
    // FullAccess maps to Disabled enforcement
    assert!(initialize_sandbox(&config).is_none());
}

#[test]
fn test_sandbox_enabled_when_read_only_and_settings_enabled() {
    let config = make_config(
        SandboxMode::ReadOnly,
        SandboxSettings::enabled(),
        Vec::new(),
    );
    // Platform may or may not be available in test environment
    let state = initialize_sandbox(&config);
    if let Some(state) = state {
        assert!(state.settings().enabled);
    }
}

#[test]
fn test_sandbox_enabled_workspace_write_with_roots() {
    let config = make_config(
        SandboxMode::WorkspaceWrite,
        SandboxSettings::enabled(),
        vec![std::path::PathBuf::from("/tmp/test-workspace")],
    );
    let state = initialize_sandbox(&config);
    if let Some(state) = state {
        assert_eq!(state.config().writable_roots.len(), 1);
        assert_eq!(
            state.config().writable_roots[0].path,
            std::path::PathBuf::from("/tmp/test-workspace")
        );
    }
}

#[tokio::test]
async fn test_start_sandbox_proxy_skipped_when_no_domain_filtering() {
    let state = std::sync::Arc::new(cocode_sandbox::SandboxState::disabled());
    let token = tokio_util::sync::CancellationToken::new();
    let proxy = start_sandbox_proxy(&state, token).await;
    // No domain allow/deny lists → proxy not started
    assert!(proxy.is_none());
}

#[tokio::test]
async fn test_start_sandbox_proxy_activates_network() {
    let settings = SandboxSettings {
        enabled: true,
        network: NetworkConfig {
            allowed_domains: vec!["github.com".to_string()],
            ..NetworkConfig::default()
        },
        ..SandboxSettings::enabled()
    };
    let config = cocode_sandbox::SandboxConfig {
        enforcement: cocode_sandbox::EnforcementLevel::WorkspaceWrite,
        ..Default::default()
    };
    let platform = cocode_sandbox::platform::create_platform();
    let state = std::sync::Arc::new(cocode_sandbox::SandboxState::new(
        cocode_sandbox::EnforcementLevel::WorkspaceWrite,
        settings,
        config,
        platform,
    ));

    let token = tokio_util::sync::CancellationToken::new();
    let result = start_sandbox_proxy(&state, token.clone()).await;

    assert!(
        result.is_some(),
        "proxy should start when domain filtering is configured"
    );
    assert!(state.network_active(), "network isolation should be active");

    let env_vars = state.proxy_env_vars();
    assert!(
        env_vars.contains_key("HTTP_PROXY"),
        "HTTP_PROXY should be set"
    );
    assert!(
        env_vars.contains_key("ALL_PROXY"),
        "ALL_PROXY should be set"
    );

    // Clean up
    token.cancel();
    if let Some(mut r) = result {
        r.proxy.stop().await;
    }
}
