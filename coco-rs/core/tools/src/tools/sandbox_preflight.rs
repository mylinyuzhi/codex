//! Sandbox pre-flight helpers for file-I/O tools.
//!
//! Read/Write/Edit invoke these before any `std::fs` call so the
//! [`coco_sandbox::PermissionChecker`] gets a chance to deny *before* the
//! tool issues the syscall. This is a UX/SDK feature, not a security
//! boundary: the platform sandboxes (bwrap, Seatbelt) catch the same
//! violations at the kernel level — pre-flight just lets SDK consumers
//! intercept via the approval bridge, and gives users a structured deny
//! reason instead of an opaque `EACCES` from the OS.
//!
//! Closes the deferral documented in `docs/coco-rs/audit-gaps.md` Round 12:
//! "PermissionChecker — type + bridge are correct, no production consumer
//! wires it into Read/Write/Edit pre-flight."

use std::path::Path;

use coco_tool_runtime::ToolError;
use coco_tool_runtime::ToolUseContext;

/// Pre-flight a path operation against the live sandbox checker.
///
/// Returns `Ok(())` when:
/// - No `sandbox_state` is installed (test harness, headless without
///   sandbox, FullAccess mode), OR
/// - The sandbox is `Disabled` enforcement, OR
/// - The path passes [`coco_sandbox::PermissionChecker::check_path`].
///
/// Returns [`ToolError::PermissionDenied`] otherwise. Tools should call
/// this *after* input parsing but *before* the first I/O syscall so the
/// model gets a structured deny rather than an opaque OS error.
pub(crate) fn preflight_path(
    ctx: &ToolUseContext,
    path: &Path,
    write: bool,
) -> Result<(), ToolError> {
    let Some(state) = ctx.sandbox_state.as_ref() else {
        return Ok(());
    };
    let checker = state.permission_checker();
    checker
        .check_path(path, write)
        .map_err(|e| ToolError::PermissionDenied {
            message: e.to_string(),
        })
}

#[cfg(test)]
#[path = "sandbox_preflight.test.rs"]
mod tests;
