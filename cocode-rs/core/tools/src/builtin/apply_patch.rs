//! Apply patch tool for batch file modifications.
//!
//! Supports both JSON function mode and freeform (Lark grammar) mode.
//! For OpenAI models (especially GPT-5), this can replace the Edit tool
//! with a more powerful batch-editing capability.

use super::prompts;
use crate::ToolDefinition;
use crate::context::FileReadState;
use crate::context::ToolContext;
use crate::error::Result;
use crate::error::tool_error;
use crate::tool::Tool;
use async_trait::async_trait;
use cocode_apply_patch::ApplyPatchFileChange;
use cocode_apply_patch::MaybeApplyPatchVerified;
use cocode_apply_patch::apply_patch as execute_patch;
use cocode_apply_patch::maybe_parse_apply_patch_verified;
use cocode_plan_mode::is_safe_file;
use cocode_protocol::ConcurrencySafety;
use cocode_protocol::ContextModifier;
use cocode_protocol::ToolOutput;
use serde_json::Value;

/// Tool for applying multi-file patches.
///
/// This tool allows batch modifications to multiple files using a unified
/// diff-like format. It supports:
/// - Adding new files
/// - Deleting existing files
/// - Updating file contents with context-aware patches
/// - Moving/renaming files
///
/// The handler auto-detects input format (JSON object vs raw string).
/// Which tool **definition** is sent to a model is decided by
/// `select_tools_for_model()` based on `ModelInfo.apply_patch_tool_type`.
#[derive(Default)]
pub struct ApplyPatchTool;

impl ApplyPatchTool {
    pub fn new() -> Self {
        Self
    }

    /// Get the Function variant tool definition (JSON schema with "input" field).
    pub fn function_definition() -> ToolDefinition {
        ToolDefinition::full(
            "apply_patch",
            prompts::APPLY_PATCH_DESCRIPTION,
            serde_json::json!({
                "type": "object",
                "properties": {
                    "input": {
                        "type": "string",
                        "description": "The entire contents of the apply_patch command"
                    }
                },
                "required": ["input"]
            }),
        )
    }

    /// Get the Freeform variant tool definition (custom tool with Lark grammar).
    pub fn freeform_definition() -> ToolDefinition {
        let lark_grammar = include_str!("tool_apply_patch.lark");
        ToolDefinition::custom(
            "apply_patch",
            prompts::APPLY_PATCH_FREEFORM_DESCRIPTION,
            serde_json::json!({
                "type": "grammar",
                "syntax": "lark",
                "definition": lark_grammar
            }),
        )
    }
}

#[async_trait]
impl Tool for ApplyPatchTool {
    fn name(&self) -> &str {
        "apply_patch"
    }

