//! Sandbox bootstrap for session initialization.
//!
//! Runs the 4-gate enable check, constructs `SandboxState`, and starts
//! the network proxy servers. Returns `None` when sandbox is disabled.

use std::sync::Arc;

use cocode_config::Config;
use cocode_sandbox::EnforcementLevel;
use cocode_sandbox::ProxyPorts;
use cocode_sandbox::SandboxConfig;
use cocode_sandbox::SandboxState;
use cocode_sandbox::WritableRoot;
use cocode_sandbox::bootstrap::EnableCheckResult;
use cocode_sandbox::bootstrap::check_enable_gates;
use cocode_sandbox::platform::create_platform;
use cocode_sandbox::proxy::DomainFilter;
use cocode_sandbox::proxy::ProxyServer;
use tokio_util::sync::CancellationToken;

/// Result of sandbox initialization including state and proxy server.
pub struct SandboxInitResult {
    /// Shared sandbox state for the session.
    pub state: Arc<SandboxState>,
    /// Running proxy server (if network filtering is active).
    pub proxy_server: Option<ProxyServer>,
}

/// Initialize sandbox state from the session config.
///
/// Returns `Some(Arc<SandboxState>)` if sandbox is enabled and all gates pass,
/// or `None` if sandbox is disabled by settings, platform, or missing deps.
pub fn initialize_sandbox(config: &Config) -> Option<Arc<SandboxState>> {
    let settings = &config.sandbox_settings;
    let is_external = config.sandbox_mode.is_external_sandbox();

    // External sandbox only requires settings.enabled (gates 2-4 don't apply
    // since platform wrapping is skipped). Standard sandbox uses the full
    // 4-gate check (settings, platform, allowlist, deps).
    if is_external {
        if !settings.enabled {
            tracing::debug!("Sandbox disabled: settings.enabled is false");
            return None;
        }
    } else {
        let gate_result = check_enable_gates(settings);
        match gate_result {
            EnableCheckResult::Enabled => {}
            other => {
                tracing::debug!("Sandbox disabled: {other:?}");
                return None;
            }
        }
    }

    // Convert protocol SandboxMode → enforcement level
    let enforcement = EnforcementLevel::from(config.sandbox_mode);
    if enforcement == EnforcementLevel::Disabled {
        // Fail-closed network (from codex-rs): still create sandbox state when
        // domain filtering is configured, so the managed proxy enforces network
        // restrictions even with full filesystem access. The proxy is a separate
        // security layer from filesystem enforcement.
        let has_domain_filtering = !settings.network.allowed_domains.is_empty()
            || !settings.network.denied_domains.is_empty();
        if !has_domain_filtering {
            tracing::debug!("Sandbox disabled: enforcement level is Disabled (FullAccess mode)");
            return None;
        }
        tracing::info!(
            "Filesystem enforcement disabled (FullAccess) but domain filtering configured; \
             creating sandbox state for fail-closed network proxy"
        );
    }

    // Build SandboxConfig from the session config
    let mut writable_roots: Vec<WritableRoot> = config
        .writable_roots
        .iter()
        .map(WritableRoot::new)
        .collect();

    // Merge filesystem.allow_write into writable roots
    for path in &settings.filesystem.allow_write {
        if !writable_roots.iter().any(|r| r.path == *path) {
            writable_roots.push(WritableRoot::new(path));
        }
    }

    let sandbox_config = SandboxConfig {
        enforcement,
        writable_roots,
        denied_paths: Vec::new(),
        denied_read_paths: settings.filesystem.deny_read.clone(),
        deny_write_paths: settings.filesystem.deny_write.clone(),
        allow_git_config: settings.filesystem.allow_git_config,
        allow_network: true, // Default to allowing network; proxy will filter
        extra_bind_ro: Vec::new(),
        weaker_network_isolation: settings.enable_weaker_network_isolation,
        allow_pty: settings.allow_pty,
        ..Default::default()
    };

    // ExternalSandbox: skip platform wrapping (bwrap/Seatbelt); the environment
    // is already sandboxed by Docker, CI, or another external mechanism.
    let state = if is_external {
        tracing::info!(
            enforcement = ?enforcement,
            "Sandbox initialized in external mode (platform wrapping skipped)"
        );
        SandboxState::external(enforcement, settings.clone(), sandbox_config)
    } else {
        let platform = create_platform();
        let s = SandboxState::new(enforcement, settings.clone(), sandbox_config, platform);

        if s.is_active() {
            tracing::info!(
                enforcement = ?enforcement,
                "Sandbox initialized with platform enforcement"
            );
        } else {
            tracing::warn!("Sandbox enabled in settings but platform enforcement unavailable");
        }
        s
    };

    Some(Arc::new(state))
}

