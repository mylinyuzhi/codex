//! MCP tool call handling.
//!
//! TS: core mcp_tool_call.ts — routes tool calls to MCP servers.

use crate::types::McpToolDefinition;
use coco_types::MCP_TOOL_PREFIX;
use coco_types::MCP_TOOL_SEPARATOR;
use serde::Deserialize;
use serde::Serialize;

/// Maximum tool description length.
const MAX_DESCRIPTION_LENGTH: usize = 2048;

/// Result of an MCP tool call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpToolCallResult {
    pub tool_name: String,
    pub server_name: String,
    pub content: Vec<McpToolContent>,
    pub is_error: bool,
    pub duration_ms: i64,
}

/// Content block in an MCP tool result.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum McpToolContent {
    Text { text: String },
    Image { data: String, mime_type: String },
    Resource { uri: String, text: Option<String> },
}

/// Truncate a tool description to the maximum length with a suffix marker.
pub fn truncate_description(description: &str) -> String {
    if description.len() <= MAX_DESCRIPTION_LENGTH {
        description.to_string()
    } else {
        format!("{}... [truncated]", &description[..MAX_DESCRIPTION_LENGTH])
    }
}

/// Prepare an MCP tool definition for the LLM (truncate description, validate schema).
pub fn prepare_tool_for_llm(tool: &McpToolDefinition) -> McpToolDefinition {
    let mut prepared = tool.clone();
    if let Some(ref desc) = prepared.description {
        if desc.len() > MAX_DESCRIPTION_LENGTH {
            prepared.description = Some(format!(
                "{}... (truncated)",
                &desc[..MAX_DESCRIPTION_LENGTH]
            ));
        }
    }
    prepared
}

/// Format an MCP tool call error for the model.
pub fn format_mcp_error(server: &str, tool: &str, error: &str) -> String {
    format!("MCP tool call failed (server={server}, tool={tool}): {error}")
}

/// Parse MCP tool name into (server_name, tool_name).
///
/// Format: "mcp__server__tool"
pub fn parse_mcp_tool_name(full_name: &str) -> Option<(&str, &str)> {
    let stripped = full_name.strip_prefix(MCP_TOOL_PREFIX)?;
    let parts: Vec<&str> = stripped.splitn(2, MCP_TOOL_SEPARATOR).collect();
    if parts.len() == 2 {
        Some((parts[0], parts[1]))
    } else {
        None
    }
}

/// Build the full MCP tool name from server and tool names.
///
/// Delegates to `naming::mcp_tool_id` which normalizes names for wire format.
pub fn build_mcp_tool_name(server: &str, tool: &str) -> String {
    crate::naming::mcp_tool_id(server, tool)
}

#[cfg(test)]
#[path = "tool_call.test.rs"]
mod tests;
