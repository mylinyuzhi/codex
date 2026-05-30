//! EnterWorktree + ExitWorktree tools — git worktree isolation.
//!
//! TS:
//! - `src/tools/EnterWorktreeTool/EnterWorktreeTool.ts`
//! - `src/tools/ExitWorktreeTool/ExitWorktreeTool.ts`
//!
//! Worktree tools let the model (usually an agent) work in an isolated
//! git worktree — create a branch, modify files there, and tear it
//! down. The process CWD restoration is handled here; higher layers
//! (query engine) are responsible for hooks, system prompt, and memory
//! cache restoration reported in the result payload.

use coco_messages::ToolResult;
use coco_tool_runtime::DescriptionOptions;
use coco_tool_runtime::Tool;
use coco_tool_runtime::ToolError;
use coco_tool_runtime::ToolResultContentPart;
use coco_tool_runtime::ToolUseContext;
use coco_types::ToolId;
use coco_types::ToolName;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;

// ── EnterWorktreeTool ──

/// Typed input for [`EnterWorktreeTool`].
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct EnterWorktreeInput {
    /// Branch name for the worktree
    #[serde(default)]
    pub branch: String,
    /// Path for the worktree directory (optional, defaults to
    /// `../worktrees/<branch>`)
    #[serde(default)]
    pub path: Option<String>,
}

/// Typed output for [`EnterWorktreeTool`].
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EnterWorktreeOutput {
    pub message: String,
    pub path: String,
    pub branch: String,
}

pub struct EnterWorktreeTool;

#[async_trait::async_trait]
impl Tool for EnterWorktreeTool {
    type Input = EnterWorktreeInput;
    coco_tool_runtime::impl_runtime_schema!(EnterWorktreeInput);
    type Output = EnterWorktreeOutput;

    fn id(&self) -> ToolId {
        ToolId::Builtin(ToolName::EnterWorktree)
    }
    fn name(&self) -> &str {
        ToolName::EnterWorktree.as_str()
    }
    fn is_enabled(&self, ctx: &coco_tool_runtime::ToolUseContext) -> bool {
        ctx.features.enabled(coco_types::Feature::Worktree)
    }
    fn description(&self, _input: &EnterWorktreeInput, _options: &DescriptionOptions) -> String {
        "Create and enter a git worktree for isolated work on a branch.".into()
    }
    fn should_defer(&self) -> bool {
        true
    }
    fn search_hint(&self) -> Option<&str> {
        Some("create and enter a git worktree on a branch")
    }

    /// Emit the prebuilt `message` field as plain text — the model
    /// doesn't need the path/branch fields separately, they're already
    /// in the message. Skips JSON envelope overhead.
    fn render_for_model(&self, out: &EnterWorktreeOutput) -> Vec<ToolResultContentPart> {
        vec![ToolResultContentPart::Text {
            text: out.message.clone(),
            provider_options: None,
        }]
    }

    async fn execute(
        &self,
        input: EnterWorktreeInput,
        _ctx: &ToolUseContext,
    ) -> Result<ToolResult<EnterWorktreeOutput>, ToolError> {
        let branch = input.branch.trim();

        if branch.is_empty() {
            return Err(ToolError::InvalidInput {
                message: "branch parameter is required".into(),
                error_code: None,
            });
        }

        let worktree_path = input
            .path
            .clone()
            .unwrap_or_else(|| format!("../worktrees/{branch}"));

        let output = tokio::process::Command::new("git")
            .args(["worktree", "add", "-b", branch, &worktree_path])
            .output()
            .await
            .map_err(|e| ToolError::ExecutionFailed {
                message: format!("Failed to run git worktree add: {e}"),
                source: None,
            })?;

        if !output.status.success() {
            // Try without -b (branch may already exist)
            let output2 = tokio::process::Command::new("git")
                .args(["worktree", "add", &worktree_path, branch])
                .output()
                .await
                .map_err(|e| ToolError::ExecutionFailed {
                    message: format!("Failed to run git worktree add: {e}"),
                    source: None,
                })?;

            if !output2.status.success() {
                let stderr = String::from_utf8_lossy(&output2.stderr);
                return Err(ToolError::ExecutionFailed {
                    message: format!("git worktree add failed: {stderr}"),
                    source: None,
                });
            }
        }

        Ok(ToolResult {
            data: EnterWorktreeOutput {
                message: format!("Created worktree at '{worktree_path}' on branch '{branch}'"),
                path: worktree_path,
                branch: branch.to_string(),
            },
            new_messages: vec![],
            app_state_patch: None,
            permission_updates: Vec::new(),
        })
    }
}

