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
//! cache restoration.

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
use std::path::PathBuf;

// ── EnterWorktreeTool ──

/// Typed input for [`EnterWorktreeTool`].
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct EnterWorktreeInput {
    /// Optional descriptive name for the worktree.
    #[serde(default)]
    pub name: Option<String>,
}

/// Typed output for [`EnterWorktreeTool`].
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnterWorktreeOutput {
    pub worktree_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree_branch: Option<String>,
    pub message: String,
}

pub struct EnterWorktreeTool;

#[async_trait::async_trait]
impl Tool for EnterWorktreeTool {
    type Input = EnterWorktreeInput;
    coco_tool_runtime::impl_runtime_schema!(EnterWorktreeInput);
    type Output = EnterWorktreeOutput;

    fn to_auto_classifier_input(&self, input: &EnterWorktreeInput) -> Option<String> {
        Some(input.name.clone().unwrap_or_default())
    }

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
        "Create and enter a git worktree for isolated work.".into()
    }
    fn should_defer(&self) -> bool {
        true
    }
    fn search_hint(&self) -> Option<&str> {
        Some("create and enter a git worktree")
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
        ctx: &ToolUseContext,
    ) -> Result<ToolResult<EnterWorktreeOutput>, ToolError> {
        let app_state = ctx
            .app_state
            .as_ref()
            .ok_or_else(|| ToolError::InvalidInput {
                message: "Cannot enter a worktree because app state is unavailable".into(),
                error_code: None,
            })?;
        let slug = worktree_slug(input.name.as_deref());
        if slug.is_empty() {
            return Err(ToolError::InvalidInput {
                message: "name must include at least one ASCII letter or digit".into(),
                error_code: None,
            });
        }

        let branch = format!("{}{slug}", coco_types::AGENT_WORKTREE_BRANCH_PREFIX);
        let current_cwd = if let Some(session_cwd) = &ctx.session_cwd {
            session_cwd.read().await.clone()
        } else {
            std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
        };
        let worktree_path = current_cwd.join("..").join("worktrees").join(&slug);
        let worktree_path_string = worktree_path.to_string_lossy().to_string();

        let output = tokio::process::Command::new("git")
            .current_dir(&current_cwd)
            .args(["worktree", "add", "-b", &branch, &worktree_path_string])
            .output()
            .await
            .map_err(|e| ToolError::ExecutionFailed {
                message: format!("Failed to run git worktree add: {e}"),
                display_data: None,
                source: None,
            })?;

        if !output.status.success() {
            // Try without -b (branch may already exist)
            let output2 = tokio::process::Command::new("git")
                .current_dir(&current_cwd)
                .args(["worktree", "add", &worktree_path_string, &branch])
                .output()
                .await
                .map_err(|e| ToolError::ExecutionFailed {
                    message: format!("Failed to run git worktree add: {e}"),
                    display_data: None,
                    source: None,
                })?;

            if !output2.status.success() {
                let stderr = String::from_utf8_lossy(&output2.stderr);
                return Err(ToolError::ExecutionFailed {
                    message: format!("git worktree add failed: {stderr}"),
                    display_data: None,
                    source: None,
                });
            }
        }

        let abs_worktree = std::fs::canonicalize(&worktree_path).unwrap_or(worktree_path);
        let state = coco_types::ActiveWorktreeState {
            original_cwd: current_cwd,
            worktree_path: abs_worktree.clone(),
            worktree_branch: Some(branch.clone()),
        };
        {
            let mut guard = app_state.write().await;
            guard.active_worktree = Some(state.clone());
        }

        if let Err(e) = std::env::set_current_dir(&abs_worktree) {
            {
                let mut guard = app_state.write().await;
                guard.active_worktree = None;
            }
            let _ = tokio::process::Command::new("git")
                .current_dir(&state.original_cwd)
                .args(["worktree", "remove", "--force", &worktree_path_string])
                .output()
                .await;
            return Err(ToolError::ExecutionFailed {
                message: format!("Created worktree but failed to enter it: {e}"),
                display_data: None,
                source: None,
            });
        }
        if let Some(session_cwd) = &ctx.session_cwd {
            *session_cwd.write().await = abs_worktree.clone();
        }

        Ok(ToolResult {
            data: EnterWorktreeOutput {
                worktree_path: abs_worktree.display().to_string(),
                worktree_branch: Some(branch.clone()),
                message: format!(
                    "Created and entered worktree at '{}' on branch '{}'",
                    abs_worktree.display(),
                    branch
                ),
            },
            new_messages: vec![],
            app_state_patch: Some(Box::new(move |app_state| {
                app_state.active_worktree = Some(state);
            })),
            permission_updates: Vec::new(),
            display_data: None,
        })
    }
}

