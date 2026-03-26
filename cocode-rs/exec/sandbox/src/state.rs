//! Sandbox runtime state.
//!
//! `SandboxState` is the central coordination point for sandbox enforcement,
//! shared via `Arc` across the system (shell executor, tool context, etc.).

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::RwLock;

use tokio::sync::Mutex;

use crate::config::EnforcementLevel;
use crate::config::SandboxBypass;
use crate::config::SandboxConfig;
use crate::config::SandboxSettings;
use crate::monitor::generate_session_tag;
use crate::platform::SandboxPlatform;
use crate::violation::ViolationStore;

/// Network proxy port configuration.
#[derive(Debug, Clone, Copy)]
pub struct ProxyPorts {
    /// HTTP proxy port (default: 3128).
    pub http_port: u16,
    /// SOCKS proxy port (default: 1080).
    pub socks_port: u16,
}

impl Default for ProxyPorts {
    fn default() -> Self {
        Self {
            http_port: 3128,
            socks_port: 1080,
        }
    }
}

/// Hot-reloadable sandbox configuration fields.
///
/// Wrapped in `RwLock` so the `/sandbox` command or settings changes
/// can update enforcement without re-creating the entire state.
struct MutableConfig {
    enforcement: EnforcementLevel,
    settings: SandboxSettings,
    config: SandboxConfig,
    network_active: bool,
    proxy_ports: Option<ProxyPorts>,
    /// Extra paths to bind-mount into the sandbox (e.g., bridge UDS sockets).
    bridge_bind_paths: Vec<std::path::PathBuf>,
}

/// Runtime sandbox state, shared via `Arc` across the system.
///
/// Immutable fields (platform, violations) live directly on the struct.
/// Mutable fields (enforcement, settings, config) are behind a `RwLock`
/// to support hot-reload when the user changes sandbox settings mid-session.
pub struct SandboxState {
    /// Whether platform enforcement is active (kernel-level).
    platform_active: bool,
    /// Violation store (ring buffer, max 100).
    violations: Arc<Mutex<ViolationStore>>,
    /// Hot-reloadable configuration.
    mutable: RwLock<MutableConfig>,
    /// Platform-specific sandbox implementation.
    platform: Box<dyn SandboxPlatform>,
    /// Session-unique tag for macOS log stream filtering and command correlation.
    session_tag: String,
}

impl std::fmt::Debug for SandboxState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let m = self
            .mutable
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        f.debug_struct("SandboxState")
            .field("enforcement", &m.enforcement)
            .field("platform_active", &self.platform_active)
            .field("network_active", &m.network_active)
            .field("proxy_ports", &m.proxy_ports)
            .finish()
    }
}

impl SandboxState {
    /// Create a new sandbox state.
    ///
    /// Call `bootstrap::check_enable_gates()` first to determine if sandbox
    /// should be enabled, then construct the state accordingly.
    pub fn new(
        enforcement: EnforcementLevel,
        settings: SandboxSettings,
        config: SandboxConfig,
        platform: Box<dyn SandboxPlatform>,
    ) -> Self {
        let platform_active = enforcement != EnforcementLevel::Disabled && platform.available();

        let mut store = ViolationStore::new();
        if !settings.ignore_violations.is_empty() {
            store.set_ignore_patterns(settings.ignore_violations.clone());
        }

        Self {
            platform_active,
            violations: Arc::new(Mutex::new(store)),
            mutable: RwLock::new(MutableConfig {
                enforcement,
                network_active: false,
                proxy_ports: None,
                settings,
                config,
                bridge_bind_paths: Vec::new(),
            }),
            platform,
            session_tag: generate_session_tag(),
        }
    }

    /// Create a disabled (no-op) sandbox state.
    pub fn disabled() -> Self {
        Self {
            platform_active: false,
            violations: Arc::new(Mutex::new(ViolationStore::new())),
            mutable: RwLock::new(MutableConfig {
                enforcement: EnforcementLevel::Disabled,
                network_active: false,
                proxy_ports: None,
                settings: SandboxSettings::default(),
                config: SandboxConfig::default(),
                bridge_bind_paths: Vec::new(),
            }),
            platform: crate::platform::create_platform(),
            session_tag: generate_session_tag(),
        }
    }

    /// Whether sandbox enforcement is active.
    pub fn is_active(&self) -> bool {
        let m = self
            .mutable
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        m.enforcement != EnforcementLevel::Disabled && self.platform_active
    }

