//! `SkillTool` — execute a skill (slash command) within the main conversation.
//!
//! TS: `tools/SkillTool/`.

use std::collections::HashMap;

use coco_messages::ToolResult;
use coco_tool_runtime::DescriptionOptions;
use coco_tool_runtime::Tool;
use coco_tool_runtime::ToolError;
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

        Ok(ToolResult {
            data: serde_json::json!(result),
            new_messages: vec![],
            app_state_patch: None,
        })
    }
}