// ── ExitWorktreeTool ──
//
// TS: `tools/ExitWorktreeTool/ExitWorktreeTool.ts:29-145`. The active
// worktree target lives in session app state, so the model only chooses
// whether to keep or remove it. This tool restores the process/session cwd
// before optional removal so later shell commands do not inherit a removed
// directory.

/// Typed input for [`ExitWorktreeTool`].
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct ExitWorktreeInput {
    /// What to do with the active worktree.
    pub action: ExitWorktreeAction,
    /// Required `true` when the worktree has uncommitted files (or its git
    /// state can't be verified). Without it the tool refuses removal and
    /// lists the pending work, so the model can confirm with the user first —
    /// removal is otherwise permanent. Mirrors TS `discard_changes`.
    #[serde(default)]
    pub discard_changes: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExitWorktreeAction {
    #[default]
    Keep,
    Remove,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExitWorktreeOutput {
    pub worktree_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree_branch: Option<String>,
    pub message: String,
}

pub struct ExitWorktreeTool;

#[async_trait::async_trait]
impl Tool for ExitWorktreeTool {
    type Input = ExitWorktreeInput;
    coco_tool_runtime::impl_runtime_schema!(ExitWorktreeInput);
    type Output = ExitWorktreeOutput;

    fn to_auto_classifier_input(&self, input: &ExitWorktreeInput) -> Option<String> {
        // `discard_changes` permits removing a dirty worktree — that's the
        // security-relevant signal, so surface it alongside the action.
        Some(if input.discard_changes {
            format!("{:?} (discard_changes)", input.action)
        } else {
            format!("{:?}", input.action)
        })
    }

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
        "Exit the active git worktree, optionally removing it.".into()
    }
    fn should_defer(&self) -> bool {
        true
    }
    fn search_hint(&self) -> Option<&str> {
        Some("exit or remove an active git worktree")
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
        let state = ctx
            .app_state
            .as_ref()
            .ok_or_else(|| ToolError::InvalidInput {
                message: "No active worktree state is available in this session".into(),
                error_code: None,
            })?
            .read()
            .await
            .active_worktree
            .clone()
            .ok_or_else(|| ToolError::InvalidInput {
                message: "No active worktree to exit".into(),
                error_code: None,
            })?;

        let worktree_path = state.worktree_path.clone();
        let restore_target = state.original_cwd.clone();
        let path_display = worktree_path.display().to_string();

        // Safety gate (TS `ExitWorktreeTool.validateInput`): unless the caller
        // passed `discard_changes`, refuse to remove a worktree that has
        // uncommitted work — `git worktree remove --force` would destroy it
        // permanently. Fail-closed: if git state can't be verified, also refuse.
        if matches!(input.action, ExitWorktreeAction::Remove) && !input.discard_changes {
            let gate_path = worktree_path.clone();
            let summary = tokio::task::spawn_blocking(move || {
                coco_git::count_worktree_changes(&gate_path, None)
            })
            .await
            .map_err(|e| ToolError::ExecutionFailed {
                message: format!("worktree status check panicked: {e}"),
                display_data: None,
                source: None,
            })?;
            match summary {
                None => {
                    return Err(ToolError::InvalidInput {
                        message: format!(
                            "Could not verify the git state of the worktree at {path_display:?}. \
                             Refusing to remove without explicit confirmation. Re-invoke with \
                             discard_changes: true to proceed."
                        ),
                        error_code: None,
                    });
                }
                Some(summary) if summary.has_pending_work() => {
                    let files = summary.changed_files;
                    let noun = if files == 1 { "file" } else { "files" };
                    return Err(ToolError::InvalidInput {
                        message: format!(
                            "Worktree at {path_display:?} has {files} uncommitted {noun}. Removing will \
                             discard this work permanently. Confirm with the user, then re-invoke \
                             with discard_changes: true."
                        ),
                        error_code: None,
                    });
                }
                Some(_) => {} // clean worktree — safe to remove
            }
        }

        if let Err(e) = std::env::set_current_dir(&restore_target) {
            return Err(ToolError::ExecutionFailed {
                message: format!(
                    "Failed to restore cwd to '{}': {e}",
                    restore_target.display()
                ),
                display_data: None,
                source: None,
            });
        }
        if let Some(session_cwd) = &ctx.session_cwd {
            *session_cwd.write().await = restore_target.clone();
        }

        if matches!(input.action, ExitWorktreeAction::Remove) {
            let mut args = vec!["worktree", "remove"];
            if input.discard_changes {
                args.push("--force");
            }
            args.push(&path_display);

            let output = tokio::process::Command::new("git")
                .current_dir(&restore_target)
                .args(&args)
                .output()
                .await
                .map_err(|e| ToolError::ExecutionFailed {
                    message: format!("Failed to run git worktree remove: {e}"),
                    display_data: None,
                    source: None,
                })?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(ToolError::ExecutionFailed {
                    message: format!("git worktree remove failed: {stderr}"),
                    display_data: None,
                    source: None,
                });
            }
        }

        // Proactive LSP cleanup: shutdown the worktree-rooted servers
        // BEFORE process-cwd restoration. The `git worktree remove`
        // succeeded above, but stale `(server_id, worktree_root)` cache
        // entries would otherwise linger until session end. The lazy
        // path in `LspServerManager::get_client` (server.rs:206-229)
        // catches this when the next request happens to touch a file
        // under the removed root, but that next request may never come
        // in the session. Best-effort — adapter swallows errors.
        if matches!(input.action, ExitWorktreeAction::Remove) {
            ctx.lsp.shutdown_for_root(&worktree_path).await;
        }
        let message = match input.action {
            ExitWorktreeAction::Keep => {
                format!("Exited worktree at '{path_display}' and kept it on disk")
            }
            ExitWorktreeAction::Remove => {
                format!("Exited and removed worktree at '{path_display}'")
            }
        };

        Ok(ToolResult {
            data: ExitWorktreeOutput {
                worktree_path: path_display,
                worktree_branch: state.worktree_branch,
                message,
            },
            new_messages: vec![],
            app_state_patch: Some(Box::new(|app_state| {
                app_state.active_worktree = None;
            })),
            permission_updates: Vec::new(),
            display_data: None,
        })
    }
}

fn worktree_slug(name: Option<&str>) -> String {
    let source = name
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| uuid::Uuid::new_v4().simple().to_string()[..8].to_string());
    let mut out = String::new();
    let mut last_dash = false;
    for ch in source.chars() {
        let next = if ch.is_ascii_alphanumeric() {
            Some(ch.to_ascii_lowercase())
        } else if ch == '-' || ch == '_' || ch.is_whitespace() {
            Some('-')
        } else {
            None
        };
        let Some(next) = next else {
            continue;
        };
        if next == '-' {
            if !last_dash && !out.is_empty() {
                out.push(next);
                last_dash = true;
            }
        } else {
            out.push(next);
            last_dash = false;
        }
        if out.len() >= 48 {
            break;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    out
}

#[cfg(test)]
#[path = "worktree.test.rs"]
mod tests;
