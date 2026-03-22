//! ExitWorktree tool for cleaning up git worktrees.

use super::prompts;
use crate::context::ToolContext;
use crate::error::Result;
use crate::tool::Tool;
use async_trait::async_trait;
use cocode_protocol::ConcurrencySafety;
use cocode_protocol::ToolOutput;
use serde_json::Value;
use std::path::PathBuf;

/// Tool for removing or keeping a git worktree.
pub struct ExitWorktreeTool;

impl ExitWorktreeTool {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ExitWorktreeTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for ExitWorktreeTool {
    fn name(&self) -> &str {
        cocode_protocol::ToolName::ExitWorktree.as_str()
    }

    fn description(&self) -> &str {
        prompts::EXIT_WORKTREE_DESCRIPTION
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "worktreePath": {
                    "type": "string",
                    "description": "Path to the worktree to exit"
                },
                "previousCwd": {
                    "type": "string",
                    "description": "Previous working directory to restore"
                },
                "action": {
                    "type": "string",
                    "enum": ["keep", "remove"],
                    "description": "Whether to keep or remove the worktree (default: remove)",
                    "default": "remove"
                },
                "delete_branch": {
                    "type": "boolean",
                    "description": "Also delete the worktree's branch when removing (default: false)",
                    "default": false
                }
            },
            "required": ["worktreePath"]
        })
    }

    fn concurrency_safety(&self) -> ConcurrencySafety {
        ConcurrencySafety::Unsafe
    }

    fn is_read_only(&self) -> bool {
        false
    }

    fn feature_gate(&self) -> Option<cocode_protocol::Feature> {
        Some(cocode_protocol::Feature::Worktree)
    }

    async fn execute(&self, input: Value, ctx: &mut ToolContext) -> Result<ToolOutput> {
        let worktree_path_str = super::input_helpers::require_str(&input, "worktreePath")?;
        let worktree_path = PathBuf::from(worktree_path_str);
        let action = input["action"].as_str().unwrap_or("remove");
        let delete_branch = super::input_helpers::bool_or(&input, "delete_branch", false);

        // Restore CWD to previous_cwd before worktree removal (Gap 9 fix)
        if let Some(prev_cwd) = input["previousCwd"].as_str() {
            let prev = PathBuf::from(prev_cwd);
            ctx.cwd = prev.clone();
            ctx.shell_executor.set_cwd(prev);
        }

        // Find the main repo root (not the worktree)
        let repo_root = find_main_repo_root(&ctx.cwd).await?;

        if action == "remove" {
            // Fire WorktreeRemove hook before removal
            if let Some(ref hooks) = ctx.hook_registry {
                let hook_ctx = cocode_hooks::HookContext::new(
                    cocode_hooks::HookEventType::WorktreeRemove,
                    ctx.session_id.clone(),
                    ctx.cwd.clone(),
                )
                .with_worktree_path(worktree_path_str.to_string());
                let _ = hooks.execute(&hook_ctx).await;
            }

            ctx.emit_progress(format!("Removing worktree at {worktree_path_str}"))
                .await;

            // Get branch name before removing
            let branch_name = if delete_branch {
                get_worktree_branch(&worktree_path).await
            } else {
                None
            };

            // Remove the worktree
            let output = tokio::process::Command::new("git")
                .current_dir(&repo_root)
                .args(["worktree", "remove", "--force"])
                .arg(&worktree_path)
                .output()
                .await
                .map_err(|e| {
                    crate::error::tool_error::ExecutionFailedSnafu {
                        message: format!("Failed to run git worktree remove: {e}"),
                    }
                    .build()
                })?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Ok(ToolOutput::error(format!(
                    "git worktree remove failed: {stderr}"
                )));
            }

            // Delete branch if requested
            if let Some(branch) = branch_name {
                let _ = tokio::process::Command::new("git")
                    .current_dir(&repo_root)
                    .args(["branch", "-D", &branch])
                    .output()
                    .await;
            }

            // Prune stale worktree entries
            let _ = tokio::process::Command::new("git")
                .current_dir(&repo_root)
                .args(["worktree", "prune"])
                .output()
                .await;

            Ok(ToolOutput::text(format!(
                "Worktree removed: {worktree_path_str}"
            )))
        } else {
            // Keep the worktree
            Ok(ToolOutput::text(format!(
                "Worktree kept at: {worktree_path_str}\nBranch preserved for future use."
            )))
        }
    }
}

async fn find_main_repo_root(cwd: &std::path::Path) -> Result<PathBuf> {
    let output = tokio::process::Command::new("git")
        .current_dir(cwd)
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .await
        .map_err(|e| {
            crate::error::tool_error::ExecutionFailedSnafu {
                message: format!("Failed to find repo root: {e}"),
            }
            .build()
        })?;

    let root = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok(PathBuf::from(root))
}

async fn get_worktree_branch(worktree_path: &std::path::Path) -> Option<String> {
    let output = tokio::process::Command::new("git")
        .current_dir(worktree_path)
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .output()
        .await
        .ok()?;

    if output.status.success() {
        let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if branch != "HEAD" { Some(branch) } else { None }
    } else {
        None
    }
}

#[cfg(test)]
#[path = "exit_worktree.test.rs"]
mod tests;
