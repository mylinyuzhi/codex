//! WSL (Windows Subsystem for Linux) detection.
//!
//! When cocode runs inside WSL, IDE lockfile discovery also scans
//! the Windows host user's `.claude/ide/` directory.

use std::sync::OnceLock;

/// Cached WSL detection result.
static IS_WSL: OnceLock<bool> = OnceLock::new();

/// Check if the current environment is WSL.
///
/// Result is cached after the first call via `OnceLock`.
pub fn is_wsl() -> bool {
    *IS_WSL.get_or_init(detect_wsl)
}

fn detect_wsl() -> bool {
    #[cfg(target_os = "linux")]
    {
        std::fs::read_to_string("/proc/version")
            .ok()
            .is_some_and(|v| {
                let lower = v.to_lowercase();
                lower.contains("microsoft") || lower.contains("wsl")
            })
    }

    #[cfg(not(target_os = "linux"))]
    {
        false
    }
}

#[cfg(test)]
#[path = "wsl.test.rs"]
mod tests;
