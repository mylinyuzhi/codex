//! End-of-batch nested-memory attachment drain.
//!
//! TS: `utils/attachments.ts:2167-2194` (`getNestedMemoryAttachments`).
//!
//! When a tool batch finishes, this drains
//! [`coco_tool_runtime::ToolUseContext::nested_memory_attachment_triggers`]
//! (the Set populated by `Read`/`NotebookEdit`/etc. via
//! [`coco_tools::track_nested_memory_attachment`]), runs each triggered
//! file through [`coco_context::traverse_for_file`], and stores the
//! resulting [`NestedMemoryInfo`] entries on
//! [`QueryEngine::pending_nested_memory`] for the next reminder build
//! to consume.
//!
//! Two ends of the pipeline used to be unwired:
//! 1. The trigger Set was a write-only black hole — populated by tools,
//!    never drained. This module provides the drain.
//! 2. [`crate::reminder_adapters::MemoryAdapter::nested_memories`]
//!    intentionally returns `Vec::new()` because nested-CLAUDE.md
//!    discovery is engine-driven (file-read triggers), not state-driven
//!    (memory store recall). The pending slot is the engine-side
//!    delivery channel.
//!
//! Session-level dedup uses
//! [`QueryEngine::loaded_nested_memory_paths`] — once a memory file is
//! injected this session, subsequent reads of files in the same subtree
//! won't re-inject it. Mirrors TS `loadedNestedMemoryPathsRef`
//! (`REPL.tsx:1964-1967`).

use std::path::PathBuf;

use coco_system_reminder::generators::memory::NestedMemoryInfo;
use coco_tool_runtime::ToolUseContext;

use crate::engine::QueryEngine;

impl QueryEngine {
    /// Drain `ctx.nested_memory_attachment_triggers`, traverse each
    /// triggered file's CWD→file slice, and append the resulting
    /// memory entries to `self.pending_nested_memory`.
    ///
    /// Idempotent across calls within a turn — the trigger Set is
    /// cleared in place, so a second drain at the same site is a no-op.
    /// Safe to call when the Set is empty (early return).
    ///
    /// `ctx.cwd_override` wins over the process cwd when set, matching
    /// the worktree-isolated subagent contract.
    pub(crate) async fn drain_nested_memory_triggers(&self, ctx: &ToolUseContext) {
        let triggered_paths: Vec<PathBuf> = {
            let mut triggers = ctx.nested_memory_attachment_triggers.write().await;
            if triggers.is_empty() {
                return;
            }
            // `drain` empties the Set in place — TS clear-after-process
            // semantics. The local Vec is cheap (Strings move, no copy).
            triggers.drain().map(PathBuf::from).collect()
        };

        let cwd = ctx
            .cwd_override
            .clone()
            .or_else(|| std::env::current_dir().ok());
        let Some(cwd) = cwd else {
            // No cwd anchor → traversal can't compute the CWD↔file
            // slice. Skip silently; trigger paths are already drained
            // so the next batch starts clean.
            return;
        };

        // Session-level dedup. Held across the whole drain so
        // sibling triggers (e.g. reading two files in the same subtree)
        // share the loaded set within one batch.
        let mut loaded = self.loaded_nested_memory_paths.lock().await;
        let mut new_entries: Vec<NestedMemoryInfo> = Vec::new();
        let mut newly_loaded: Vec<(String, String, coco_context::MemoryFileSource)> = Vec::new();
        for path in triggered_paths {
            let trigger_path = path.display().to_string();
            let entries = coco_context::traverse_for_file(&path, &cwd, &mut loaded);
            for entry in entries {
                newly_loaded.push((
                    entry.path.display().to_string(),
                    trigger_path.clone(),
                    entry.source,
                ));
                new_entries.push(NestedMemoryInfo {
                    path: entry.path.display().to_string(),
                    content: entry.content,
                });
            }
        }
        drop(loaded);

        // Fire `InstructionsLoaded` for each newly-loaded memory file.
        // TS: `executeInstructionsLoadedHooks` invoked per-file in
        // `claudemd.ts` after each load. Reason `nested_traversal`
        // matches the lazy traversal path here; the eager pass at
        // session start fires with `session_start`.
        if let Some(registry) = self.hooks.as_ref() {
            let ctx = self.orchestration_ctx();
            if !ctx.disable_all_hooks {
                for (loaded_path, trigger_path, source) in newly_loaded {
                    let memory_type = match source {
                        coco_context::MemoryFileSource::UserGlobal => {
                            coco_hooks::orchestration::MemoryType::User
                        }
                        coco_context::MemoryFileSource::Local => {
                            coco_hooks::orchestration::MemoryType::Local
                        }
                        coco_context::MemoryFileSource::ProjectConfig
                        | coco_context::MemoryFileSource::Project => {
                            coco_hooks::orchestration::MemoryType::Project
                        }
                    };
                    if let Err(e) = coco_hooks::orchestration::execute_instructions_loaded(
                        registry,
                        &ctx,
                        &loaded_path,
                        memory_type,
                        coco_hooks::orchestration::InstructionsLoadReason::NestedTraversal,
                        /*globs*/ None,
                        Some(trigger_path.as_str()),
                        /*parent_file_path*/ None,
                    )
                    .await
                    {
                        tracing::warn!(error = %e, path = %loaded_path, "InstructionsLoaded hook failed");
                    }
                }
            }
        }

        if new_entries.is_empty() {
            return;
        }
        let mut pending = self.pending_nested_memory.lock().await;
        pending.extend(new_entries);
    }

    /// Take and clear the engine-side pending nested-memory slot.
    ///
    /// Called once per turn from
    /// [`crate::engine_turn_reminders`] right before building
    /// `TurnReminderInput`. Returning `Vec::new()` (the common case
    /// when no triggers fired this turn) is a no-op for the generator,
    /// which short-circuits on empty input.
    pub(crate) async fn take_pending_nested_memory(&self) -> Vec<NestedMemoryInfo> {
        let mut pending = self.pending_nested_memory.lock().await;
        std::mem::take(&mut *pending)
    }

    /// Reset the session-level dedup set. Wired to `/clear` /
    /// conversation-reset paths so a fresh conversation re-injects
    /// memory files even if their content is unchanged. Mirrors TS
    /// `loadedNestedMemoryPathsRef.current.clear()` in REPL's
    /// `clearConversation`.
    #[allow(dead_code)] // wired by /clear paths added in a follow-up
    pub(crate) async fn clear_loaded_nested_memory_paths(&self) {
        self.loaded_nested_memory_paths.lock().await.clear();
    }
}

#[cfg(test)]
#[path = "engine_attachments.test.rs"]
mod tests;
