//! Skill tool for executing named skills.

use super::prompts;
use crate::context::InvokedSkill;
use crate::context::SpawnAgentInput;
use crate::context::ToolContext;
use crate::error::Result;
use crate::tool::Tool;
use async_trait::async_trait;
use cocode_protocol::ConcurrencySafety;
use cocode_protocol::PermissionResult;
use cocode_protocol::ToolOutput;
use cocode_skill::SkillContext;
use cocode_skill::register_skill_hooks;
use serde_json::Value;
use std::time::Instant;

/// Tool for executing named skills (slash commands).
///
/// Delegates to the skill system to load and run skills
/// defined in the project or user configuration.
pub struct SkillTool;

impl SkillTool {
    /// Create a new Skill tool.
    pub fn new() -> Self {
        Self
    }
}

impl Default for SkillTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for SkillTool {
    fn name(&self) -> &str {
        cocode_protocol::ToolName::Skill.as_str()
    }

    fn description(&self) -> &str {
        prompts::SKILL_DESCRIPTION
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "skill": {
                    "type": "string",
                    "description": "The skill name. E.g., 'commit', 'review-pr', or 'pdf'"
                },
                "args": {
                    "type": "string",
                    "description": "Optional arguments for the skill"
                }
            },
            "required": ["skill"]
        })
    }

    fn concurrency_safety(&self) -> ConcurrencySafety {
        ConcurrencySafety::Unsafe
    }

    fn is_read_only(&self) -> bool {
        false
    }

    async fn check_permission(&self, input: &Value, ctx: &ToolContext) -> PermissionResult {
        // Auto-allow skills that have no unsafe properties (no allowed_tools, no hooks).
        // Skills with tool restrictions or hooks need user approval.
        let skill_name = match input["skill"].as_str() {
            Some(name) => name,
            None => return PermissionResult::Passthrough,
        };

        let skill = match ctx
            .skill_manager
            .as_ref()
            .and_then(|sm| sm.find_by_name_or_alias(skill_name))
        {
            Some(s) => s,
            None => return PermissionResult::Passthrough,
        };

        let has_allowed_tools = skill.allowed_tools.is_some();
        let has_hooks = skill
            .interface
            .as_ref()
            .is_some_and(|i| i.hooks.as_ref().is_some_and(|h| !h.is_empty()));

        if !has_allowed_tools && !has_hooks {
            PermissionResult::Allowed
        } else {
            PermissionResult::Passthrough
        }
    }

    async fn execute(&self, input: Value, ctx: &mut ToolContext) -> Result<ToolOutput> {
        let skill_name = super::input_helpers::require_str(&input, "skill")?;
        let args = input["args"].as_str().unwrap_or("");

        ctx.emit_progress(format!("Executing skill: {skill_name}"))
            .await;

        // Get skill manager from context
        let skill_manager = ctx.skill_manager.as_ref().ok_or_else(|| {
            crate::error::tool_error::InternalSnafu {
                message: "Skill manager not configured",
            }
            .build()
        })?;

        // Look up the skill by name or alias
        let skill = skill_manager
            .find_by_name_or_alias(skill_name)
            .ok_or_else(|| {
                crate::error::tool_error::NotFoundSnafu {
                    name: format!("skill '{skill_name}'"),
                }
                .build()
            })?;

        // Check if the LLM is allowed to invoke this skill
        if !skill.is_llm_invocable() {
            return Ok(ToolOutput::error(format!(
                "Skill '{skill_name}' cannot be invoked by the model"
            )));
        }

        // Build the prompt with full argument substitution (shared with execute_skill)
        let prompt = cocode_skill::substitute_skill_args(
            &skill.prompt,
            args,
            skill.arguments.as_deref(),
            skill.base_dir.as_deref(),
        );

        // Check for fork context — spawn subagent instead of inline execution
        if skill.context == SkillContext::Fork {
            if ctx.can_spawn_agent() {
                let agent_type = skill.agent.clone().unwrap_or_else(|| "general".to_string());
                let spawn_input = SpawnAgentInput {
                    agent_type: agent_type.clone(),
                    prompt: prompt.clone(),
                    model: skill.model.clone(),
                    max_turns: None,
                    run_in_background: Some(false),
                    allowed_tools: skill.allowed_tools.clone(),
                    parent_selections: ctx.parent_selections.clone(),
                    permission_mode: None,
                    resume_from: None,
                    isolation: None,
                    name: None,
                    team_name: None,
                    mode: None,
                    cwd: None,
                    description: None,
                };

                // Emit SubagentSpawned event for TUI visibility
                ctx.emit_event(cocode_protocol::LoopEvent::SubagentSpawned {
                    agent_id: String::new(), // Will be filled by result
                    agent_type: agent_type.clone(),
                    description: format!("Skill fork: {skill_name}"),
                    color: None,
                })
                .await;

                match ctx.spawn_agent(spawn_input).await {
                    Ok(result) => {
                        // Register cancel token for background agents
                        if let Some(token) = result.cancel_token.clone() {
                            ctx.agent_cancel_tokens
                                .lock()
                                .await
                                .insert(result.agent_id.clone(), token);
                        }

                        let output_text = result.output.unwrap_or_default();
                        return Ok(ToolOutput::text(format!(
                            "<skill-result name=\"{skill_name}\" agent=\"forked\" agent_id=\"{}\">\n{output_text}\n</skill-result>",
                            result.agent_id
                        )));
                    }
                    Err(e) => {
                        tracing::warn!(
                            skill_name,
                            status = ?e.status_code(),
                            error = ?e,
                            "Fork context spawn failed; falling back to inline execution"
                        );
                        // Fall through to inline execution
                    }
                }
            } else {
                tracing::warn!(
                    skill_name,
                    "Fork context requested but spawn unavailable; executing inline"
                );
            }
        }

        // Register skill hooks if the skill has an interface with hooks
        if let Some(ref interface) = skill.interface
            && let Some(ref registry) = ctx.hook_registry
        {
            let hook_count = register_skill_hooks(registry, interface);
            if hook_count > 0 {
                tracing::debug!(skill_name, hook_count, "Registered skill hooks");
            }
        }

        // Record usage for scoring
        if let Some(ref tracker) = ctx.skill_usage_tracker {
            tracker.track(skill_name);
        }

        // Track every inline invocation for system reminder injection
        ctx.invoked_skills.lock().await.push(InvokedSkill {
            name: skill_name.to_string(),
            started_at: Instant::now(),
            prompt_content: prompt.clone(),
            path: skill.base_dir.clone(),
        });

        // Build the output with optional tool restriction modifier
        let mut output = ToolOutput::text(format!(
            "<skill-invoked name=\"{skill_name}\">\n{prompt}\n</skill-invoked>"
        ));

        // If the skill specifies allowed_tools, add a context modifier
        // so the driver can restrict tool execution
        if let Some(ref allowed_tools) = skill.allowed_tools {
            output
                .modifiers
                .push(cocode_protocol::ContextModifier::SkillAllowedTools {
                    skill_name: skill_name.to_string(),
                    allowed_tools: allowed_tools.clone(),
                });
        }

        // For inline skills with a model override, emit a ModelOverride modifier
        // (fork context handles model via SpawnAgentInput.model)
        if skill.context != SkillContext::Fork
            && let Some(ref model) = skill.model
        {
            output
                .modifiers
                .push(cocode_protocol::ContextModifier::ModelOverride {
                    model: model.clone(),
                    skill_name: skill_name.to_string(),
                });
        }

        Ok(output)
    }
}

#[cfg(test)]
#[path = "skill.test.rs"]
mod tests;
