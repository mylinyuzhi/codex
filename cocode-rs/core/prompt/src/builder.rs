//! System prompt builder.
//!
//! Assembles the complete system prompt from templates and conversation context.

use cocode_context::ConversationContext;
use cocode_context::InjectionPosition;
use cocode_context::SubagentType;
use cocode_protocol::CacheScope;

use crate::cache_block::SystemPromptBlock;
use crate::engine;
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
        assemble_sections(&Self::build_ordered_sections(ctx))
    }

    /// Build the system prompt as cache-scoped blocks for prompt caching.
    ///
    /// Splits the prompt into a stable prefix (rarely changes within a session)
    /// and a dynamic suffix (changes per session/turn). The stable prefix can
    /// be cached with a `Global` scope for higher cache hit rates.
    ///
    /// This implements Claude Code's Mode 2 (boundary-based) splitting strategy,
    /// equivalent to the `__SYSTEM_PROMPT_DYNAMIC_BOUNDARY__` mechanism.
    ///
    /// Returns 1-2 blocks: stable (Global scope) and dynamic (no scope).
    pub fn build_for_cache(ctx: &ConversationContext) -> Vec<SystemPromptBlock> {
        let all_sections = Self::build_ordered_sections(ctx);

        // Split at the Environment section: everything before is stable
        // (Identity, ToolPolicy, Security, GitWorkflow, TaskManagement,
        // McpInstructions, Before/AfterTools injections), everything from
        // Environment onward is dynamic (Environment, Permission, MemoryFiles,
        // EndOfPrompt injections, OutputStyle).
        let split_idx = all_sections
            .iter()
            .position(|(s, _)| *s == PromptSection::Environment)
            .unwrap_or(all_sections.len());

        let mut blocks = Vec::new();

        let stable_text = assemble_sections(&all_sections[..split_idx]);
        if !stable_text.is_empty() {
            blocks.push(SystemPromptBlock {
                text: stable_text,
                cache_scope: Some(CacheScope::Global),
            });
        }

        let dynamic_text = assemble_sections(&all_sections[split_idx..]);
        if !dynamic_text.is_empty() {
            blocks.push(SystemPromptBlock {
                text: dynamic_text,
                cache_scope: None,
            });
        }

        blocks
    }

    /// Build all ordered sections of the system prompt.
    ///
    /// Shared by `build()` and `build_for_cache()` to avoid duplication.
    /// Sections are returned in canonical order: stable sections first
    /// (Identity through AfterTools injections), then dynamic sections
    /// (Environment through OutputStyle).
    fn build_ordered_sections(ctx: &ConversationContext) -> Vec<(PromptSection, String)> {
        let has_output_style = ctx.output_style.is_some();
        let keep_coding = ctx
            .output_style
            .as_ref()
            .is_none_or(|s| s.keep_coding_instructions);

        let mut sections = Vec::new();

        // 1. Identity (strip Communication Style when output style is active)
        let identity = if has_output_style {
            strip_communication_style(templates::BASE_IDENTITY)
        } else {
            templates::BASE_IDENTITY.to_string()
        };
        sections.push((PromptSection::Identity, identity));

        // 2. Tool policy (if tools present) — skipped when keep_coding=false
        if !ctx.tool_names.is_empty() && keep_coding {
            let mut policy = engine::render("tool_policy", minijinja::context! {});
            let tool_lines = sections::generate_tool_policy_lines(&ctx.tool_names);
            if !tool_lines.is_empty() {
                policy.push('\n');
                policy.push_str(&tool_lines);
            }
            sections.push((PromptSection::ToolPolicy, policy));
        }

        // 2b. Sandbox mode (Bash tool restrictions) — only if sandbox is active
        if ctx.sandbox_active
            && let Some(ref desc) = ctx.sandbox_enforcement_desc
        {
            let mode_block = if ctx.sandbox_allow_unsandboxed {
                // Open mode: matches Claude Code's exact guidance
                "- You should always default to running commands within the sandbox. \
                 Do NOT attempt to set `dangerouslyDisableSandbox: true` unless:\n\
                   - The user *explicitly* asks you to bypass sandbox\n\
                   - A specific command just failed and you see evidence of sandbox \
                     restrictions causing the failure. Note that commands can fail for many \
                     reasons unrelated to the sandbox (missing files, wrong arguments, \
                     network issues, etc.).\n\n\
                 - Evidence of sandbox-caused failures includes:\n\
                   - \"Operation not permitted\" errors for file/network operations\n\
                   - Access denied to specific paths outside allowed directories\n\
                   - Network connection failures to non-whitelisted hosts\n\
                   - Unix socket connection errors\n\n\
                 - When you see evidence of sandbox-caused failure:\n\
                   - Immediately retry with `dangerouslyDisableSandbox: true` (don't ask, just do it)\n\
                   - Briefly explain what sandbox restriction likely caused the failure. Be sure to \
                     mention that the user can use the `/sandbox` command to manage restrictions.\n\
                   - This will prompt the user for permission\n\n\
                 - Treat each command you execute with `dangerouslyDisableSandbox: true` individually. \
                   Even if you have recently run a command with this setting, you should default to \
                   running future commands within the sandbox.\n\n\
                 - Do not suggest adding sensitive paths like ~/.bashrc, ~/.zshrc, ~/.ssh/*, or \
                   credential files to the sandbox allowlist."
                    .to_string()
            } else {
                // Closed mode: matches Claude Code's exact guidance
                "- All commands MUST run in sandbox mode - the `dangerouslyDisableSandbox` \
                 parameter is disabled by policy.\n\
                 - Commands cannot run outside the sandbox under any circumstances.\n\
                 - If a command fails due to sandbox restrictions, work with the user to adjust \
                   sandbox settings instead."
                    .to_string()
            };

            // Build JSON restriction strings matching Claude Code's format
            let restrictions = format!(
                "Filesystem: {desc}\nNetwork: {}",
                ctx.sandbox_network_desc
                    .as_deref()
                    .unwrap_or("Allowed (proxy-filtered when available)")
            );

            let mut sandbox_block = templates::SANDBOX_BASH
                .replace("{restrictions}", &restrictions)
                .replace("{mode_block}", &mode_block);

            // MCP CLI exception: MCP tool commands route through the parent
            // process, not actual shell execution, so they must bypass sandbox.
            if !ctx.mcp_server_names.is_empty() {
                sandbox_block.push_str(
                    "\n- EXCEPTION: `mcp-cli` commands must always be called with \
                     `dangerouslyDisableSandbox: true` because they route through \
                     parent MCP connections, not actual shell execution.",
                );
            }

            sections.push((PromptSection::ToolPolicy, sandbox_block));
        }

        // 3. Security
        sections.push((PromptSection::Security, templates::SECURITY.to_string()));

        // 4. Git workflow — skipped when keep_coding=false
        if keep_coding {
            sections.push((
                PromptSection::GitWorkflow,
                templates::GIT_WORKFLOW.to_string(),
            ));
        }

        // 5. Task management — skipped when keep_coding=false
        if keep_coding {
            sections.push((
                PromptSection::TaskManagement,
                templates::TASK_MANAGEMENT.to_string(),
            ));
        }

        // 6. MCP instructions (if MCP servers present)
        if !ctx.mcp_server_names.is_empty() {
            sections.push((
                PromptSection::McpInstructions,
                templates::MCP_INSTRUCTIONS.to_string(),
            ));
        }

        // Before-tools injections
        let before_tools = render_injections(ctx, InjectionPosition::BeforeTools);
        if !before_tools.is_empty() {
            sections.push((PromptSection::Injections, before_tools));
        }

        // After-tools injections
        let after_tools = render_injections(ctx, InjectionPosition::AfterTools);
        if !after_tools.is_empty() {
            sections.push((PromptSection::Injections, after_tools));
        }

        // --- Dynamic boundary (stable sections above, dynamic below) ---

        // 7. Environment
        sections.push((PromptSection::Environment, render_environment(ctx)));

        // 8. Permission
        sections.push((
            PromptSection::Permission,
            permission_section(&ctx.permission_mode).to_string(),
        ));

        // 9. Memory files
        let memory = render_memory_files(ctx);
        if !memory.is_empty() {
            sections.push((PromptSection::MemoryFiles, memory));
        }

        // 10. End-of-prompt injections
        let end_injections = render_injections(ctx, InjectionPosition::EndOfPrompt);
        if !end_injections.is_empty() {
            sections.push((PromptSection::Injections, end_injections));
        }

        // 11. Output style instructions (appended at the end)
        if let Some(ref style) = ctx.output_style {
            sections.push((
                PromptSection::OutputStyle,
                format!("# Output Style: {}\n\n{}", style.name, style.content),
            ));
        }

        sections
    }

    /// Build system prompt for a subagent (explore/plan).
    pub fn build_for_subagent(ctx: &ConversationContext, subagent_type: SubagentType) -> String {
        // Only Explore and Plan have custom templates; others use default
        let template_name = match subagent_type {
            SubagentType::Explore => Some("explore_subagent"),
            SubagentType::Plan => Some("plan_subagent"),
            _ => None,
        };

        let mut sections = if let Some(template) = template_name {
            vec![
                (
                    PromptSection::Identity,
                    engine::render(template, minijinja::context! {}),
                ),
                (PromptSection::Security, templates::SECURITY.to_string()),
                (
                    PromptSection::Environment,
                    sections::render_environment(ctx),
                ),
            ]
        } else {
            // Default: use the main agent prompt
            return Self::build(ctx);
        };

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
