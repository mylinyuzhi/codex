//! VerifyPlanExecutionTool — record a post-plan verification checkpoint.
//!
//! TS parity: `tools.ts` conditionally registers `VerifyPlanExecutionTool`
//! when `CLAUDE_CODE_VERIFY_PLAN === 'true'`; the mirrored source tree only
//! contains the conditional references, not the tool implementation. coco-rs
//! ships a small built-in tool so `verify_plan_reminder` always points at a
//! callable tool instead of a dangling name.
//!
//! **Scope.** TS's (unavailable) tool triggers a *background verification*
//! agent (`state/AppStateStore.ts` carries `verificationStarted` /
//! `verificationCompleted` sub-flags for that flow). coco-rs deliberately
//! ships the simpler shape: this tool does **not** verify anything itself —
//! the model is expected to inspect files and run checks first; calling the
//! tool only *records the checkpoint* and clears `pending_plan_verification`
//! so the `verify_plan_reminder` nudge stops firing.

use std::collections::HashMap;

use coco_messages::ToolResult;
use coco_tool_runtime::DescriptionOptions;
use coco_tool_runtime::Tool;
use coco_tool_runtime::ToolError;
use coco_tool_runtime::ToolResultContentPart;
use coco_tool_runtime::ToolUseContext;
use coco_types::ToolCheckResult;
use coco_types::ToolId;
use coco_types::ToolInputSchema;
use coco_types::ToolName;
use serde_json::Value;

pub struct VerifyPlanExecutionTool;

#[async_trait::async_trait]
impl Tool for VerifyPlanExecutionTool {
    fn id(&self) -> ToolId {
        ToolId::Builtin(ToolName::VerifyPlanExecution)
    }

    fn name(&self) -> &str {
        ToolName::VerifyPlanExecution.as_str()
    }

    fn description(&self, _: &Value, _options: &DescriptionOptions) -> String {
        "Record a checkpoint that you have verified the implementation against the approved plan. \
         This tool does not run any verification itself — do the verification first, then call it."
            .into()
    }

    async fn prompt(&self, _options: &coco_tool_runtime::PromptOptions) -> String {
        "Call this only AFTER you have directly verified that the implementation satisfies the \
         approved plan: inspect the relevant files and run the appropriate checks yourself (do \
         not delegate to the Agent tool or a subagent). The tool performs no verification of its \
         own — it just records the checkpoint and clears the pending plan-verification reminder."
            .into()
    }

    fn input_schema(&self) -> ToolInputSchema {
        let mut properties = HashMap::new();
        properties.insert(
            "summary".into(),
            serde_json::json!({
                "type": "string",
                "description": "Brief summary of what was verified."
            }),
        );
        properties.insert(
            "issues".into(),
            serde_json::json!({
                "type": "string",
                "description": "Any remaining issues or gaps found during verification. Leave empty when none."
            }),
        );
        ToolInputSchema {
            properties,
            required: Vec::new(),
        }
    }

    fn is_read_only(&self, _: &Value) -> bool {
        true
    }

    fn requires_user_interaction(&self) -> bool {
        false
    }

    async fn check_permissions(&self, _input: &Value, _ctx: &ToolUseContext) -> ToolCheckResult {
        ToolCheckResult::Allow {
            updated_input: None,
            feedback: None,
        }
    }

    fn render_for_model(&self, data: &Value) -> Vec<ToolResultContentPart> {
        let status = data
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("verified");
        let mut text = match status {
            "no_pending_verification" => "No pending plan verification was active.".to_string(),
            _ => "Plan execution verification recorded.".to_string(),
        };
        if let Some(path) = data.get("planFilePath").and_then(Value::as_str)
            && !path.is_empty()
        {
            text.push_str(&format!(" Plan file: {path}."));
        }
        vec![ToolResultContentPart::Text {
            text,
            provider_options: None,
        }]
    }

    async fn execute(
        &self,
        input: Value,
        ctx: &ToolUseContext,
    ) -> Result<ToolResult<Value>, ToolError> {
        let pending = match ctx.app_state.as_ref() {
            Some(state) => state.read().await.pending_plan_verification,
            None => false,
        };

        let session_id = ctx.session_id_for_history.as_deref();
        let agent_id = ctx.agent_id.as_ref().map(|a| a.as_str().to_string());
        let plan_file_path = match (session_id, ctx.plans_dir.as_ref()) {
            (Some(sid), Some(plans_dir)) => Some(
                coco_context::get_plan_file_path(sid, plans_dir, agent_id.as_deref())
                    .to_string_lossy()
                    .into_owned(),
            ),
            _ => None,
        };

        let summary = input
            .get("summary")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        let issues = input
            .get("issues")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();

        let status = if pending {
            "verified"
        } else {
            "no_pending_verification"
        };

        let patch: coco_types::AppStatePatch = Box::new(|state| {
            state.pending_plan_verification = false;
        });

        Ok(ToolResult::data(serde_json::json!({
            "status": status,
            "planFilePath": plan_file_path,
            "summary": summary,
            "issues": issues,
        }))
        .with_patch(patch))
    }
}

#[cfg(test)]
#[path = "verify_plan_execution.test.rs"]
mod tests;
