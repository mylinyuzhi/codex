//! `apply_patch` — model-specific tool used by the gpt-5 family in lieu of
//! the `Edit` built-in. The model emits a unified-diff-style patch and the
//! runtime applies it. Visible only when
//! `ctx.tool_overrides.is_extra(ToolId::Builtin(ToolName::ApplyPatch))`.
//!
//! Backed by [`coco_apply_patch::apply_patch`] + [`coco_exec_server::LOCAL_FS`].

use async_trait::async_trait;
use coco_messages::ToolResult;
use coco_tool_runtime::DescriptionOptions;
use coco_tool_runtime::Tool;
use coco_tool_runtime::ToolResultContentPart;
use coco_tool_runtime::ToolUseContext;
use coco_tool_runtime::error::ToolError;
use coco_types::ToolCheckResult;
use coco_types::ToolId;
use coco_types::ToolName;
use coco_utils_absolute_path::AbsolutePathBuf;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;

/// Typed input for [`ApplyPatchTool`].
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct ApplyPatchInput {
    /// Patch body wrapped in `*** Begin Patch` / `*** End Patch`.
    pub patch: String,
}

/// Typed output — stdout / stderr emitted by `coco_apply_patch::apply_patch`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct ApplyPatchOutput {
    pub stdout: String,
    pub stderr: String,
}

pub struct ApplyPatchTool;

#[async_trait]
impl Tool for ApplyPatchTool {
    type Input = ApplyPatchInput;
    type Output = ApplyPatchOutput;

    fn id(&self) -> ToolId {
        ToolId::Builtin(ToolName::ApplyPatch)
    }

    fn name(&self) -> &str {
        ToolName::ApplyPatch.as_str()
    }

    /// Layer-2 gate: only models that explicitly add `apply_patch` as
    /// an extra tool (e.g. gpt-5) see this tool. Other models would
    /// call it accidentally if it were registered universally.
    fn is_enabled(&self, ctx: &ToolUseContext) -> bool {
        ctx.tool_overrides
            .is_extra(&ToolId::Builtin(ToolName::ApplyPatch))
    }

    fn description(&self, _input: &ApplyPatchInput, _options: &DescriptionOptions) -> String {
        "Apply a unified-diff-style patch to one or more files. The patch \
         body must follow the `*** Begin Patch` / `*** End Patch` envelope \
         emitted by gpt-5."
            .into()
    }

    fn is_read_only(&self, _input: &ApplyPatchInput) -> bool {
        false
    }

    async fn check_permissions(
        &self,
        input: &ApplyPatchInput,
        ctx: &ToolUseContext,
    ) -> ToolCheckResult {
        let Ok(cwd) = apply_patch_cwd(ctx) else {
            return ToolCheckResult::Passthrough;
        };
        let Ok(paths) = affected_paths_from_patch(&input.patch, &cwd) else {
            return ToolCheckResult::Passthrough;
        };
        if paths.is_empty() {
            return ToolCheckResult::Passthrough;
        }

        let cwd_str = cwd.as_path().to_string_lossy().to_string();
        let mut all_paths_to_check = Vec::new();
        for path in &paths {
            if let Some(message) = crate::check_write_root_fence(ctx, path.as_path()) {
                return ToolCheckResult::Deny { message };
            }
            let path_str = path.to_string_lossy();
            let paths_to_check =
                coco_permissions::filesystem::get_paths_for_permission_check(&path_str, &cwd_str);
            all_paths_to_check.extend(paths_to_check);
        }
        crate::tools::write_permissions::check_write_permission_for_paths(
            &all_paths_to_check,
            ctx,
            ToolName::ApplyPatch.as_str(),
            "apply a patch",
            cwd.as_path(),
        )
    }

