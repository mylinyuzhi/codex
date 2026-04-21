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
    /// Cached proxy environment variables (computed once at `activate_network()`).
    /// Avoids re-allocating 18+ strings on every command execution.
    cached_proxy_env: HashMap<String, String>,
    /// Extra paths to bind-mount into the sandbox (e.g., bridge UDS sockets).
    bridge_bind_paths: Vec<std::path::PathBuf>,
}

/// Pre-computed snapshot for per-command sandbox decisions.
///
/// Produced by `SandboxState::command_snapshot()` with a single `RwLock` read,
/// then consumed by the shell executor without further lock acquisitions.
pub struct CommandSandboxSnapshot {
    /// Whether the command should be wrapped with platform enforcement.
    pub should_wrap: bool,
    /// Whether the managed network proxy is active.
    pub network_active: bool,
    /// Cached proxy environment variables (empty if proxy is not active).
    pub proxy_env: HashMap<String, String>,
    /// Full sandbox config (only populated when `should_wrap` is true).
    pub config: Option<SandboxConfig>,
    /// Whether network is allowed.
    pub allow_network: bool,
    /// Current enforcement level (for tracing).
    pub enforcement: EnforcementLevel,
}

/// Runtime sandbox state, shared via `Arc` across the system.
///
/// Immutable fields (platform, violations) live directly on the struct.
/// Mutable fields (enforcement, settings, config) are behind a `RwLock`
/// to support hot-reload when the user changes sandbox settings mid-session.
pub struct SandboxState {
    /// Whether platform enforcement is active (kernel-level).
    platform_active: bool,
    /// External sandbox mode: environment is already sandboxed (Docker, CI).
    /// Platform wrapping (bwrap/Seatbelt) is skipped, but env vars, proxy
    /// filtering, and violation tracking still apply.
    external_sandbox: bool,
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
            .field("external_sandbox", &self.external_sandbox)
            .field("network_active", &m.network_active)
            .field("proxy_ports", &m.proxy_ports)
            .finish()
    }
}

