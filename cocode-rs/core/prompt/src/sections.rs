//! Prompt section assembly.
//!
//! Defines the ordered sections of a system prompt and provides
//! template rendering and assembly functions.

use cocode_context::ConversationContext;
use cocode_context::InjectionPosition;
use cocode_protocol::PermissionMode;

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
    let git_branch = ctx.environment.git_branch.as_deref().unwrap_or("(none)");

    let mut env = templates::ENVIRONMENT_TEMPLATE
        .replace("{{platform}}", &ctx.environment.platform)
        .replace("{{os_version}}", &ctx.environment.os_version)
        .replace("{{cwd}}", &ctx.environment.cwd.display().to_string())
        .replace("{{is_git_repo}}", &ctx.environment.is_git_repo.to_string())
        .replace("{{git_branch}}", git_branch)
        .replace("{{date}}", &ctx.environment.date)
        .replace("{{model}}", &ctx.environment.model);

    // Append language preference if set
    if let Some(ref lang) = ctx.environment.language_preference {
        env.push_str(&format!("\n# Language Preference\n\nYou MUST respond in {}. All your responses, explanations, and communications should be in this language unless the user explicitly requests otherwise.\n", lang));
    }

    env
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

    let mut parts = vec!["# Memory Files".to_string()];
    let mut sorted_files = ctx.memory_files.clone();
    sorted_files.sort_by_key(|f| f.priority);

    for file in &sorted_files {
        parts.push(format!("## {}\n\n{}", file.path, file.content.trim()));
    }

    parts.join("\n\n")
}

/// Generate tool-specific policy lines based on available tool names.
///
/// Only includes policy lines for tools that are actually registered,
/// avoiding wasted tokens for disabled tools.
pub fn generate_tool_policy_lines(tool_names: &[String]) -> String {
    let tools: std::collections::HashSet<&str> = tool_names.iter().map(|s| s.as_str()).collect();
    let mut lines = Vec::new();
    if tools.contains("Read") {
        lines.push("- Use Read for reading files (not cat/head/tail)");
    }
    if tools.contains("Edit") {
        lines.push("- Use Edit for modifying files (not sed/awk)");
    }
    if tools.contains("Write") {
        lines.push("- Use Write for creating files (not echo/heredoc)");
    }
    if tools.contains("Grep") {
        lines.push("- Use Grep for searching file contents (not grep/rg)");
    }
    if tools.contains("Glob") {
        lines.push("- Use Glob for finding files by pattern (not find/ls)");
    }
    if tools.contains("LS") {
        lines.push("- Use LS for directory listing (not Bash ls)");
    }
    if lines.is_empty() {
        String::new()
    } else {
        lines.join("\n")
    }
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
