//! Cwd resolution + post-exec snap-back shared by `BashTool` and
//! `PowerShellTool`.
//!
//! The post-exec sequence per command:
//!
//! ```text
//!  spawn at getCwd()                 — current session cwd (no pre-reset)
//!  → exec the shell command
//!  → setCwd(new_cwd from pwd file)   — session cwd = where the command ended
//!  → resetCwdIfOutsideProject()      — if cwd drifted outside, snap back
//!                                      to originalCwd, return true
//!  → if reset fired, append "Shell cwd was reset to <orig>" to THIS
//!    command's stderr (annotation lands on the offending call)
//! ```
//!
//! The sequence maps to three calls:
//!
//! ```text
//!  before exec
//!  ───────────
//!     resolve_spawn_cwd(ctx)              → PathBuf
//!  after exec
//!  ──────────
//!     finalize_cwd_post_exec(ctx, new_cwd) → Option<reset_message>
//!     annotate_stderr_with_reset(&mut stderr, reset_message)
//! ```
//!
//! All helpers are no-ops when `ctx.cwd_override` is set (worktree-isolated
//! subagents fence cwd via the override and don't share state with the
//! parent session), and on the legacy / test path where
//! `ctx.session_cwd` / `ctx.original_cwd` are absent.

use std::path::Path;
use std::path::PathBuf;

use coco_tool_runtime::ToolUseContext;
use unicode_normalization::UnicodeNormalization;

/// Resolve the cwd to spawn the shell in.
///
/// Priority:
///
/// 1. `ctx.cwd_override` — set by `AgentTool` for worktree-isolated subagents.
/// 2. `ctx.session_cwd` — live session cwd updated after each command.
/// 3. `std::env::current_dir()` — fallback for tests / SDK paths without session state.
/// 4. `/tmp` — last-resort floor when even `current_dir()` fails.
pub async fn resolve_spawn_cwd(ctx: &ToolUseContext) -> PathBuf {
    if let Some(over) = ctx.cwd_override.clone() {
        over
    } else if let Some(session_cwd) = &ctx.session_cwd {
        session_cwd.read().await.clone()
    } else {
        std::env::current_dir()
            .ok()
            .unwrap_or_else(|| PathBuf::from("/tmp"))
    }
}

/// Run the post-exec cwd sequence: update `session_cwd` then snap back if
/// the cwd drifted outside the project.
///
/// 1. If `new_cwd` is `Some`, write it into `ctx.session_cwd`.
/// 2. Then check the just-updated session cwd against
///    `ctx.original_cwd ∪ permission_context.additional_dirs`. If
///    `shell_config.maintain_project_working_dir` is true OR the cwd has
///    drifted outside the allowed set, snap `session_cwd` back to
///    `original_cwd`.
/// 3. Return the user-visible `"Shell cwd was reset to …"` message
///    when a non-maintain reset fired, so the caller can append it via
///    [`annotate_stderr_with_reset`]. Returns `None` for the
///    silent-maintain reset (every command resets in maintain mode —
///    no annotation emitted).
///
/// Skips entirely when `cwd_override` is set or session/original cwd is
/// unwired.
pub async fn finalize_cwd_post_exec(
    ctx: &ToolUseContext,
    new_cwd: Option<PathBuf>,
) -> Option<String> {
    if ctx.cwd_override.is_some() {
        return None;
    }
    let session_cwd = ctx.session_cwd.as_ref()?;

    if let Some(new) = new_cwd {
        *session_cwd.write().await = new;
    }

    let original = ctx.original_cwd.as_ref()?;
    let cwd_now = session_cwd.read().await.clone();

    let should_maintain = ctx.shell_config.maintain_project_working_dir;
    let needs_reset = should_maintain
        || (cwd_now != *original
            && !path_in_allowed_working_path(&cwd_now, original, &ctx.permission_context));
    if !needs_reset {
        return None;
    }

    tracing::info!(
        "Shell cwd '{}' is outside allowed working directories, resetting to '{}'",
        cwd_now.display(),
        original.display()
    );
    *session_cwd.write().await = original.clone();

    if should_maintain {
        // Every command resets in maintain mode — no stderr annotation.
        return None;
    }
    Some(format!("Shell cwd was reset to {}", original.display()))
}

/// Append `reset_message` (when present) to `stderr`.
///
/// ```ts
/// `${stderr.trim()}\n${msg}`
/// ```
///
/// Always trims existing stderr; always inserts a newline. Empty
/// stderr → leading newline before the message.
pub fn annotate_stderr_with_reset(stderr: &mut String, reset_message: Option<String>) {
    let Some(msg) = reset_message else {
        return;
    };
    let trimmed = stderr.trim();
    *stderr = format!("{trimmed}\n{msg}");
}

/// `path` is allowed iff it lives inside `original_cwd` or any
/// `permission_context.additional_dirs` entry (any-of).
fn path_in_allowed_working_path(
    path: &Path,
    original_cwd: &Path,
    perm_ctx: &coco_types::ToolPermissionContext,
) -> bool {
    if path_in_working_path(path, original_cwd) {
        return true;
    }
    for key in perm_ctx.additional_dirs.keys() {
        if path_in_working_path(path, Path::new(key)) {
            return true;
        }
    }
    false
}

/// Both paths are normalized (NFC + macOS `/private` rewrites +
/// lowercase) before the prefix check. Same path or strict descendant
/// → inside; sibling or disjoint root → outside.
fn path_in_working_path(path: &Path, working_path: &Path) -> bool {
    let p = normalize_for_compare(path);
    let w = normalize_for_compare(working_path);
    if p == w {
        return true;
    }
    // Strict descendant: append the OS separator so `/foo` does NOT
    // claim `/foobar` as inside.
    let mut w_with_sep = w;
    if !w_with_sep.ends_with(std::path::MAIN_SEPARATOR) {
        w_with_sep.push(std::path::MAIN_SEPARATOR);
    }
    p.starts_with(&w_with_sep)
}

/// NFC normalize + macOS `/private/{var,tmp}` rewrites + lowercase.
///
/// We don't realpath / canonicalize here — the cwd from `pwd -P` is
/// already symlink-resolved by the kernel. Symlink-chain walking would
/// only matter for `additional_dirs` entries that are themselves
/// symlinks — out of scope here.
fn normalize_for_compare(path: &Path) -> String {
    let s: String = path.to_string_lossy().nfc().collect();
    let rewritten = if s == "/private/var" {
        "/var".to_string()
    } else if s == "/private/tmp" {
        "/tmp".to_string()
    } else if let Some(rest) = s.strip_prefix("/private/var/") {
        format!("/var/{rest}")
    } else if let Some(rest) = s.strip_prefix("/private/tmp/") {
        format!("/tmp/{rest}")
    } else {
        s
    };
    rewritten.to_lowercase()
}
