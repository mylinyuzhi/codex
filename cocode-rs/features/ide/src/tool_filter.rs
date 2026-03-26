//! IDE MCP tool filtering.
//!
//! IDE extensions expose tools with the `mcp__ide__` prefix. Most are internal
//! (called directly by cocode, not by the AI model). Only a small allowlist
//! of tools is exposed to the model in the tool registry.

/// Prefix for all IDE MCP tools (defined by IDE extension protocol, not our enum).
const IDE_TOOL_PREFIX: &str = "mcp__ide__";

/// Tools from the IDE MCP server that are exposed to the AI model.
///
/// All other `mcp__ide__` tools are internal and called directly by cocode.
/// These names are defined by the IDE extension protocol (Claude Code / VS Code
/// extension), not by our internal `ToolName` enum.
const AI_VISIBLE_TOOLS: &[&str] = &["mcp__ide__executeCode", "mcp__ide__getDiagnostics"];

/// Check if an MCP tool should be exposed to the AI model.
///
/// Returns `true` for non-IDE tools (always exposed) and for IDE tools
/// that are in the allowlist. Returns `false` for internal IDE tools.
pub fn should_expose_to_model(tool_name: &str) -> bool {
    if !tool_name.starts_with(IDE_TOOL_PREFIX) {
        return true;
    }
    AI_VISIBLE_TOOLS.contains(&tool_name)
}

/// Check if a tool name belongs to the IDE MCP server.
pub fn is_ide_tool(tool_name: &str) -> bool {
    tool_name.starts_with(IDE_TOOL_PREFIX)
}

#[cfg(test)]
#[path = "tool_filter.test.rs"]
mod tests;