    /// Current enforcement level.
    pub fn enforcement(&self) -> EnforcementLevel {
        self.mutable
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .enforcement
    }

    /// Whether a command should be sandboxed.
    pub fn should_sandbox_command(&self, command: &str, bypass: SandboxBypass) -> bool {
        let m = self
            .mutable
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        m.enforcement != EnforcementLevel::Disabled
            && self.platform_active
            && m.settings.is_sandboxed(command, bypass)
    }

    /// Whether auto-allow for bash commands is enabled.
    pub fn auto_allow_enabled(&self) -> bool {
        let m = self
            .mutable
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        m.enforcement != EnforcementLevel::Disabled
            && self.platform_active
            && m.settings.auto_allow_bash_if_sandboxed
    }

    /// Get a snapshot of the sandbox settings.
    pub fn settings(&self) -> SandboxSettings {
        self.mutable
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .settings
            .clone()
    }

    /// Get a snapshot of the sandbox config with bridge bind paths merged.
    pub fn config(&self) -> SandboxConfig {
        let m = self
            .mutable
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut config = m.config.clone();
        if !m.bridge_bind_paths.is_empty() {
            config
                .extra_bind_ro
                .extend(m.bridge_bind_paths.iter().cloned());
        }
        config
    }

    /// Get the session-unique tag for log filtering and command correlation.
    pub fn session_tag(&self) -> &str {
        &self.session_tag
    }

    /// Get the platform sandbox implementation.
    pub fn platform(&self) -> &dyn SandboxPlatform {
        &*self.platform
    }

    /// Get the violation store.
    pub fn violations(&self) -> &Arc<Mutex<ViolationStore>> {
        &self.violations
    }

    /// Get the current violation count (non-benign).
    pub async fn violation_count(&self) -> i32 {
        self.violations.lock().await.non_benign_count()
    }

    /// Get proxy environment variables for injection into sandboxed commands.
    pub fn proxy_env_vars(&self) -> HashMap<String, String> {
        let m = self
            .mutable
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(ports) = m.proxy_ports else {
            return HashMap::new();
        };

        let http_proxy = format!("http://localhost:{}", ports.http_port);
        let socks_proxy = format!("socks5://localhost:{}", ports.socks_port);
        let no_proxy = "localhost,127.0.0.1,::1,10.0.0.0/8,172.16.0.0/12,192.168.0.0/16";

        HashMap::from([
            ("HTTP_PROXY".to_string(), http_proxy.clone()),
            ("HTTPS_PROXY".to_string(), http_proxy.clone()),
            ("http_proxy".to_string(), http_proxy.clone()),
            ("https_proxy".to_string(), http_proxy),
            ("ALL_PROXY".to_string(), socks_proxy.clone()),
            ("all_proxy".to_string(), socks_proxy),
            ("NO_PROXY".to_string(), no_proxy.to_string()),
            ("no_proxy".to_string(), no_proxy.to_string()),
        ])
    }

    /// Set network isolation as active with the given proxy ports.
    pub fn activate_network(&self, ports: ProxyPorts) {
        let mut m = self
            .mutable
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        m.network_active = true;
        m.proxy_ports = Some(ports);
    }

    /// Register bridge socket paths to be bind-mounted into the sandbox.
    pub fn set_bridge_bind_paths(&self, paths: Vec<std::path::PathBuf>) {
        let mut m = self
            .mutable
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        m.bridge_bind_paths = paths;
    }

    /// Whether network isolation is active.
    pub fn network_active(&self) -> bool {
        self.mutable
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .network_active
    }

    /// Hot-reload sandbox configuration.
    ///
    /// Updates enforcement level, settings, and config without re-creating
    /// the entire state. Proxy servers and violation store are preserved.
    /// Called when the user changes sandbox settings via `/sandbox` command.
    pub fn update_config(
        &self,
        enforcement: EnforcementLevel,
        settings: SandboxSettings,
        config: SandboxConfig,
    ) {
        let mut m = self
            .mutable
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        m.enforcement = enforcement;
        m.settings = settings;
        m.config = config;
        tracing::info!(
            enforcement = ?m.enforcement,
            "Sandbox configuration hot-reloaded"
        );
    }
}

#[cfg(test)]
#[path = "state.test.rs"]
mod tests;
