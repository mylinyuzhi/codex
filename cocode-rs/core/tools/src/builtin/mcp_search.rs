//! MCPSearch tool for discovering MCP tools by keyword.
//!
//! When the full MCP tool list exceeds the context budget,
//! this tool is registered instead to allow on-demand discovery.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;
use tokio::sync::Mutex;

use crate::context::ToolContext;
use crate::error::ToolError;
use crate::registry::McpToolInfo;
use crate::tool::Tool;
use cocode_protocol::ConcurrencySafety;
use cocode_protocol::ToolOutput;

/// MCPSearch tool for discovering MCP tools by keyword.
///
/// When the full MCP tool list exceeds the context budget,
/// this tool is registered instead to allow on-demand discovery.
/// The LLM can call this tool to search MCP tool names and descriptions
/// by keyword, returning matching tool schemas for use.
pub struct McpSearchTool {
    /// Shared reference to available MCP tool metadata.
    mcp_tools: Arc<Mutex<Vec<McpToolInfo>>>,
}

impl McpSearchTool {
    /// Create a new MCPSearch tool with a shared reference to MCP tool metadata.
    pub fn new(mcp_tools: Arc<Mutex<Vec<McpToolInfo>>>) -> Self {
        Self { mcp_tools }
    }
}

#[async_trait]
impl Tool for McpSearchTool {
    fn name(&self) -> &str {
        "MCPSearch"
    }

    fn description(&self) -> &str {
        "Search for MCP tools by keyword when the full tool list exceeds context budget. \
         Returns matching tool names, descriptions, and input schemas."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search query to match against tool names and descriptions"
                },
                "server": {
                    "type": "string",
                    "description": "Optional server name to filter results"
                }
            },
            "required": ["query"]
        })
    }

    fn concurrency_safety(&self) -> ConcurrencySafety {
        ConcurrencySafety::Safe
    }

    fn is_read_only(&self) -> bool {
        true
    }

    async fn execute(&self, input: Value, _ctx: &mut ToolContext) -> Result<ToolOutput, ToolError> {
        let query = input
            .get("query")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_lowercase();

        let server_filter = input
            .get("server")
            .and_then(|v| v.as_str())
            .map(std::string::ToString::to_string);

        let tools = self.mcp_tools.lock().await;

        let mut matches: Vec<&McpToolInfo> = tools
            .iter()
            .filter(|t| {
                // Filter by server if specified
                if let Some(ref server) = server_filter
                    && &t.server != server
                {
                    return false;
                }

                // Match against name and description
                let name_match = t.name.to_lowercase().contains(&query)
                    || t.qualified_name().to_lowercase().contains(&query);
                let desc_match = t
                    .description
                    .as_deref()
                    .map(|d| d.to_lowercase().contains(&query))
                    .unwrap_or(false);

                name_match || desc_match
            })
            .collect();

        // Sort by relevance: name matches first, then description matches
        matches.sort_by(|a, b| {
            let a_name_match = a.name.to_lowercase().contains(&query);
            let b_name_match = b.name.to_lowercase().contains(&query);
            b_name_match.cmp(&a_name_match)
        });

        if matches.is_empty() {
            return Ok(ToolOutput::text(format!(
                "No MCP tools found matching query: \"{query}\". Try a different search term.",
            )));
        }

        let mut output = format!(
            "Found {} MCP tool(s) matching \"{query}\":\n\n",
            matches.len()
        );
        for tool in &matches {
            output.push_str(&format!("## {}\n", tool.qualified_name()));
            output.push_str(&format!("Server: {}\n", tool.server));
            if let Some(desc) = &tool.description {
                output.push_str(&format!("Description: {desc}\n"));
            }
            output.push_str(&format!(
                "Schema: {}\n\n",
                serde_json::to_string_pretty(&tool.input_schema).unwrap_or_default()
            ));
        }

        Ok(ToolOutput::text(output))
    }
}

#[cfg(test)]
#[path = "mcp_search.test.rs"]
mod tests;
