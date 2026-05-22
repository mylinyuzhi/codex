//! MCPSearch tool for discovering deferred tools by keyword.
//!
//! Searches both MCP tools (when auto-search mode is active) and deferred
//! built-in tools, allowing on-demand discovery when their schemas are not
//! included in every API request.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;
use tokio::sync::Mutex;

use crate::context::ToolContext;
use crate::error::ToolError;
use crate::registry::McpToolInfo;
use crate::tool::Tool;
use cocode_protocol::ConcurrencySafety;
use cocode_protocol::ContextModifier;
use cocode_protocol::ToolOutput;

/// Searchable metadata for a deferred built-in tool.
#[derive(Debug, Clone)]
pub struct DeferredToolInfo {
    pub name: String,
    pub description: String,
}

/// MCPSearch tool for discovering deferred tools by keyword.
///
/// Searches both MCP tools and deferred built-in tools. When the full
/// tool list exceeds the context budget or tools are deferred to save
/// tokens, the LLM can call this tool to search by keyword and restore
/// matching tools for use.
pub struct McpSearchTool {
    /// Shared reference to available MCP tool metadata.
    mcp_tools: Arc<Mutex<Vec<McpToolInfo>>>,
    /// Deferred built-in tools (read-only after construction).
    deferred_builtin: Arc<Vec<DeferredToolInfo>>,
}

impl McpSearchTool {
    /// Create a new MCPSearch tool with a shared reference to MCP tool metadata.
    pub fn new(mcp_tools: Arc<Mutex<Vec<McpToolInfo>>>) -> Self {
        Self {
            mcp_tools,
            deferred_builtin: Arc::new(Vec::new()),
        }
    }

    /// Create with both MCP and built-in deferred tool metadata.
    pub fn with_deferred_builtin(
        mcp_tools: Arc<Mutex<Vec<McpToolInfo>>>,
        deferred_builtin: Arc<Vec<DeferredToolInfo>>,
    ) -> Self {
        Self {
            mcp_tools,
            deferred_builtin,
        }
    }
}

#[async_trait]
impl Tool for McpSearchTool {
    fn name(&self) -> &str {
        cocode_protocol::ToolName::McpSearch.as_str()
    }

    fn description(&self) -> &str {
        "Fetches full schema definitions for deferred tools so they can be called.\n\n\
         Deferred tools appear by name in <system-reminder> messages. Until fetched, \
         only the name is known. This tool takes a query, matches it against the \
         deferred tool list, and returns the matched tools' complete definitions."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Query to find deferred tools. Use \"select:<tool_name>\" for direct selection, or keywords to search."
                },
                "max_results": {
                    "type": "integer",
                    "description": "Maximum number of results to return (default: 5)",
                    "default": 5
                },
                "server": {
                    "type": "string",
                    "description": "Optional server name to filter MCP results"
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
        let query_raw = input.get("query").and_then(|v| v.as_str()).unwrap_or("");

        let max_results = input
            .get("max_results")
            .and_then(Value::as_i64)
            .unwrap_or(5) as usize;

        let server_filter = input
            .get("server")
            .and_then(|v| v.as_str())
            .map(std::string::ToString::to_string);

        // Support "select:Tool1,Tool2" syntax for direct selection
        let (is_select, query) = if let Some(names) = query_raw.strip_prefix("select:") {
            (true, names.to_string())
        } else {
            (false, query_raw.to_lowercase())
        };

        let mut all_names: Vec<String> = Vec::new();
        let mut output = String::new();

        // Search MCP tools
        {
            let tools = self.mcp_tools.lock().await;
            let mut mcp_matches: Vec<&McpToolInfo> = if is_select {
                let select_names: Vec<&str> = query.split(',').map(str::trim).collect();
                tools
                    .iter()
                    .filter(|t| {
                        select_names.iter().any(|n| {
                            t.qualified_name().eq_ignore_ascii_case(n)
                                || t.name.eq_ignore_ascii_case(n)
                        })
                    })
                    .collect()
            } else {
                tools
                    .iter()
                    .filter(|t| {
                        if let Some(ref server) = server_filter
                            && &t.server != server
                        {
                            return false;
                        }
                        let name_match = t.name.to_lowercase().contains(&query)
                            || t.qualified_name().to_lowercase().contains(&query);
                        let desc_match = t
                            .description
                            .as_deref()
                            .map(|d| d.to_lowercase().contains(&query))
                            .unwrap_or(false);
                        name_match || desc_match
                    })
                    .collect()
            };

            mcp_matches.sort_by(|a, b| {
                let a_name = a.name.to_lowercase().contains(&query);
                let b_name = b.name.to_lowercase().contains(&query);
                b_name.cmp(&a_name)
            });

            for tool in mcp_matches.iter().take(max_results) {
                all_names.push(tool.qualified_name());
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
        }

        // Search deferred built-in tools
        {
            let builtin = &self.deferred_builtin;
            let mut builtin_matches: Vec<&DeferredToolInfo> = if is_select {
                let select_names: Vec<&str> = query.split(',').map(str::trim).collect();
                builtin
                    .iter()
                    .filter(|t| select_names.iter().any(|n| t.name.eq_ignore_ascii_case(n)))
                    .collect()
            } else {
                builtin
                    .iter()
                    .filter(|t| {
                        let name_match = t.name.to_lowercase().contains(&query);
                        let desc_match = t.description.to_lowercase().contains(&query);
                        name_match || desc_match
                    })
                    .collect()
            };

            builtin_matches.sort_by(|a, b| {
                let a_name = a.name.to_lowercase().contains(&query);
                let b_name = b.name.to_lowercase().contains(&query);
                b_name.cmp(&a_name)
            });

            let remaining = max_results.saturating_sub(all_names.len());
            for tool in builtin_matches.iter().take(remaining) {
                all_names.push(tool.name.clone());
                output.push_str(&format!("## {}\n", tool.name));
                output.push_str(&format!("Description: {}\n\n", tool.description));
            }
        }

        if all_names.is_empty() {
            return Ok(ToolOutput::text(format!(
                "No deferred tools found matching query: \"{query_raw}\". \
                 Try a different search term.",
            )));
        }

        let header = format!(
            "Found {} deferred tool(s) matching \"{query_raw}\":\n\n",
            all_names.len()
        );
        output.insert_str(0, &header);

        Ok(ToolOutput::text(output)
            .with_modifier(ContextModifier::RestoreDeferredMcpTools { names: all_names }))
    }
}

#[cfg(test)]
#[path = "mcp_search.test.rs"]
mod tests;