    /// Render `{stdout, stderr}` by joining stdout + stderr with a
    /// newline (skip empty pieces). Same shape as a simplified Bash.
    fn render_for_model(&self, out: &ApplyPatchOutput) -> Vec<ToolResultContentPart> {
        let stdout = out.stdout.trim_end();
        let stderr = out.stderr.trim();
        let combined = [stdout, stderr]
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect::<Vec<&str>>()
            .join("\n");
        vec![ToolResultContentPart::Text {
            text: combined,
            provider_options: None,
        }]
    }

    async fn execute(
        &self,
        input: ApplyPatchInput,
        ctx: &ToolUseContext,
    ) -> Result<ToolResult<ApplyPatchOutput>, ToolError> {
        let patch = &input.patch;

        let cwd_path = ctx
            .cwd_override
            .clone()
            .or_else(|| std::env::current_dir().ok())
            .ok_or_else(|| ToolError::ExecutionFailed {
                message: "no working directory available for apply_patch".into(),
                source: None,
            })?;
        let cwd = AbsolutePathBuf::from_absolute_path(&cwd_path).map_err(|e| {
            ToolError::ExecutionFailed {
                message: format!("cwd `{}` is not absolute: {e}", cwd_path.display()),
                source: None,
            }
        })?;

        // Pre-flight: parse the patch to extract affected paths so we
        // can record file-history snapshots BEFORE the mutation and
        // notify the LSP AFTER. Errors here are not fatal —
        // `apply_patch` below will produce its own (more descriptive)
        // parse error in that case. Mirrors the per-tool track_file_edit
        // ordering used by Edit/Write.
        let affected_paths: Vec<std::path::PathBuf> = coco_apply_patch::parse_patch(patch)
            .map(|parsed| {
                parsed
                    .hunks
                    .iter()
                    .map(|hunk| hunk.resolve_path(&cwd).as_path().to_path_buf())
                    .collect()
            })
            .unwrap_or_default();
        for path in &affected_paths {
            crate::track_file_edit(ctx, path.as_path()).await;
        }

        let mut stdout: Vec<u8> = Vec::new();
        let mut stderr: Vec<u8> = Vec::new();
        let fs: &dyn coco_exec_server::ExecutorFileSystem = coco_exec_server::LOCAL_FS.as_ref();
        coco_apply_patch::apply_patch(patch, &cwd, &mut stdout, &mut stderr, fs)
            .await
            .map_err(|e| ToolError::ExecutionFailed {
                message: format!(
                    "{}{}",
                    String::from_utf8_lossy(&stderr),
                    if stderr.is_empty() {
                        e.to_string()
                    } else {
                        String::new()
                    },
                ),
                source: None,
            })?;

        // TS parity with Write/Edit — notify LSP of `didSave` per file
        // touched so diagnostics refresh. Best-effort, errors swallowed.
        for path in &affected_paths {
            ctx.lsp.notify_save(path.as_path()).await;
        }

        Ok(ToolResult::data(ApplyPatchOutput {
            stdout: String::from_utf8_lossy(&stdout).to_string(),
            stderr: String::from_utf8_lossy(&stderr).to_string(),
        }))
    }
}

fn apply_patch_cwd(ctx: &ToolUseContext) -> Result<AbsolutePathBuf, String> {
    let cwd_path = ctx
        .cwd_override
        .clone()
        .or_else(|| std::env::current_dir().ok())
        .ok_or_else(|| "no working directory available for apply_patch".to_string())?;
    AbsolutePathBuf::from_absolute_path(&cwd_path)
        .map_err(|e| format!("cwd `{}` is not absolute: {e}", cwd_path.display()))
}

fn affected_paths_from_patch(
    patch: &str,
    cwd: &AbsolutePathBuf,
) -> Result<Vec<std::path::PathBuf>, String> {
    coco_apply_patch::parse_patch(patch)
        .map(|parsed| {
            parsed
                .hunks
                .iter()
                .map(|hunk| hunk.resolve_path(cwd).as_path().to_path_buf())
                .collect()
        })
        .map_err(|e| e.to_string())
}

#[cfg(test)]
#[path = "apply_patch.test.rs"]
mod tests;
