//! Resolve `--resume` / `--continue` / `--fork-session` CLI flags
//! into a concrete `ResumePlan` (source session id, prior messages,
//! and the live session id the new turn should write under).
//!
//! TS parity: `loadInitialMessages()` in `entrypoints/cli.tsx` calls
//! `loadConversationForResume()` then `processResumedConversation()` —
//! same shape, different file split. Keeping the flag-resolution
//! logic in one place lets `main.rs`, `tui_runner.rs`, and
//! `sdk_server::sdk_runner` all reuse it without duplicating the
//! "id vs jsonl path vs --continue most-recent" rules.
//!
//! The resolver is filesystem-only; it never touches an `ApiClient`,
//! `SessionManager`, or runtime state. Callers thread the resulting
//! `ResumePlan` into either:
//! - `RunChatOptions::prior_messages` (headless path) +
//!   `ResumePlan::session_id` for the runtime config, or
//! - `runtime.history.lock().await = plan.prior_messages` before
//!   spawning the TUI driver.

use std::path::Path;
use std::path::PathBuf;

use anyhow::Result;
use coco_session::TranscriptStore;
use coco_session::recovery::RecoveredConversation;
use coco_session::recovery::can_resume_session;
use coco_session::recovery::fork_conversation;
use coco_session::recovery::load_conversation_for_resume;
use uuid::Uuid;

use crate::Cli;

/// Result of resolving the resume-related CLI flags.
#[derive(Debug)]
pub struct ResumePlan {
    /// The session id that the upcoming run should write transcript
    /// entries under. For `--resume` / `--continue` this is the
    /// source session id (writes append onto the existing JSONL).
    /// For `--fork-session` this is a fresh uuid (writes go into a
    /// new JSONL that begins with a copy of the source).
    pub session_id: String,
    /// Source session id we loaded messages from. Same as
    /// `session_id` for resume/continue; different for fork.
    pub source_session_id: String,
    /// Path to the source transcript JSONL.
    pub source_path: PathBuf,
    /// Path to the destination transcript JSONL (= source for
    /// resume/continue, fresh file for fork).
    pub destination_path: PathBuf,
    /// Pre-loaded messages from the source transcript.
    pub prior_messages: Vec<coco_messages::Message>,
    /// Aggregate metadata recovered from the source transcript.
    /// Callers surface `model` and token counts in their startup
    /// banner so the user sees what they're continuing.
    pub recovered: RecoveredConversation,
    /// `true` when `--fork-session` was set (the destination diverged).
    pub is_fork: bool,
}

/// Inspect the CLI flags and (when one of `--resume` / `--continue` /
/// `--fork-session` is set) load the conversation from disk.
///
/// Returns `Ok(None)` when none of the resume flags are set —
/// callers fall through to fresh-session bootstrap. Returns an error
/// when the requested source isn't on disk or the JSONL is unreadable.
///
/// Resolution rules (TS-aligned):
/// - `--resume <id|path>`: load the named session by id, or treat
///   the argument as a path when it ends in `.jsonl`.
/// - `--continue` / `--continue-session`: load the most recent
///   non-sidechain session in `sessions_dir`.
/// - `--fork-session`: requires `--resume <id>`; copies the source
///   JSONL into `<dest_session_id>.jsonl` (where `dest` is
///   `--session-id` if provided, else a fresh uuid).
pub fn resolve(cli: &Cli, sessions_dir: &Path) -> Result<Option<ResumePlan>> {
    let store = TranscriptStore::new(sessions_dir.to_path_buf());

    let (source_session_id, source_path): (String, PathBuf) =
        if let Some(arg) = cli.resume.as_deref() {
            resolve_source_arg(&store, sessions_dir, arg)?
        } else if cli.continue_session {
            match resolve_most_recent(&store)? {
                Some(s) => s,
                None => {
                    // No prior sessions to continue. Treat as a no-op
                    // rather than an error so `coco -c` on a clean
                    // install just starts a fresh chat. TS does the
                    // same — falls through to the new-session path.
                    return Ok(None);
                }
            }
        } else if cli.fork_session {
            // Fork without an explicit source: fork the most-recent.
            // TS allows `--fork-session` standalone with the same
            // implicit-most-recent behavior.
            match resolve_most_recent(&store)? {
                Some(s) => s,
                None => {
                    anyhow::bail!("--fork-session requires an existing session to copy from");
                }
            }
        } else {
            return Ok(None);
        };

    if !can_resume_session(&source_path) {
        anyhow::bail!(
            "transcript at {} is empty or unreadable; nothing to resume",
            source_path.display(),
        );
    }
    let recovered = load_conversation_for_resume(&source_path)
        .map_err(|e| anyhow::anyhow!("failed to load transcript {}: {e}", source_path.display()))?;

    let prior_messages = recovered.messages.clone();

    if cli.fork_session {
        let dest_id = cli
            .session_id
            .clone()
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        let dest_path = store.transcript_path(&dest_id);
        fork_conversation(&source_path, &dest_path).map_err(|e| {
            anyhow::anyhow!(
                "fork copy {} → {} failed: {e}",
                source_path.display(),
                dest_path.display(),
            )
        })?;
        return Ok(Some(ResumePlan {
            session_id: dest_id,
            source_session_id,
            source_path,
            destination_path: dest_path,
            prior_messages,
            recovered,
            is_fork: true,
        }));
    }

    Ok(Some(ResumePlan {
        session_id: source_session_id.clone(),
        source_session_id,
        source_path: source_path.clone(),
        destination_path: source_path,
        prior_messages,
        recovered,
        is_fork: false,
    }))
}

/// Resolve `--resume <arg>` — accepts either a bare session id or a
/// `.jsonl` path. Returns `(session_id, transcript_path)`.
fn resolve_source_arg(
    store: &TranscriptStore,
    sessions_dir: &Path,
    arg: &str,
) -> Result<(String, PathBuf)> {
    if arg.ends_with(".jsonl") {
        let path = PathBuf::from(arg);
        let abs = if path.is_absolute() {
            path
        } else {
            // Relative .jsonl path is rooted in sessions_dir per
            // TS's `loadMessagesFromJsonlPath` resolution rule.
            sessions_dir.join(&path)
        };
        let id = abs
            .file_stem()
            .and_then(|s| s.to_str())
            .map(str::to_string)
            .unwrap_or_default();
        return Ok((id, abs));
    }
    let path = store.transcript_path(arg);
    if !path.exists() {
        anyhow::bail!("no session found for id {arg}; expected {}", path.display(),);
    }
    Ok((arg.to_string(), path))
}

/// Pick the newest non-sidechain session by transcript mtime.
fn resolve_most_recent(store: &TranscriptStore) -> Result<Option<(String, PathBuf)>> {
    let mut sessions = store
        .list_main_sessions()
        .map_err(|e| anyhow::anyhow!("listing sessions failed: {e}"))?;
    if sessions.is_empty() {
        return Ok(None);
    }
    // `list_main_sessions` sorts newest-first, so the first one is
    // the one we want.
    let latest = sessions.remove(0);
    if latest.session_id.is_empty() {
        return Ok(None);
    }
    let path = store.transcript_path(&latest.session_id);
    Ok(Some((latest.session_id, path)))
}

#[cfg(test)]
#[path = "resume_resolver.test.rs"]
mod tests;
