//! Prompt section assembly.
//!
//! Defines the ordered sections of a system prompt and provides
//! template rendering and assembly functions.

use cocode_context::ConversationContext;
use cocode_context::InjectionPosition;
use cocode_protocol::PermissionMode;

use crate::engine;
use crate::templates;

/// Logical sections of the system prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PromptSection {
    /// Agent identity and capabilities.
    Identity,
    /// Tool usage rules.
    ToolPolicy,
    /// Security guidelines.
    Security,
    /// Git workflow rules.
    GitWorkflow,
    /// Task management approach.
    TaskManagement,
    /// MCP server instructions.
    McpInstructions,
    /// Runtime environment info.
    Environment,
    /// Permission mode rules.
    Permission,
    /// Memory file contents.
    MemoryFiles,
    /// Injected content.
    Injections,
}

/// Assemble ordered sections into a single prompt string.
///
/// Sections are joined with double newlines. Empty sections are skipped.
pub fn assemble_sections(sections: &[(PromptSection, String)]) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for (_, content) in sections {
        let trimmed = content.trim();
        if !trimmed.is_empty() {
            parts.push(trimmed);
        }
    }
    parts.join("\n\n")
}

/// Render the environment template with values from the conversation context.
pub fn render_environment(ctx: &ConversationContext) -> String {
    let env = &ctx.environment;
    engine::render(
        "environment",
        minijinja::context! {
            platform => &env.platform,
            cwd => env.cwd.display().to_string(),
            is_git_repo => env.is_git_repo,
            git_branch => env.git_branch.as_deref(),
            date => &env.date,
            os_version => &env.os_version,
            language_preference => env.language_preference.as_deref(),
        },
    )
}

/// Get the permission section text for the given mode.
pub fn permission_section(mode: &PermissionMode) -> &'static str {
    match mode {
        PermissionMode::Default => templates::PERMISSION_DEFAULT,
        PermissionMode::Plan => templates::PERMISSION_PLAN,
        PermissionMode::AcceptEdits => templates::PERMISSION_ACCEPT_EDITS,
        PermissionMode::Bypass => templates::PERMISSION_BYPASS,
        PermissionMode::DontAsk => templates::PERMISSION_DEFAULT,
    }
}

/// Render memory files as a prompt section.
pub fn render_memory_files(ctx: &ConversationContext) -> String {
    if ctx.memory_files.is_empty() {
        return String::new();
    }

    let mut files = ctx.memory_files.clone();
    files.sort_by_key(|f| f.priority);
    engine::render("memory_files", minijinja::context! { files })
}

/// Generate tool-specific policy lines based on available tool names.
///
/// Only includes policy lines for tools that are actually registered,
/// avoiding wasted tokens for disabled tools.
pub fn generate_tool_policy_lines(tool_names: &[String]) -> String {
    let tools: std::collections::HashSet<&str> = tool_names.iter().map(|s| s.as_str()).collect();
    let rules: Vec<&str> = [
        ("Read", "Use Read for reading files (not cat/head/tail)"),
        ("Edit", "Use Edit for modifying files (not sed/awk)"),
        ("Write", "Use Write for creating files (not echo/heredoc)"),
        ("Grep", "Use Grep for searching file contents (not grep/rg)"),
        (
            "Glob",
            "Use Glob for finding files by pattern (not find/ls)",
        ),
        ("LS", "Use LS for directory listing (not Bash ls)"),
    ]
    .iter()
    .filter(|(name, _)| tools.contains(name))
    .map(|(_, rule)| *rule)
    .collect();

    if rules.is_empty() {
        return String::new();
    }
    engine::render("tool_policy_lines", minijinja::context! { rules })
        .trim()
        .to_string()
}

/// Render injections for a specific position.
pub fn render_injections(ctx: &ConversationContext, position: InjectionPosition) -> String {
    let matching: Vec<&_> = ctx
        .injections
        .iter()
        .filter(|i| i.position == position)
        .collect();

    if matching.is_empty() {
        return String::new();
    }

    matching
        .iter()
        .map(|i| format!("<!-- {} -->\n{}", i.label, i.content.trim()))
        .collect::<Vec<_>>()
        .join("\n\n")
}

#[cfg(test)]
#[path = "sections.test.rs"]
mod tests;
