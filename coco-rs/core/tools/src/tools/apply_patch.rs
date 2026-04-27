//! `apply_patch` — model-specific tool used by the gpt-5 family in lieu of
//! the `Edit` built-in. The model emits a unified-diff-style patch and the
//! runtime applies it. Visible only when
//! `ctx.tool_overrides.is_extra(ToolId::Builtin(ToolName::ApplyPatch))`.
//!
//! Backed by [`coco_apply_patch::apply_patch`] + [`coco_exec_server::LOCAL_FS`].

use std::collections::HashMap;

use async_trait::async_trait;
use coco_tool_runtime::DescriptionOptions;
use coco_tool_runtime::Tool;
use coco_tool_runtime::ToolUseContext;
use coco_tool_runtime::error::ToolError;
use coco_types::ToolId;
use coco_types::ToolInputSchema;
use coco_types::ToolName;
use coco_types::ToolResult;
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
