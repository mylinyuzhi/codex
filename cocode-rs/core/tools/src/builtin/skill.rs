//! Skill tool for executing named skills.

use super::prompts;
use crate::context::InvokedSkill;
use crate::context::ToolContext;
use crate::error::Result;
use crate::tool::Tool;
use async_trait::async_trait;
use cocode_protocol::ConcurrencySafety;
use cocode_protocol::ToolOutput;
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
        "Skill"
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

    async fn execute(&self, input: Value, ctx: &mut ToolContext) -> Result<ToolOutput> {
        let skill_name = input["skill"].as_str().ok_or_else(|| {
            crate::error::tool_error::InvalidInputSnafu {
                message: "skill must be a string",
            }
            .build()
        })?;
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
        if skill.disable_model_invocation {
            return Ok(ToolOutput::error(format!(
                "Skill '{skill_name}' cannot be invoked by the model (disable_model_invocation is set)"
            )));
        }

        // Build the prompt with argument substitution
        let mut prompt = if skill.prompt.contains("$ARGUMENTS") {
            skill.prompt.replace("$ARGUMENTS", args)
        } else if args.is_empty() {
            skill.prompt.clone()
        } else {
            format!("{}\n\nArguments: {}", skill.prompt, args)
        };

        // Inject base directory prefix if available
        if let Some(ref base_dir) = skill.base_dir {
            prompt = format!(
                "Base directory for this skill: {}\n\n{prompt}",
                base_dir.display()
            );
        }

        // Register skill hooks if the skill has an interface with hooks
        if let Some(ref interface) = skill.interface {
            if let Some(ref registry) = ctx.hook_registry {
                let hook_count = register_skill_hooks(registry, interface);
                if hook_count > 0 {
                    tracing::debug!(skill_name, hook_count, "Registered skill hooks");
                }
            }

            // Track the invoked skill for later cleanup
            ctx.invoked_skills.lock().await.push(InvokedSkill {
                name: skill_name.to_string(),
                started_at: Instant::now(),
            });
        }

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

        Ok(output)
    }
}

#[cfg(test)]
#[path = "skill.test.rs"]
mod tests;
