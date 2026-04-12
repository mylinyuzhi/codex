//! Enhanced MCP tool features ported from TS MCPTool/.
//!
//! TS: tools/MCPTool/MCPTool.ts, classifyForCollapse.ts, prompt.ts
//!
//! Provides MCP tool call execution with result formatting, resource content
//! fetching, tool schema discovery for deferred tools, and result truncation.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

use coco_types::{MCP_TOOL_PREFIX, MCP_TOOL_SEPARATOR};

/// Maximum result size for MCP tool output (100K chars, matching TS).
pub const MAX_MCP_RESULT_SIZE_CHARS: usize = 100_000;

/// Maximum number of content parts in a single MCP tool result.
const MAX_CONTENT_PARTS: usize = 50;

// ── MCP tool call types ──

/// Input for an MCP tool call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpToolCallInput {
    /// MCP server name (from ToolId::Mcp { server, .. }).
    pub server_name: String,
    /// Tool name on the MCP server.
    pub tool_name: String,
    /// Arguments as a JSON object (passed through to the MCP server).
    #[serde(default)]
    pub arguments: Value,
}

/// A single content part in an MCP tool result.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum McpContentPart {
    Text { text: String },
    Image { data: String, mime_type: String },
    Resource { uri: String, text: Option<String> },
}

/// Result from an MCP tool execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpToolResult {
    /// Content parts returned by the tool.
    pub content: Vec<McpContentPart>,
    /// Whether the tool execution produced an error.
    #[serde(default)]
    pub is_error: bool,
}

// ── MCP resource types ──

/// An MCP resource descriptor.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpResource {
    /// Resource URI (e.g., "file:///path" or custom scheme).
    pub uri: String,
    /// Human-readable name.
    pub name: String,
    /// Optional description.
    #[serde(default)]
    pub description: Option<String>,
    /// MIME type of the resource content.
    #[serde(default)]
    pub mime_type: Option<String>,
}

/// Content fetched from an MCP resource.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpResourceContent {
    pub uri: String,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub blob: Option<String>,
    #[serde(default)]
    pub mime_type: Option<String>,
}

// ── MCP tool schema discovery ──

/// Schema for a tool available on an MCP server (for deferred tool discovery).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpToolSchema {
    /// Tool name (as registered on the MCP server).
    pub name: String,
    /// Human-readable description.
    #[serde(default)]
    pub description: Option<String>,
    /// JSON schema for the tool's input parameters.
    #[serde(default)]
    pub input_schema: Option<Value>,
}

// ── Result formatting ──

/// Format an MCP tool result into a string suitable for model consumption.
///
/// Concatenates text content parts, handles images as placeholders, and
/// respects the max result size limit.
pub fn format_mcp_result(result: &McpToolResult) -> String {
    if result.content.is_empty() {
        return if result.is_error {
            "MCP tool execution failed with no output".to_string()
        } else {
            String::new()
        };
    }

    let mut output = String::new();
    let parts = if result.content.len() > MAX_CONTENT_PARTS {
        &result.content[..MAX_CONTENT_PARTS]
    } else {
        &result.content
    };

    for (i, part) in parts.iter().enumerate() {
        if i > 0 {
            output.push('\n');
        }
        match part {
            McpContentPart::Text { text } => output.push_str(text),
            McpContentPart::Image { mime_type, .. } => {
                output.push_str(&format!("[Image: {mime_type}]"));
            }
            McpContentPart::Resource { uri, text } => {
                if let Some(text) = text {
                    output.push_str(text);
                } else {
                    output.push_str(&format!("[Resource: {uri}]"));
                }
            }
        }
    }

    if result.content.len() > MAX_CONTENT_PARTS {
        let omitted = result.content.len() - MAX_CONTENT_PARTS;
        output.push_str(&format!("\n\n... [{omitted} content parts omitted]"));
    }

    truncate_mcp_output(output)
}

/// Format multiple MCP resource contents into a readable string.
pub fn format_resource_contents(contents: &[McpResourceContent]) -> String {
    if contents.is_empty() {
        return "No resource content available".to_string();
    }

    let mut output = String::new();
    for (i, content) in contents.iter().enumerate() {
        if i > 0 {
            output.push_str("\n---\n");
        }
        output.push_str(&format!("Resource: {}\n", content.uri));
        if let Some(mime) = &content.mime_type {
            output.push_str(&format!("Type: {mime}\n"));
        }
        if let Some(text) = &content.text {
            output.push_str(text);
        } else if content.blob.is_some() {
            output.push_str("[Binary content]");
        } else {
            output.push_str("[Empty]");
        }
    }

    truncate_mcp_output(output)
}

/// Format discovered tool schemas for deferred tool loading.
pub fn format_tool_schemas(schemas: &[McpToolSchema]) -> String {
    if schemas.is_empty() {
        return "No tools available on this MCP server".to_string();
    }

    let mut lines = vec![format!("Available tools ({}):", schemas.len())];
    for schema in schemas {
        let desc = schema
            .description
            .as_deref()
            .unwrap_or("No description");
        lines.push(format!("\n  {} - {desc}", schema.name));
        if let Some(input_schema) = &schema.input_schema {
            if let Some(props) = input_schema.get("properties") {
                if let Some(obj) = props.as_object() {
                    for (name, prop) in obj {
                        let prop_desc = prop
                            .get("description")
                            .and_then(Value::as_str)
                            .unwrap_or("");
                        let prop_type = prop
                            .get("type")
                            .and_then(Value::as_str)
                            .unwrap_or("any");
                        lines.push(format!("    {name} ({prop_type}): {prop_desc}"));
                    }
                }
            }
        }
    }
    lines.join("\n")
}

/// Truncate MCP output to max size with a trailing marker.
fn truncate_mcp_output(output: String) -> String {
    if output.len() <= MAX_MCP_RESULT_SIZE_CHARS {
        return output;
    }
    // Find last newline within budget to avoid cutting mid-line
    let cutoff = output[..MAX_MCP_RESULT_SIZE_CHARS]
        .rfind('\n')
        .unwrap_or(MAX_MCP_RESULT_SIZE_CHARS);
    let kept = &output[..cutoff];
    let remaining_lines = output[cutoff..].matches('\n').count();
    format!("{kept}\n\n... [{remaining_lines} lines truncated]")
}

/// Check if MCP output was truncated (for UI indication).
pub fn is_result_truncated(output: &str) -> bool {
    output.ends_with("truncated]")
}

/// Classify an MCP tool result for UI collapse behavior.
///
/// Returns a short label (e.g., "success", "error", "empty") that
/// the TUI uses to decide whether to collapse the result.
pub fn classify_for_collapse(result: &McpToolResult) -> &'static str {
    if result.is_error {
        return "error";
    }
    if result.content.is_empty() {
        return "empty";
    }
    // Check if result is trivially small (single short text)
    if result.content.len() == 1 {
        if let McpContentPart::Text { text } = &result.content[0] {
            if text.len() < 100 {
                return "brief";
            }
        }
    }
    "success"
}

/// Build the wire-format tool ID for an MCP tool.
pub fn mcp_tool_id(server: &str, tool: &str) -> String {
    format!("{MCP_TOOL_PREFIX}{server}{MCP_TOOL_SEPARATOR}{tool}")
}

/// Parse an MCP tool ID into (server, tool) components.
pub fn parse_mcp_tool_id(id: &str) -> Option<(&str, &str)> {
    let rest = id.strip_prefix(MCP_TOOL_PREFIX)?;
    rest.split_once(MCP_TOOL_SEPARATOR)
}

#[cfg(test)]
#[path = "mcp_advanced.test.rs"]
mod tests;
