//! `SkillTool` — execute a skill (slash command) within the main conversation.
//!
//! TS: `tools/SkillTool/`.

use std::collections::HashMap;

use coco_messages::ToolResult;
use coco_tool_runtime::DescriptionOptions;
use coco_tool_runtime::Tool;
use coco_tool_runtime::ToolError;
use coco_tool_runtime::ToolResultContentPart;
use coco_tool_runtime::ToolUseContext;
use coco_types::ToolId;
use coco_types::ToolInputSchema;
use coco_types::ToolName;
use serde_json::Value;

pub struct SkillTool;

#[async_trait::async_trait]
impl Tool for SkillTool {
    fn id(&self) -> ToolId {
        ToolId::Builtin(ToolName::Skill)
    }
    fn name(&self) -> &str {
        ToolName::Skill.as_str()
    }
    fn description(&self, _: &Value, _options: &DescriptionOptions) -> String {
        "Execute a skill within the main conversation. Skills provide specialized \
         capabilities and domain knowledge."
            .into()
    }
    fn input_schema(&self) -> ToolInputSchema {
        let mut p = HashMap::new();
        p.insert(
            "skill".into(),
            serde_json::json!({
                "type": "string",
                "description": "The skill name to invoke (e.g. 'commit', 'review-pr', 'pdf')"
            }),
        );
        p.insert(
            "args".into(),
            serde_json::json!({
                "type": "string",
                "description": "Optional arguments for the skill"
            }),
        );
        ToolInputSchema { properties: p }
    }

    /// Render the skill envelope. TS parity:
    /// `SkillTool.ts:843-862 mapToolResultToToolResultBlockParam`,
    /// data shape per `SkillTool.ts:301-326 outputSchema`.
    ///
    /// Two branches keyed off the `status` field added by execute:
    /// - `"inline"`: model sees `Launching skill: {commandName}` —
    ///   the resolved prompt is fed back into the agent's history
    ///   via `new_messages`, not the tool result.
    /// - `"forked"`: model sees `Skill "{commandName}" completed
    ///   (forked execution).\n\nResult:\n{result}` — the child
    ///   agent ran in its own session and the aggregated output is
    ///   echoed here.
    ///
    /// The pre-Phase-7 single-string envelope is supported as a
    /// fallback so older transcripts and `NoOpSkillHandle` test
    /// doubles still render sensibly.
    fn render_for_model(&self, data: &Value) -> Vec<ToolResultContentPart> {
        let status = data.get("status").and_then(Value::as_str);
        let command_name = data
            .get("commandName")
            .and_then(Value::as_str)
            .unwrap_or("");
        let text = match status {
            Some("forked") => {
                let result = data.get("result").and_then(Value::as_str).unwrap_or("");
                format!(
                    "Skill \"{command_name}\" completed (forked execution).\n\nResult:\n{result}"
                )
            }
            Some("inline") => format!("Launching skill: {command_name}"),
            _ => data
                .as_str()
                .map(str::to_string)
                .unwrap_or_else(|| serde_json::to_string(data).unwrap_or_default()),
        };
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
        let skill_name = input
            .get("skill")
            .and_then(|v| v.as_str())
            .unwrap_or_default();

        if skill_name.is_empty() {
            return Err(ToolError::InvalidInput {
                message: "skill name is required".into(),
                error_code: None,
            });
        }

        let args = input
            .get("args")
            .and_then(|v| v.as_str())
            .unwrap_or_default();

        // Skills route through the dedicated `SkillHandle` (Phase 7 split)
        // rather than `AgentHandle` — skills are a different runtime
        // concept. Forward parent's Layer 1 + 2 so a fork-mode skill
        // subagent inherits the same gate set; inline expansion ignores it.
        let inherit = coco_tool_runtime::SubagentInheritance {
            features: Some(ctx.features.clone()),
            tool_overrides: Some(ctx.tool_overrides.clone()),
            parent_tool_filter: Some(ctx.tool_filter.clone()),
        };
        let result = ctx
            .skill
            .invoke_skill(skill_name, args, inherit)
            .await
            .map_err(|e| ToolError::ExecutionFailed {
                message: format!("Failed to resolve skill '{skill_name}': {e}"),
                source: None,
            })?;

        // Flatten `SkillInvocationResult` into a TS-shaped envelope so
        // render can produce `Launching skill: ...` (inline) or
        // `Skill "..." completed (forked execution).\n\nResult:\n...`
        // (forked) per `SkillTool.ts:843-862`. Field names match the TS
        // `outputSchema` at `SkillTool.ts:301-326` (`status`/`result`,
        // not `mode`/`output`) so transcript readers see the same wire
        // shape across runtimes. The inline `new_messages` are
        // preserved on the data envelope as raw JSON `Value`s for the
        // runtime to splice at the seam where Value→Message conversion
        // lives — this tool's `new_messages: Vec<Message>` slot stays
        // empty.
        let data = match result {
            coco_tool_runtime::SkillInvocationResult::Inline {
                summary,
                new_messages,
            } => serde_json::json!({
                "status": "inline",
                "success": true,
                "commandName": skill_name,
                "summary": summary,
                "new_messages": new_messages,
            }),
            coco_tool_runtime::SkillInvocationResult::Forked { agent_id, output } => {
                serde_json::json!({
                    "status": "forked",
                    "success": true,
                    "commandName": skill_name,
                    "agentId": agent_id,
                    "result": output,
                })
            }
        };

        Ok(ToolResult {
            data,
            new_messages: vec![],
            app_state_patch: None,
        })
    }
}