/// Result of starting sandbox networking (proxy + optional bridge).
pub struct SandboxNetworkResult {
    /// Running proxy server.
    pub proxy: ProxyServer,
    /// Running bridge manager (Linux only, when network is unshared).
    pub bridge: Option<cocode_sandbox::BridgeManager>,
}

/// Start the network proxy for sandbox domain filtering.
///
/// Creates HTTP CONNECT and SOCKS5 proxy servers using the domain
/// allow/deny lists from the sandbox network config. On Linux, also
/// starts socat bridges so the proxy is reachable across the bwrap
/// network namespace boundary.
///
/// Returns the running handles (must be kept alive for the session
/// lifetime). Returns `None` if domain filtering is not configured.
pub async fn start_sandbox_proxy(
    state: &Arc<SandboxState>,
    cancel_token: CancellationToken,
) -> Option<SandboxNetworkResult> {
    let settings = state.settings();
    let network = &settings.network;

    // Skip proxy when no domain filtering is configured
    if network.allowed_domains.is_empty() && network.denied_domains.is_empty() {
        tracing::debug!("Sandbox proxy skipped: no domain filtering configured");
        return None;
    }

    let filter = Arc::new(DomainFilter::from_network_config(network));

    let proxy = match ProxyServer::start_with_ports(
        filter,
        cancel_token.clone(),
        network.http_proxy_port,
        network.socks_proxy_port,
    )
    .await
    {
        Ok(server) => server,
        Err(e) => {
            tracing::warn!(error = %e, "Failed to start sandbox proxy servers");
            return None;
        }
    };

    let ports = ProxyPorts {
        http_port: proxy.http_port(),
        socks_port: proxy.socks_port(),
    };
    state.activate_network(ports);

    // On Linux, start socat bridges for proxy across network namespace
    let bridge = start_bridge_if_needed(state, &proxy, cancel_token).await;

    tracing::info!(
        http_port = ports.http_port,
        socks_port = ports.socks_port,
        bridge_active = bridge.is_some(),
        "Sandbox network proxy active"
    );

    Some(SandboxNetworkResult { proxy, bridge })
}

/// Start socat bridges on Linux when network namespace is unshared.
///
/// The bridge creates Unix domain sockets that are bind-mounted into the
/// bwrap sandbox, allowing sandboxed commands to reach the host proxy.
async fn start_bridge_if_needed(
    state: &Arc<SandboxState>,
    proxy: &ProxyServer,
    cancel_token: CancellationToken,
) -> Option<cocode_sandbox::BridgeManager> {
    // Only needed on Linux (macOS Seatbelt uses different network isolation)
    if !cfg!(target_os = "linux") {
        return None;
    }

    // Only needed when network is NOT allowed (bwrap will use --unshare-net)
    if state.config().allow_network {
        return None;
    }

    // Find socat binary
    let socat_path = find_socat()?;

    let session_tag = state.session_tag().replace('_', "");
    match cocode_sandbox::BridgeManager::start(
        socat_path,
        proxy.http_port(),
        proxy.socks_port(),
        &session_tag,
        cancel_token,
    )
    .await
    {
        Ok(bridge) => {
            // Register bridge socket paths for bwrap bind mounts
            let bind_paths: Vec<_> = bridge.socket_paths().iter().map(|p| (*p).clone()).collect();
            state.set_bridge_bind_paths(bind_paths);
            Some(bridge)
        }
        Err(e) => {
            tracing::warn!(error = %e, "Failed to start proxy bridge; network filtering may not work in sandbox");
            None
        }
    }
}

/// Find the socat binary on the system.
fn find_socat() -> Option<std::path::PathBuf> {
    let paths = ["/usr/bin/socat", "/usr/local/bin/socat"];
    paths
        .iter()
        .map(std::path::PathBuf::from)
        .find(|p| p.exists())
}

#[cfg(test)]
#[path = "sandbox_init.test.rs"]
mod tests;