impl SandboxState {
    /// Shared construction logic for all `SandboxState` variants.
    fn build(
        platform_active: bool,
        external_sandbox: bool,
        enforcement: EnforcementLevel,
        settings: SandboxSettings,
        config: SandboxConfig,
        platform: Box<dyn SandboxPlatform>,
    ) -> Self {
        let mut store = ViolationStore::new();
        if !settings.ignore_violations.is_empty() {
            store.set_ignore_patterns(settings.ignore_violations.clone());
        }

        Self {
            platform_active,
            external_sandbox,
            violations: Arc::new(Mutex::new(store)),
            mutable: RwLock::new(MutableConfig {
                enforcement,
                network_active: false,
                proxy_ports: None,
                settings,
                config,
                cached_proxy_env: HashMap::new(),
                bridge_bind_paths: Vec::new(),
            }),
            platform,
            session_tag: generate_session_tag(),
        }
    }

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
        Self::build(
            platform_active,
            /*external_sandbox*/ false,
            enforcement,
            settings,
            config,
            platform,
        )
    }

    /// Create a sandbox state for external sandbox mode (Docker, CI).
    ///
    /// Platform wrapping is skipped since the environment is already sandboxed,
    /// but env vars, proxy filtering, and violation tracking still apply.
    pub fn external(
        enforcement: EnforcementLevel,
        settings: SandboxSettings,
        config: SandboxConfig,
    ) -> Self {
        Self::build(
            /*platform_active*/ false,
            /*external_sandbox*/ true,
            enforcement,
            settings,
            config,
            crate::platform::create_platform(),
        )
    }

    /// Create a disabled (no-op) sandbox state.
    pub fn disabled() -> Self {
        Self::build(
            /*platform_active*/ false,
            /*external_sandbox*/ false,
            EnforcementLevel::Disabled,
            SandboxSettings::default(),
            SandboxConfig::default(),
            crate::platform::create_platform(),
        )
    }

    /// Whether sandbox enforcement is active.
    pub fn is_active(&self) -> bool {
        let m = self
            .mutable
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        m.enforcement != EnforcementLevel::Disabled
            && (self.platform_active || self.external_sandbox)
    }

    /// Whether this is an external sandbox (Docker, CI).
    pub fn is_external_sandbox(&self) -> bool {
        self.external_sandbox
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
            && (self.platform_active || self.external_sandbox)
            && m.settings.is_sandboxed(command, bypass)
    }

    /// Whether auto-allow for bash commands is enabled.
    pub fn auto_allow_enabled(&self) -> bool {
        let m = self
            .mutable
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        m.enforcement != EnforcementLevel::Disabled
            && (self.platform_active || self.external_sandbox)
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
        // Merge runtime state into the config snapshot (not persisted to disk).
        config.proxy_active = m.network_active;
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
    ///
    /// Returns the cached env vars computed at `activate_network()` time.
    /// Returns an empty map if the proxy is not active.
    pub fn proxy_env_vars(&self) -> HashMap<String, String> {
        let m = self
            .mutable
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        m.cached_proxy_env.clone()
    }

    /// Build proxy environment variables for the given ports.
    ///
    /// Aligned with Claude Code's `f21()` function: sets HTTP, SOCKS, FTP,
    /// Docker, gcloud, gRPC proxy vars and GIT_SSH_COMMAND for SOCKS tunneling.
    fn build_proxy_env_vars(ports: ProxyPorts) -> HashMap<String, String> {
        let http_proxy = format!("http://localhost:{}", ports.http_port);
        // socks5h: DNS resolution via proxy (prevents DNS leaks in sandbox)
        let socks_proxy = format!("socks5h://localhost:{}", ports.socks_port);
        let no_proxy = "localhost,127.0.0.1,::1,*.local,.local,\
                         169.254.0.0/16,10.0.0.0/8,172.16.0.0/12,192.168.0.0/16";

        let mut vars = HashMap::from([
            ("HTTP_PROXY".to_string(), http_proxy.clone()),
            ("HTTPS_PROXY".to_string(), http_proxy.clone()),
            ("http_proxy".to_string(), http_proxy.clone()),
            ("https_proxy".to_string(), http_proxy.clone()),
            ("ALL_PROXY".to_string(), socks_proxy.clone()),
            ("all_proxy".to_string(), socks_proxy.clone()),
            ("NO_PROXY".to_string(), no_proxy.to_string()),
            ("no_proxy".to_string(), no_proxy.to_string()),
            // FTP proxy
            ("FTP_PROXY".to_string(), socks_proxy.clone()),
            ("ftp_proxy".to_string(), socks_proxy.clone()),
            (
                "RSYNC_PROXY".to_string(),
                format!("localhost:{}", ports.socks_port),
            ),
            // Docker proxy
            ("DOCKER_HTTP_PROXY".to_string(), http_proxy.clone()),
            ("DOCKER_HTTPS_PROXY".to_string(), http_proxy),
            // gRPC proxy
            ("GRPC_PROXY".to_string(), socks_proxy.clone()),
            ("grpc_proxy".to_string(), socks_proxy),
            // gcloud SDK proxy
            ("CLOUDSDK_PROXY_TYPE".to_string(), "https".to_string()),
            (
                "CLOUDSDK_PROXY_ADDRESS".to_string(),
                "localhost".to_string(),
            ),
            (
                "CLOUDSDK_PROXY_PORT".to_string(),
                ports.http_port.to_string(),
            ),
        ]);

        // GIT_SSH_COMMAND: route git SSH via SOCKS proxy (macOS/Linux)
        vars.insert(
            "GIT_SSH_COMMAND".to_string(),
            format!(
                "ssh -o ProxyCommand='nc -X 5 -x localhost:{} %h %p'",
                ports.socks_port
            ),
        );

        vars
    }

    /// Set network isolation as active with the given proxy ports.
    ///
    /// Pre-computes and caches the proxy environment variables so they
    /// don't need to be re-allocated on every command execution.
    pub fn activate_network(&self, ports: ProxyPorts) {
        let mut m = self
            .mutable
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        m.network_active = true;
        m.proxy_ports = Some(ports);
        m.cached_proxy_env = Self::build_proxy_env_vars(ports);
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

    /// Take a single-lock snapshot of all fields needed for per-command sandbox
    /// decisions. Avoids 5+ separate `RwLock` acquisitions on the hot path.
    pub fn command_snapshot(&self, command: &str, bypass: SandboxBypass) -> CommandSandboxSnapshot {
        let m = self
            .mutable
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        let has_enforcement_backing = self.platform_active || self.external_sandbox;
        let active = m.enforcement != EnforcementLevel::Disabled && has_enforcement_backing;
        let should_wrap = active && m.settings.is_sandboxed(command, bypass);

        CommandSandboxSnapshot {
            should_wrap,
            network_active: m.network_active,
            proxy_env: if m.network_active {
                m.cached_proxy_env.clone()
            } else {
                HashMap::new()
            },
            config: if should_wrap {
                let mut config = m.config.clone();
                config.proxy_active = m.network_active;
                if !m.bridge_bind_paths.is_empty() {
                    config
                        .extra_bind_ro
                        .extend(m.bridge_bind_paths.iter().cloned());
                }
                Some(config)
            } else {
                None
            },
            allow_network: m.config.allow_network,
            enforcement: m.enforcement,
        }
    }

    /// JSON description of filesystem restrictions for the system prompt.
    ///
    /// Matches Claude Code's E9z format so the model sees structured
    /// restriction data: `{"read":{...},"write":{...}}`.
    pub fn describe_filesystem(&self) -> String {
        let cfg = self.config();

        // Collect all read-denied paths (denied_paths + denied_read_paths).
        let mut deny_read: Vec<String> = cfg
            .denied_paths
            .iter()
            .chain(cfg.denied_read_paths.iter())
            .map(|p| p.display().to_string())
            .collect();
        deny_read.dedup();

        // Collect all write-denied paths (denied_paths + deny_write_paths).
        let mut deny_write: Vec<String> = cfg
            .denied_paths
            .iter()
            .chain(cfg.deny_write_paths.iter())
            .map(|p| p.display().to_string())
            .collect();
        deny_write.dedup();

        // Collect writable roots with read-only subpath annotations.
        let allow_write: Vec<serde_json::Value> = cfg
            .writable_roots
            .iter()
            .map(|r| {
                if r.readonly_subpaths.is_empty() {
                    serde_json::Value::String(r.path.display().to_string())
                } else {
                    serde_json::json!({
                        "path": r.path.display().to_string(),
                        "readOnlySubpaths": r.readonly_subpaths,
                    })
                }
            })
            .collect();

        let desc = serde_json::json!({
            "read": { "denyOnly": deny_read },
            "write": { "allowOnly": allow_write, "denyOnly": deny_write },
        });
        desc.to_string()
    }

    /// JSON description of network restrictions for the system prompt.
    ///
    /// Matches Claude Code's E9z format: `{"allowedHosts":[...],"deniedHosts":[...]}`.
    pub fn describe_network(&self) -> String {
        let settings = self.settings();
        let net = &settings.network;

        if !self.network_active() {
            if !self.config().allow_network {
                return r#"{"status":"blocked"}"#.to_string();
            }
            return r#"{"status":"allowed","filtering":"none"}"#.to_string();
        }

        let desc = serde_json::json!({
            "allowedHosts": net.allowed_domains,
            "deniedHosts": net.denied_domains,
            "unixSockets": if net.allow_all_unix_sockets { "all" } else { "filtered" },
        });
        desc.to_string()
    }

    /// Add a writable root to the sandbox configuration.
    ///
    /// Used when the agent enters a new worktree or workspace that needs
    /// write access. The writable root uses default read-only subpath
    /// protections (.git, .coco, .agents).
    ///
    /// Uses `std::sync::RwLock::write()` (blocking). The lock is held briefly
    /// so this is safe to call from async contexts, but avoid calling while
    /// another task holds the read lock in a tight loop.
    pub fn add_writable_root(&self, path: std::path::PathBuf) {
        let mut m = self
            .mutable
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // Avoid duplicates
        if !m.config.writable_roots.iter().any(|r| r.path == path) {
            tracing::info!(path = %path.display(), "Adding writable root to sandbox config");
            m.config
                .writable_roots
                .push(crate::config::WritableRoot::new(path));
        }
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
