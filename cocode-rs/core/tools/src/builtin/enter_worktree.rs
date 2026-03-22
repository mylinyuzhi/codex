//! EnterWorktree tool for creating isolated git worktrees.

use super::prompts;
use crate::context::ToolContext;
use crate::error::Result;
use crate::tool::Tool;
use async_trait::async_trait;
use cocode_protocol::ConcurrencySafety;
use cocode_protocol::ToolOutput;
use serde_json::Value;
use std::path::PathBuf;

/// Tool for manually creating git worktrees for isolated agent workspaces.
pub struct EnterWorktreeTool;

impl EnterWorktreeTool {
    pub fn new() -> Self {
        Self
    }
}

impl Default for EnterWorktreeTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for EnterWorktreeTool {
    fn name(&self) -> &str {
        cocode_protocol::ToolName::EnterWorktree.as_str()
    }

    fn description(&self) -> &str {
        prompts::ENTER_WORKTREE_DESCRIPTION
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "branch": {
                    "type": "string",
                    "description": "Branch name for the worktree (auto-generated if omitted)"
                },
                "path": {
                    "type": "string",
                    "description": "Custom path for the worktree (auto-generated if omitted)"
                },
                "base": {
                    "type": "string",
                    "description": "Base branch/commit to create the worktree from (default: HEAD)"
                },
                "sparsePaths": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Paths for sparse checkout (checkout only these paths)"
                }
            }
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
        // Must be in a git repo
        let cwd = ctx.cwd.clone();
        if !cocode_git::is_inside_git_repo(&cwd) {
            return Ok(ToolOutput::error(
                "Not inside a git repository. EnterWorktree requires a git repo.",
            ));
        }

        // Generate branch name using agent/task-{timestamp} convention (CC alignment)
        let branch = input["branch"]
            .as_str()
            .map(String::from)
            .unwrap_or_else(|| {
                let timestamp = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                format!("agent/task-{timestamp}")
            });

        let worktree_path = input["path"]
            .as_str()
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                // Default: sibling directory ../worktrees/<branch>
                let parent = cwd.parent().unwrap_or(&cwd);
                parent.join("worktrees").join(&branch)
            });

        let base = input["base"].as_str().unwrap_or("HEAD");

        ctx.emit_progress(format!("Creating worktree at {}", worktree_path.display()))
            .await;

        // Create parent directory
        if let Some(parent) = worktree_path.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(|e| {
                crate::error::tool_error::ExecutionFailedSnafu {
                    message: format!("Failed to create parent directory: {e}"),
                }
                .build()
            })?;
        }

        // Run git worktree add
        let output = tokio::process::Command::new("git")
            .current_dir(&cwd)
            .args(["worktree", "add", "-b", &branch])
            .arg(&worktree_path)
            .arg(base)
            .output()
            .await
            .map_err(|e| {
                crate::error::tool_error::ExecutionFailedSnafu {
                    message: format!("Failed to run git worktree add: {e}"),
                }
                .build()
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Ok(ToolOutput::error(format!(
                "git worktree add failed: {stderr}"
            )));
        }

        // Handle sparse checkout if requested
        if let Some(sparse_paths) = input["sparsePaths"].as_array() {
            let paths: Vec<&str> = sparse_paths.iter().filter_map(|v| v.as_str()).collect();
            if !paths.is_empty() {
                // Enable sparse checkout
                let _ = tokio::process::Command::new("git")
                    .current_dir(&worktree_path)
                    .args(["sparse-checkout", "init", "--cone"])
                    .output()
                    .await;

                let mut cmd = tokio::process::Command::new("git");
                cmd.current_dir(&worktree_path)
                    .args(["sparse-checkout", "set"]);
                for p in &paths {
                    cmd.arg(p);
                }
                let _ = cmd.output().await;
            }
        }

        // Store previous CWD for ExitWorktree
        let previous_cwd = cwd.display().to_string();

        // Update the agent's working directory to the worktree (Gap 9 fix)
        ctx.cwd = worktree_path.clone();
        ctx.shell_executor.set_cwd(worktree_path.clone());

        // Fire WorktreeCreate hook
        if let Some(ref hooks) = ctx.hook_registry {
            let hook_ctx = cocode_hooks::HookContext::new(
                cocode_hooks::HookEventType::WorktreeCreate,
                ctx.session_id.clone(),
                ctx.cwd.clone(),
            )
            .with_worktree_path(worktree_path.display().to_string())
            .with_worktree_branch(&branch);
            let _ = hooks.execute(&hook_ctx).await;
        }

        let result = serde_json::json!({
            "worktreePath": worktree_path.display().to_string(),
            "branch": branch,
            "previousCwd": previous_cwd,
            "sparseCheckout": input.get("sparsePaths").cloned().unwrap_or(Value::Null),
        });

        Ok(ToolOutput::text(
            serde_json::to_string_pretty(&result).unwrap_or_default(),
        ))
    }
}

#[cfg(test)]
#[path = "enter_worktree.test.rs"]
mod tests;
