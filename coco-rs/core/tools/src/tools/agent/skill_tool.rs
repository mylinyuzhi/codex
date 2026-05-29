//! `SkillTool` — execute a skill (slash command) within the main conversation.
//!
//! TS: `tools/SkillTool/`.

use coco_messages::ToolResult;
use coco_tool_runtime::DescriptionOptions;
use coco_tool_runtime::SkillGateContext;
use coco_tool_runtime::Tool;
use coco_tool_runtime::ToolError;
use coco_tool_runtime::ToolResultContentPart;
use coco_tool_runtime::ToolUseContext;
use coco_types::ToolId;
use coco_types::ToolName;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;

/// Typed input for [`SkillTool`].
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct SkillInput {
    /// The skill name to invoke (e.g. 'commit', 'review-pr', 'pdf')
    #[serde(default)]
    pub skill: String,
    /// Optional arguments for the skill
    #[serde(default)]
    pub args: Option<String>,
}

/// Typed output for [`SkillTool`]. Tagged union of `inline` (resolved
/// prompt fed back into the agent's history) and `forked` (child agent
/// ran in its own session, output aggregated).
///
/// TS parity: `SkillTool.ts:301-326 outputSchema`. Wire field names
/// preserved (`commandName`, `agentId`) via `rename` for cross-runtime
/// transcript compatibility.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum SkillOutput {
    /// Inline path — the resolved prompt was spliced into history via
    /// `new_messages`; the renderer emits `Launching skill: {name}`.
    Inline {
        #[serde(default)]
        success: bool,
        #[serde(rename = "commandName", default)]
        command_name: String,
        #[serde(default)]
        summary: String,
        /// Raw JSON message Values handed to the runtime's
        /// `Value → Message` adapter. Kept as `Vec<Value>` because the
        /// concrete `Message` type lives outside this module.
        #[serde(default)]
        new_messages: Vec<Value>,
    },
    /// Forked path — the skill ran as a subagent; the renderer emits
    /// `Skill "{name}" completed (forked execution).\n\nResult:\n...`.
    Forked {
        #[serde(default)]
        success: bool,
        #[serde(rename = "commandName", default)]
        command_name: String,
        #[serde(rename = "agentId", default)]
        agent_id: String,
        #[serde(default)]
        result: String,
    },
}

pub struct SkillTool;

#[async_trait::async_trait]
impl Tool for SkillTool {
    type Input = SkillInput;
    coco_tool_runtime::impl_runtime_schema!(SkillInput);
    type Output = SkillOutput;

    fn id(&self) -> ToolId {
        ToolId::Builtin(ToolName::Skill)
    }
    fn name(&self) -> &str {
        ToolName::Skill.as_str()
    }
    fn description(&self, _input: &SkillInput, _options: &DescriptionOptions) -> String {
        "Execute a skill within the main conversation. Skills provide specialized \
         capabilities and domain knowledge."
            .into()
    }

    /// Render the skill envelope. TS parity:
    /// `SkillTool.ts:843-862 mapToolResultToToolResultBlockParam`,
    /// data shape per `SkillTool.ts:301-326 outputSchema`.
    fn render_for_model(&self, out: &SkillOutput) -> Vec<ToolResultContentPart> {
        let text = match out {
            SkillOutput::Inline { command_name, .. } => {
                format!("Launching skill: {command_name}")
            }
            SkillOutput::Forked {
                command_name,
                result,
                ..
            } => format!(
                "Skill \"{command_name}\" completed (forked execution).\n\nResult:\n{result}"
            ),
        };
        vec![ToolResultContentPart::Text {
            text,
            provider_options: None,
        }]
    }

    async fn execute(
        &self,
        input: SkillInput,
        ctx: &ToolUseContext,
    ) -> Result<ToolResult<SkillOutput>, ToolError> {
        if input.skill.is_empty() {
            return Err(ToolError::InvalidInput {
                message: "skill name is required".into(),
                error_code: None,
            });
        }

        let skill_name = input.skill.as_str();
        let args = input.args.as_deref().unwrap_or_default();

        // Skills route through the dedicated `SkillHandle` (Phase 7 split)
        // rather than `AgentHandle` — skills are a different runtime
        // concept. Forward parent's Layer 1 + 2 so a fork-mode skill
        // subagent inherits the same gate set; inline expansion ignores it.
        let inherit = coco_tool_runtime::SubagentInheritance {
            features: Some(ctx.features.clone()),
            tool_overrides: Some(ctx.tool_overrides.clone()),
            parent_tool_filter: Some(ctx.tool_filter.clone()),
        };
        // `gate` carries the inputs `QuerySkillRuntime` needs to
        // enforce the TS 4-state Skill tool gate
        // (`cli_inner_pretty.js:353567-353590`). With default-empty
        // `skill_overrides` tiers the gate short-circuits to `On` so
        // PR2 introduces no observable behavior change.
        //
        // The handle resolves the skill (canonical name + aliases)
        // and tests every candidate against `typed_slashes_in_turn`
        // — `commit-fast → commit` aliases bypass the gate
        // correctly even though the SkillTool only sees the
        // canonical name.
        let gate = SkillGateContext {
            overrides: ctx.skill_overrides.clone(),
            typed_slashes_in_turn: ctx.typed_slashes_in_turn(),
        };
        let result = ctx
            .skill
            .invoke_skill(skill_name, args, inherit, gate)
            .await
            .map_err(|e| ToolError::ExecutionFailed {
                message: format!("Failed to resolve skill '{skill_name}': {e}"),
                source: None,
            })?;

        // Pull `permission_updates` off the inline variant so the
        // skill's `allowed-tools` frontmatter folds into the running
        // session config via the executor's `PermissionRuleHandle`.
        // Fork variant has no return-channel updates — those were
        // applied at dispatch time via `AgentQueryConfig.extra_permission_rules`.
        let (data, permission_updates) = match result {
            coco_tool_runtime::SkillInvocationResult::Inline {
                summary,
                new_messages,
                permission_updates,
            } => {
                let new_messages_value: Vec<Value> = new_messages
                    .iter()
                    .map(|m| serde_json::to_value(m).unwrap_or(Value::Null))
                    .collect();
                (
                    SkillOutput::Inline {
                        success: true,
                        command_name: skill_name.to_string(),
                        summary,
                        new_messages: new_messages_value,
                    },
                    permission_updates,
                )
            }
            coco_tool_runtime::SkillInvocationResult::Forked { agent_id, output } => (
                SkillOutput::Forked {
                    success: true,
                    command_name: skill_name.to_string(),
                    agent_id,
                    result: output,
                },
                Vec::new(),
            ),
        };

        Ok(ToolResult {
            data,
            new_messages: vec![],
            app_state_patch: None,
            permission_updates,
        })
    }
}
