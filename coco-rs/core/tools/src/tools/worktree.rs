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

use coco_tool_runtime::DescriptionOptions;
use coco_tool_runtime::Tool;
use coco_tool_runtime::ToolError;
use coco_tool_runtime::ToolUseContext;
use coco_types::ToolId;
use coco_types::ToolInputSchema;
use coco_types::ToolName;
use coco_types::ToolResult;
use serde_json::Value;
use std::collections::HashMap;

// ── EnterWorktreeTool ──

pub struct EnterWorktreeTool;

#[async_trait::async_trait]
impl Tool for EnterWorktreeTool {
    fn id(&self) -> ToolId {
        ToolId::Builtin(ToolName::EnterWorktree)
    }
    fn name(&self) -> &str {
        ToolName::EnterWorktree.as_str()
    }
    fn description(&self, _: &Value, _options: &DescriptionOptions) -> String {
        "Create and enter a git worktree for isolated work on a branch.".into()
    }
    fn input_schema(&self) -> ToolInputSchema {
        let mut p = HashMap::new();
        p.insert(
            "branch".into(),
            serde_json::json!({"type": "string", "description": "Branch name for the worktree"}),
        );
        p.insert(
            "path".into(),
            serde_json::json!({"type": "string", "description": "Path for the worktree directory (optional, defaults to ../worktrees/<branch>)"}),
        );
        ToolInputSchema { properties: p }
    }

    async fn execute(
        &self,
        input: Value,
        _ctx: &ToolUseContext,
    ) -> Result<ToolResult<Value>, ToolError> {
        let branch = input
            .get("branch")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();

        if branch.is_empty() {
            return Err(ToolError::InvalidInput {
                message: "branch parameter is required".into(),
                error_code: None,
            });
        }

        let worktree_path = input
            .get("path")
            .and_then(|v| v.as_str())
            .map(std::string::ToString::to_string)
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
            data: serde_json::json!({
                "message": format!("Created worktree at '{worktree_path}' on branch '{branch}'"),
                "path": worktree_path,
                "branch": branch,
            }),
            new_messages: vec![],
            app_state_patch: None,
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

pub struct ExitWorktreeTool;

#[async_trait::async_trait]
impl Tool for ExitWorktreeTool {
    fn id(&self) -> ToolId {
        ToolId::Builtin(ToolName::ExitWorktree)
    }
    fn name(&self) -> &str {
        ToolName::ExitWorktree.as_str()
    }
    fn description(&self, _: &Value, _options: &DescriptionOptions) -> String {
        "Remove a git worktree and return to the previous working directory. \
         Restores the process CWD if it was inside the worktree being removed \
         and returns a `restoration` block describing the session state to \
         rebuild (hooks, system prompt, memory caches)."
            .into()
    }
    fn input_schema(&self) -> ToolInputSchema {
        let mut p = HashMap::new();
        p.insert(
            "path".into(),
            serde_json::json!({"type": "string", "description": "Path of the worktree to remove"}),
        );
        p.insert(
            "force".into(),
            serde_json::json!({"type": "boolean", "description": "Force removal even with uncommitted changes"}),
        );
        p.insert(
            "previous_cwd".into(),
            serde_json::json!({
                "type": "string",
                "description": "Absolute path to restore as the process cwd after the \
                               worktree is removed. If omitted, defaults to the parent \
                               directory of the worktree."
            }),
        );
        ToolInputSchema { properties: p }
    }

    async fn execute(
        &self,
        input: Value,
        _ctx: &ToolUseContext,
    ) -> Result<ToolResult<Value>, ToolError> {
        let path = input
            .get("path")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();

        if path.is_empty() {
            return Err(ToolError::InvalidInput {
                message: "path parameter is required".into(),
                error_code: None,
            });
        }

        let force = input
            .get("force")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);

        // Resolve the restoration target BEFORE we remove the worktree.
        // Three sources in priority order:
        //   1. Explicit `previous_cwd` parameter from the caller.
        //   2. The parent directory of the worktree path.
        //   3. Current process cwd — last-ditch fallback; if the process
        //      cwd is inside the worktree this will fail step 1 below
        //      and leave the caller in a dangling dir. Better than
        //      panicking, though.
        let explicit_prev = input
            .get("previous_cwd")
            .and_then(|v| v.as_str())
            .map(std::path::PathBuf::from);

        let worktree_path = std::path::PathBuf::from(path);
        let parent_fallback = worktree_path.parent().map(std::path::Path::to_path_buf);
        let restore_target = explicit_prev
            .or(parent_fallback)
            .or_else(|| std::env::current_dir().ok());

        let mut args = vec!["worktree", "remove"];
        if force {
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

        // Layers 2-6: report what the query-engine layer still needs to
        // restore. These are keys the caller can use to drive its own
        // cleanup hook — the tool itself can't touch them because they
        // live in a higher-layer state tree that's not accessible via
        // ToolUseContext.
        Ok(ToolResult {
            data: serde_json::json!({
                "message": format!("Removed worktree at '{path}'"),
                "path": path,
                "restoration": {
                    "cwd_target": restore_target.as_ref().and_then(|p| p.to_str()),
                    "cwd_restored": cwd_restored,
                    "cwd_restore_error": restore_error,
                    // Follow-up layers for the query-engine cleanup hook:
                    "pending_layers": [
                        "originalCwd",
                        "projectRoot",
                        "hooksSnapshot",
                        "systemPromptSections",
                        "memoryCaches",
                    ]
                }
            }),
            new_messages: vec![],
            app_state_patch: None,
        })
    }
}

#[cfg(test)]
#[path = "worktree.test.rs"]
mod tests;