    fn description(&self) -> &str {
        prompts::APPLY_PATCH_DESCRIPTION
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "input": {
                    "type": "string",
                    "description": "The entire contents of the apply_patch command"
                }
            },
            "required": ["input"]
        })
    }

    fn concurrency_safety(&self) -> ConcurrencySafety {
        ConcurrencySafety::Unsafe
    }

    fn is_read_only(&self) -> bool {
        false
    }

    #[allow(clippy::unwrap_used)]
    async fn execute(&self, input: Value, ctx: &mut ToolContext) -> Result<ToolOutput> {
        // TODO(sandbox): Current implementation executes patches directly in-process.
        //
        // codex-rs's apply_patch uses subprocess execution (unlike read_file/write_file/smart_edit):
        // 1. assess_patch_safety() determines if approval is needed
        // 2. SafetyCheck::Reject → return error directly, no execution
        // 3. SafetyCheck::AutoApprove/AskUser → DelegateToExec → ApplyPatchRuntime
        // 4. ApplyPatchRuntime spawns subprocess: codex --codex-run-as-apply-patch "<patch>"
        // 5. Subprocess can be wrapped in sandbox to restrict filesystem access
        //
        // When cocode-rs needs sandbox support, implement:
        // 1. Add InternalApplyPatchInvocation enum (Output vs DelegateToExec)
        // 2. Add assess_patch_safety() safety check
        // 3. Add ApplyPatchRuntime (build_command_spec)
        // 4. Connect arg0 dispatch (exists: cocode-rs/exec/arg0/src/lib.rs)
        // 5. Add user approval flow with caching
        //
        // Reference: codex-rs/core/src/tools/handlers/apply_patch.rs
        //            codex-rs/core/src/tools/runtimes/apply_patch.rs

        // 1. Extract patch content: auto-detect JSON object vs string input
        let patch_input = if input.is_string() {
            // Freeform mode: direct string input
            input.as_str().unwrap().to_string()
        } else {
            // Function mode: JSON object with "input" field
            input["input"]
                .as_str()
                .ok_or_else(|| {
                    tool_error::InvalidInputSnafu {
                        message: "input field must be a string",
                    }
                    .build()
                })?
                .to_string()
        };

        // 2. Parse and verify the patch
        let argv = vec!["apply_patch".to_string(), patch_input.clone()];
        let cwd = ctx.cwd.clone();

        match maybe_parse_apply_patch_verified(&argv, &cwd) {
            MaybeApplyPatchVerified::Body(action) => {
                // 3. Plan mode check: only allow modifications to plan file
                if ctx.is_plan_mode {
                    for path in action.changes().keys() {
                        if !is_safe_file(path, ctx.plan_file_path.as_deref()) {
                            return Err(tool_error::ExecutionFailedSnafu {
                                message: format!(
                                    "Plan mode: cannot modify '{}'. Only the plan file can be modified.",
                                    path.display()
                                ),
                            }
                            .build());
                        }
                    }
                }

                // 4. Execute the patch
                let mut stdout = Vec::new();
                let mut stderr = Vec::new();

                match execute_patch(&patch_input, &mut stdout, &mut stderr) {
                    Ok(()) => {
                        // 5. Track modifications and update read state
                        let mut result_modifiers = Vec::new();

                        for (path, change) in action.changes() {
                            ctx.record_file_modified(path).await;

                            // Update read state for files that now have content
                            match change {
                                ApplyPatchFileChange::Add { content }
                                | ApplyPatchFileChange::Update {
                                    new_content: content,
                                    ..
                                } => {
                                    let mtime = tokio::fs::metadata(path)
                                        .await
                                        .ok()
                                        .and_then(|m| m.modified().ok());
                                    ctx.record_file_read_with_state(
                                        path,
                                        FileReadState::complete(content.clone(), mtime),
                                    )
                                    .await;

                                    // Add context modifier for the updated content
                                    result_modifiers.push(ContextModifier::FileRead {
                                        path: path.clone(),
                                        content: content.clone(),
                                    });
                                }
                                ApplyPatchFileChange::Delete { .. } => {
                                    // File was deleted, no content to track
                                }
                            }
                        }

                        let output_text = String::from_utf8_lossy(&stdout).to_string();
                        let mut result = ToolOutput::text(output_text);
                        result.modifiers = result_modifiers;

                        Ok(result)
                    }
                    Err(e) => {
                        let error_text = String::from_utf8_lossy(&stderr).to_string();
                        Err(tool_error::ExecutionFailedSnafu {
                            message: format!("Patch failed: {e}\n{error_text}"),
                        }
                        .build())
                    }
                }
            }
            MaybeApplyPatchVerified::CorrectnessError(e) => Err(tool_error::ExecutionFailedSnafu {
                message: format!("Patch verification failed: {e}"),
            }
            .build()),
            MaybeApplyPatchVerified::ShellParseError(e) => Err(tool_error::InvalidInputSnafu {
                message: format!("Failed to parse patch input: {e:?}"),
            }
            .build()),
            MaybeApplyPatchVerified::NotApplyPatch => Err(tool_error::InvalidInputSnafu {
                message: "Input is not a valid apply_patch command",
            }
            .build()),
        }
    }
}

#[cfg(test)]
#[path = "apply_patch.test.rs"]
mod tests;
