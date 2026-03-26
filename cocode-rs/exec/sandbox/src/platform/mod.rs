//! Platform-specific sandbox implementations.
//!
//! Provides a `SandboxPlatform` trait with platform-gated implementations
//! for macOS (Seatbelt) and Linux (bubblewrap + seccomp).

use crate::config::SandboxConfig;
use crate::error::Result;

#[cfg(target_os = "macos")]
pub mod macos;

#[cfg(target_os = "linux")]
pub mod linux;

/// Platform-specific sandbox enforcement.
///
/// Implementations wrap child process commands with OS-level restrictions
/// (Seatbelt on macOS, bubblewrap + seccomp on Linux).
pub trait SandboxPlatform: Send + Sync {
    /// Returns true if this sandbox implementation is available on the current OS.
    fn available(&self) -> bool;

    /// Wraps a command to run under sandbox enforcement.
    ///
    /// Modifies the command to execute within the platform-specific sandbox,
    /// applying filesystem, network, and process isolation according to the config.
    fn wrap_command(&self, config: &SandboxConfig, cmd: &mut tokio::process::Command)
    -> Result<()>;
}

/// Returns the platform-appropriate sandbox implementation.
#[cfg(target_os = "macos")]
pub fn create_platform() -> Box<dyn SandboxPlatform> {
    Box::new(macos::MacOsSandbox)
}

/// Returns the platform-appropriate sandbox implementation.
#[cfg(target_os = "linux")]
pub fn create_platform() -> Box<dyn SandboxPlatform> {
    Box::new(linux::LinuxSandbox)
}

/// Returns a no-op sandbox for unsupported platforms.
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub fn create_platform() -> Box<dyn SandboxPlatform> {
    Box::new(NoopSandbox)
}

/// No-op sandbox for unsupported platforms.
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
struct NoopSandbox;

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
impl SandboxPlatform for NoopSandbox {
    fn available(&self) -> bool {
        false
    }

    fn wrap_command(
        &self,
        _config: &SandboxConfig,
        _cmd: &mut tokio::process::Command,
    ) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
#[path = "mod.test.rs"]
mod tests;
