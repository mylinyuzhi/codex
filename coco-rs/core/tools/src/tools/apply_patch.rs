//! `apply_patch` — model-specific tool used by the gpt-5 family in lieu of
//! the `Edit` built-in. The model emits a unified-diff-style patch and the
//! runtime applies it. Visible only when
//! `ctx.tool_overrides.is_extra(ToolId::Builtin(ToolName::ApplyPatch))`.
//!
//! Backed by [`coco_apply_patch::apply_patch`] + [`coco_exec_server::LOCAL_FS`].

use std::collections::HashMap;

use async_trait::async_trait;
use coco_messages::ToolResult;
use coco_tool_runtime::DescriptionOptions;
use coco_tool_runtime::Tool;
use coco_tool_runtime::ToolResultContentPart;
use coco_tool_runtime::ToolUseContext;
use coco_tool_runtime::error::ToolError;
use coco_types::ToolId;
use coco_types::ToolInputSchema;
use coco_types::ToolName;
use coco_utils_absolute_path::AbsolutePathBuf;
use serde_json::Value;

pub struct ApplyPatchTool;

#[async_trait]
impl Tool for ApplyPatchTool {
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

    fn description(&self, _: &Value, _: &DescriptionOptions) -> String {
        "Apply a unified-diff-style patch to one or more files. The patch \
         body must follow the `*** Begin Patch` / `*** End Patch` envelope \
         emitted by gpt-5."
            .into()
    }

    fn input_schema(&self) -> ToolInputSchema {
        let mut p = HashMap::new();
        p.insert(
            "patch".into(),
            serde_json::json!({
                "type": "string",
                "description": "Patch body wrapped in `*** Begin Patch` / `*** End Patch`."
            }),
        );
        ToolInputSchema { properties: p }
    }

    fn is_read_only(&self, _: &Value) -> bool {
        false
    }

    /// Render `{stdout, stderr}` by joining stdout + stderr with a
    /// newline (skip empty pieces). Same shape as a simplified Bash.
    fn render_for_model(&self, data: &Value) -> Vec<ToolResultContentPart> {
        let stdout = data
            .get("stdout")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim_end()
            .to_string();
        let stderr = data
            .get("stderr")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        let combined = [stdout.as_str(), stderr.as_str()]
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
        input: Value,
        ctx: &ToolUseContext,
    ) -> Result<ToolResult<Value>, ToolError> {
        let patch =
            input
                .get("patch")
                .and_then(|v| v.as_str())
                .ok_or_else(|| ToolError::InvalidInput {
                    message: "`patch` is required and must be a string".into(),
                    error_code: None,
                })?;

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

        let out = String::from_utf8_lossy(&stdout).to_string();
        let err = String::from_utf8_lossy(&stderr).to_string();
        Ok(ToolResult::data(serde_json::json!({
            "stdout": out,
            "stderr": err,
        })))
    }
}

#[cfg(test)]
#[path = "apply_patch.test.rs"]
mod tests;