// ── ExitWorktreeTool ──
//
// TS: `tools/ExitWorktreeTool/ExitWorktreeTool.ts:29-145`. The TS tool
// tears down a worktree AND restores the session's prior state in a
// specific order:
//
//   1. `setCwd(originalCwd)` — process-level current directory
//   2. `setOriginalCwd(originalCwd)` — session's recorded origin cwd
//   3. `setProjectRoot(previousProjectRoot)` — conditional
//   4. `restoreHooksSnapshot()` — revert hook overrides made in worktree
//   5. `restoreSystemPromptSections()` — rebuild system prompt
//   6. `clearMemoryCaches()` — drop claude.md / memory caches
//
// Steps 3–6 live at the query-engine/app layer in coco-rs (they require
// cross-crate access to the system prompt builder, hook registry, etc.)
// and are out of scope for this tool alone. Step 1 (`set_current_dir`)
// is the critical one — without it, the process cwd is left dangling
// inside a just-removed directory and the next Bash call fails with
// ENOENT. This implementation handles step 1 inline and emits the
// other restoration targets in the result payload so the query engine
// can apply them in its SessionEnd-like cleanup hook.

/// Typed input for [`ExitWorktreeTool`].
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct ExitWorktreeInput {
    /// Path of the worktree to remove
    #[serde(default)]
    pub path: String,
    /// Force removal even with uncommitted changes
    #[serde(default)]
    pub force: bool,
    /// Absolute path to restore as the process cwd after the worktree
    /// is removed. If omitted, defaults to the parent directory of the
    /// worktree.
    #[serde(default)]
    pub previous_cwd: Option<String>,
}

/// Restoration metadata for the query-engine cleanup hook. Layers
/// 2–6 (from `ExitWorktreeTool.ts:126-145`) can't be performed by the
/// tool itself; this struct reports what the upper layer still needs
/// to restore.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExitWorktreeRestoration {
    /// Process-cwd target the tool resolved (explicit `previous_cwd`,
    /// then the worktree's parent dir, then `current_dir`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd_target: Option<String>,
    /// True iff `std::env::set_current_dir(cwd_target)` succeeded.
    #[serde(default)]
    pub cwd_restored: bool,
    /// Set when set_current_dir failed; the upper-layer cleanup hook
    /// may want to surface this to the user.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd_restore_error: Option<String>,
    /// Layers the upper-layer cleanup hook still needs to handle.
    /// Currently always the same five labels; kept as a typed Vec so
    /// future layers can be added without breaking the wire shape.
    #[serde(default)]
    pub pending_layers: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExitWorktreeOutput {
    pub message: String,
    pub path: String,
    pub restoration: ExitWorktreeRestoration,
}

pub struct ExitWorktreeTool;

#[async_trait::async_trait]
impl Tool for ExitWorktreeTool {
    type Input = ExitWorktreeInput;
    coco_tool_runtime::impl_runtime_schema!(ExitWorktreeInput);
    type Output = ExitWorktreeOutput;

    fn id(&self) -> ToolId {
        ToolId::Builtin(ToolName::ExitWorktree)
    }
    fn name(&self) -> &str {
        ToolName::ExitWorktree.as_str()
    }
    fn is_enabled(&self, ctx: &coco_tool_runtime::ToolUseContext) -> bool {
        ctx.features.enabled(coco_types::Feature::Worktree)
    }
    fn description(&self, _input: &ExitWorktreeInput, _options: &DescriptionOptions) -> String {
        "Remove a git worktree and return to the previous working directory. \
         Restores the process CWD if it was inside the worktree being removed \
         and returns a `restoration` block describing the session state to \
         rebuild (hooks, system prompt, memory caches)."
            .into()
    }
    fn should_defer(&self) -> bool {
        true
    }
    fn search_hint(&self) -> Option<&str> {
        Some("remove a git worktree and restore previous cwd")
    }

