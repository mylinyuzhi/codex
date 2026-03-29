//! Sandbox enforcement for the cocode agent.
//!
//! This crate provides:
//! - Sandbox configuration (enforcement levels, writable roots, network access)
//! - Permission checking for file and network operations
//! - Platform-specific sandbox enforcement (Seatbelt on macOS, bubblewrap on Linux)
//! - Bootstrap lifecycle with 4-gate enable check
//! - Dependency detection for platform-specific binaries
//! - Violation tracking via ring buffer store
//! - Runtime state management shared across the system via `Arc`

pub mod bootstrap;
pub mod checker;
pub mod config;
pub mod deps;
pub mod error;
pub mod monitor;
pub mod platform;
pub mod proxy;
pub mod seccomp;
pub mod state;
pub mod violation;

pub use bootstrap::EnableCheckResult;
pub use bootstrap::check_enable_gates;
pub use checker::PermissionChecker;
pub use config::EnforcementLevel;
pub use config::FilesystemConfig;
pub use config::IgnoreViolationsConfig;
pub use config::MitmProxyConfig;
pub use config::NetworkConfig;
pub use config::NetworkMode;
pub use config::SandboxBypass;
pub use config::SandboxConfig;
pub use config::SandboxSettings;
pub use config::WritableRoot;
pub use error::SandboxError;
pub use monitor::ViolationMonitor;
pub use platform::SandboxPlatform;
pub use state::CommandSandboxSnapshot;
pub use state::ProxyPorts;
pub use state::SandboxState;
pub use violation::ViolationStore;

// Re-export bridge types for Linux network namespace proxy bridging
pub use proxy::BridgeManager;
pub use proxy::BridgePorts;
