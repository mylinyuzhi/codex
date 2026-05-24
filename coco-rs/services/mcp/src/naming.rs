//! Server/tool name normalization.
//!
//! TS: services/mcp/normalization.ts + mcpStringUtils.ts
//! Naming convention: "mcp__<normalized_server>__<normalized_tool>" for ToolId.

use coco_types::MCP_TOOL_PREFIX;
use coco_types::MCP_TOOL_SEPARATOR;

/// Prefix identifying claude.ai-hosted MCP servers.
/// These get extra normalization (consecutive underscores collapsed).
const CLAUDEAI_SERVER_PREFIX: &str = "claude.ai ";

/// Normalize a server or tool name for MCP wire format.
///
/// Replaces any character outside `[a-zA-Z0-9_-]` with underscore.
/// For claude.ai servers: also collapses consecutive underscores and strips
/// leading/trailing underscores to prevent interference with the `__` delimiter.
///
/// TS: `normalizeNameForMCP()` in normalization.ts
pub fn normalize_name_for_mcp(name: &str, is_claudeai: bool) -> String {
    let mut normalized = String::with_capacity(name.len());
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
            normalized.push(ch);
        } else {
            normalized.push('_');
        }
    }

    if is_claudeai {
        // Collapse consecutive underscores
        let mut collapsed = String::with_capacity(normalized.len());
        let mut prev_underscore = false;
        for ch in normalized.chars() {
            if ch == '_' {
                if !prev_underscore {
                    collapsed.push(ch);
                }
                prev_underscore = true;
            } else {
                collapsed.push(ch);
                prev_underscore = false;
            }
        }
        // Strip leading/trailing underscores
        collapsed.trim_matches('_').to_string()
    } else {
        normalized
    }
}

/// Construct an MCP tool ID string from server and tool names.
///
/// Names are normalized before construction to ensure wire-format compatibility.
/// TS: `buildMcpToolName()` in mcpStringUtils.ts
pub fn mcp_tool_id(server: &str, tool: &str) -> String {
    let is_claudeai = server.starts_with(CLAUDEAI_SERVER_PREFIX);
    let norm_server = normalize_name_for_mcp(server, is_claudeai);
    let norm_tool = normalize_name_for_mcp(tool, is_claudeai);
    format!("{MCP_TOOL_PREFIX}{norm_server}{MCP_TOOL_SEPARATOR}{norm_tool}")
}

/// Construct an MCP tool ID without normalizing names.
///
/// Use when server/tool names are already normalized (e.g. from parsed ToolId).
pub fn mcp_tool_id_raw(server: &str, tool: &str) -> String {
    format!("{MCP_TOOL_PREFIX}{server}{MCP_TOOL_SEPARATOR}{tool}")
}

/// Get the MCP prefix for a server (normalized), e.g. `"mcp__slack__"`.
///
/// TS: `getMcpPrefix()` in mcpStringUtils.ts
pub fn mcp_prefix(server: &str) -> String {
    let is_claudeai = server.starts_with(CLAUDEAI_SERVER_PREFIX);
    let norm = normalize_name_for_mcp(server, is_claudeai);
    format!("{MCP_TOOL_PREFIX}{norm}{MCP_TOOL_SEPARATOR}")
}

/// Get the display name of an MCP tool by stripping the server prefix.
///
/// TS: `getMcpDisplayName()` in mcpStringUtils.ts
pub fn mcp_display_name(full_name: &str, server: &str) -> String {
    let prefix = mcp_prefix(server);
    full_name
        .strip_prefix(&prefix)
        .unwrap_or(full_name)
        .to_string()
}

/// Parse an MCP tool ID string into (server, tool) components.
/// Returns None if the string doesn't match "mcp__<server>__<tool>".
///
/// TS: `mcpInfoFromString()` in mcpStringUtils.ts
pub fn parse_mcp_tool_id(id: &str) -> Option<(String, String)> {
    let rest = id.strip_prefix(MCP_TOOL_PREFIX)?;
    let (server, tool) = rest.split_once(MCP_TOOL_SEPARATOR)?;
    Some((server.to_string(), tool.to_string()))
}

/// Generate a short request ID from a tool_use_id.
pub fn short_request_id(tool_use_id: &str) -> String {
    if tool_use_id.len() > 8 {
        tool_use_id[..8].to_string()
    } else {
        tool_use_id.to_string()
    }
}

#[cfg(test)]
#[path = "naming.test.rs"]
mod tests;