    /// Emit the prebuilt `message` field; restoration metadata is for
    /// the query-engine cleanup hook, not the model.
    fn render_for_model(&self, out: &ExitWorktreeOutput) -> Vec<ToolResultContentPart> {
        vec![ToolResultContentPart::Text {
            text: out.message.clone(),
            provider_options: None,
        }]
    }

    async fn execute(
        &self,
        input: ExitWorktreeInput,
        ctx: &ToolUseContext,
    ) -> Result<ToolResult<ExitWorktreeOutput>, ToolError> {
        let path = input.path.trim();

        if path.is_empty() {
            return Err(ToolError::InvalidInput {
                message: "path parameter is required".into(),
                error_code: None,
            });
        }

        // Resolve the restoration target BEFORE we remove the worktree.
        // Three sources in priority order:
        //   1. Explicit `previous_cwd` parameter from the caller.
        //   2. The parent directory of the worktree path.
        //   3. Current process cwd — last-ditch fallback; if the process
        //      cwd is inside the worktree this will fail step 1 below
        //      and leave the caller in a dangling dir. Better than
        //      panicking, though.
        let explicit_prev = input.previous_cwd.as_deref().map(std::path::PathBuf::from);

        let worktree_path = std::path::PathBuf::from(path);
        let parent_fallback = worktree_path.parent().map(std::path::Path::to_path_buf);
        let restore_target = explicit_prev
            .or(parent_fallback)
            .or_else(|| std::env::current_dir().ok());

        let mut args = vec!["worktree", "remove"];
        if input.force {
            args.push("--force");
        }
        args.push(path);

        let output = tokio::process::Command::new("git")
            .args(&args)
            .output()
            .await
            .map_err(|e| ToolError::ExecutionFailed {
                message: format!("Failed to run git worktree remove: {e}"),
                source: None,
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(ToolError::ExecutionFailed {
                message: format!("git worktree remove failed: {stderr}"),
                source: None,
            });
        }

        // Proactive LSP cleanup: shutdown the worktree-rooted servers
        // BEFORE process-cwd restoration. The `git worktree remove`
        // succeeded above, but stale `(server_id, worktree_root)` cache
        // entries would otherwise linger until session end. The lazy
        // path in `LspServerManager::get_client` (server.rs:206-229)
        // catches this when the next request happens to touch a file
        // under the removed root, but that next request may never come
        // in the session. Best-effort — adapter swallows errors.
        let abs_worktree =
            std::fs::canonicalize(&worktree_path).unwrap_or_else(|_| worktree_path.clone());
        ctx.lsp.shutdown_for_root(&abs_worktree).await;

        // Layer 1: restore process CWD. Critical: without this, if the
        // process cwd was inside the just-removed worktree, every
        // subsequent relative path operation fails with ENOENT.
        //
        // TS `ExitWorktreeTool.ts:126` calls `setCwd(originalCwd)`
        // **unconditionally** — it always moves the cwd to the
        // restoration target, whether or not the current cwd was inside
        // the worktree. This is the predictable behavior: callers can
        // rely on "after ExitWorktree, cwd is the restoration target".
        let mut cwd_restored = false;
        let mut restore_error: Option<String> = None;
        if let Some(target) = restore_target.as_ref() {
            match std::env::set_current_dir(target) {
                Ok(()) => cwd_restored = true,
                Err(e) => restore_error = Some(e.to_string()),
            }
        }

        let cwd_target = restore_target
            .as_ref()
            .and_then(|p| p.to_str().map(String::from));

        // Layers 2-6: report what the query-engine layer still needs to
        // restore. These are keys the caller can use to drive its own
        // cleanup hook — the tool itself can't touch them because they
        // live in a higher-layer state tree that's not accessible via
        // ToolUseContext.
        Ok(ToolResult {
            data: ExitWorktreeOutput {
                message: format!("Removed worktree at '{path}'"),
                path: path.to_string(),
                restoration: ExitWorktreeRestoration {
                    cwd_target,
                    cwd_restored,
                    cwd_restore_error: restore_error,
                    pending_layers: vec![
                        "originalCwd".into(),
                        "projectRoot".into(),
                        "hooksSnapshot".into(),
                        "systemPromptSections".into(),
                        "memoryCaches".into(),
                    ],
                },
            },
            new_messages: vec![],
            app_state_patch: None,
            permission_updates: Vec::new(),
        })
    }
}

#[cfg(test)]
#[path = "worktree.test.rs"]
mod tests;
