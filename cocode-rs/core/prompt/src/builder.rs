//! System prompt builder.
//!
//! Assembles the complete system prompt from templates and conversation context.

use cocode_context::ConversationContext;
use cocode_context::InjectionPosition;
use cocode_context::SubagentType;

use crate::sections::PromptSection;
use crate::sections::assemble_sections;
use crate::sections::permission_section;
use crate::sections::render_environment;
use crate::sections::render_injections;
use crate::sections::render_memory_files;
use crate::sections::{self};
use crate::summarization;
use crate::templates;

/// System prompt builder.
///
/// All methods are sync — pure string assembly with no I/O.
pub struct SystemPromptBuilder;

impl SystemPromptBuilder {
    /// Build the complete system prompt for a main agent.
    pub fn build(ctx: &ConversationContext) -> String {
        let mut ordered_sections = Vec::new();

        let has_output_style = ctx.output_style.is_some();
        let keep_coding = ctx
            .output_style
            .as_ref()
            .map_or(true, |s| s.keep_coding_instructions);

        // 1. Identity (strip Communication Style when output style is active)
        let identity = if has_output_style {
            strip_communication_style(templates::BASE_IDENTITY)
        } else {
            templates::BASE_IDENTITY.to_string()
        };
        ordered_sections.push((PromptSection::Identity, identity));

        // 2. Tool policy (if tools present) — skipped when output style has keep_coding=false
        if ctx.has_tools() && keep_coding {
            let mut policy = templates::TOOL_POLICY.to_string();
            let tool_lines = sections::generate_tool_policy_lines(&ctx.tool_names);
            if !tool_lines.is_empty() {
                policy.push('\n');
                policy.push_str(&tool_lines);
            }
            ordered_sections.push((PromptSection::ToolPolicy, policy));
        }

        // 3. Security
        ordered_sections.push((PromptSection::Security, templates::SECURITY.to_string()));

        // 4. Git workflow — skipped when output style has keep_coding=false
        if keep_coding {
            ordered_sections.push((
                PromptSection::GitWorkflow,
                templates::GIT_WORKFLOW.to_string(),
            ));
        }

        // 5. Task management — skipped when output style has keep_coding=false
        if keep_coding {
            ordered_sections.push((
                PromptSection::TaskManagement,
                templates::TASK_MANAGEMENT.to_string(),
            ));
        }

        // 6. MCP instructions (if MCP servers present)
        if ctx.has_mcp_servers() {
            ordered_sections.push((
                PromptSection::McpInstructions,
                templates::MCP_INSTRUCTIONS.to_string(),
            ));
        }

        // Before-tools injections
        let before_tools = render_injections(ctx, InjectionPosition::BeforeTools);
        if !before_tools.is_empty() {
            ordered_sections.push((PromptSection::Injections, before_tools));
        }

        // After-tools injections
        let after_tools = render_injections(ctx, InjectionPosition::AfterTools);
        if !after_tools.is_empty() {
            ordered_sections.push((PromptSection::Injections, after_tools));
        }

        // 7. Environment
        ordered_sections.push((PromptSection::Environment, render_environment(ctx)));

        // 8. Permission
        ordered_sections.push((
            PromptSection::Permission,
            permission_section(&ctx.permission_mode).to_string(),
        ));

        // 9. Memory files
        let memory = render_memory_files(ctx);
        if !memory.is_empty() {
            ordered_sections.push((PromptSection::MemoryFiles, memory));
        }

        // 10. End-of-prompt injections
        let end_injections = render_injections(ctx, InjectionPosition::EndOfPrompt);
        if !end_injections.is_empty() {
            ordered_sections.push((PromptSection::Injections, end_injections));
        }

        // 11. Output style instructions (appended at the end)
        if let Some(ref style) = ctx.output_style {
            ordered_sections.push((
                PromptSection::OutputStyle,
                format!("# Output Style: {}\n\n{}", style.name, style.content),
            ));
        }

        assemble_sections(&ordered_sections)
    }

    /// Build system prompt for a subagent (explore/plan).
    pub fn build_for_subagent(ctx: &ConversationContext, subagent_type: SubagentType) -> String {
        let subagent_template = match subagent_type {
            SubagentType::Explore => templates::EXPLORE_SUBAGENT,
            SubagentType::Plan => templates::PLAN_SUBAGENT,
        };

        let mut sections = vec![
            (PromptSection::Identity, subagent_template.to_string()),
            (PromptSection::Security, templates::SECURITY.to_string()),
            (
                PromptSection::Environment,
                sections::render_environment(ctx),
            ),
        ];

        // Include memory files for subagents too
        let memory = render_memory_files(ctx);
        if !memory.is_empty() {
            sections.push((PromptSection::MemoryFiles, memory));
        }

        assemble_sections(&sections)
    }

    /// Build summarization prompts for context compaction.
    ///
    /// Returns `(system_prompt, user_prompt)`.
    pub fn build_summarization(
        conversation_text: &str,
        custom_instructions: Option<&str>,
    ) -> (String, String) {
        summarization::build_summarization_prompt(conversation_text, custom_instructions)
    }

    /// Build brief summarization prompts for micro-compaction.
    ///
    /// Returns `(system_prompt, user_prompt)`.
    pub fn build_brief_summarization(conversation_text: &str) -> (String, String) {
        summarization::build_brief_summary_prompt(conversation_text)
    }
}

/// Strip the "## Communication Style" section from identity text.
///
/// When an output style is active, the default communication style
/// instructions conflict with the output style. This function removes
/// the `## Communication Style` section (header + body) while keeping
/// all other sections intact.
fn strip_communication_style(identity: &str) -> String {
    let mut result = String::new();
    let mut skip = false;

    for line in identity.lines() {
        if line.starts_with("## Communication Style") {
            skip = true;
            continue;
        }
        // Stop skipping when we hit another ## heading
        if skip && line.starts_with("## ") {
            skip = false;
        }
        if !skip {
            result.push_str(line);
            result.push('\n');
        }
    }

    result
}

#[cfg(test)]
#[path = "builder.test.rs"]
mod tests;
