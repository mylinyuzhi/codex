//! Extension for tool loading logging
//!
//! Provides utilities to log loaded tools with metadata like shell variants
//! and execution modes.

use crate::client_common::tools::ToolSpec;
use crate::tools::registry::ConfiguredToolSpec;
use tracing::info;

/// Log loaded tools with variant annotations
pub fn log_loaded_tools(tools: &[ConfiguredToolSpec], model: &str) {
    let tool_displays: Vec<String> = tools
        .iter()
        .map(|tool| {
            let name = tool_name(&tool.spec);
            if let Some(variant) = tool_variant(&tool.spec) {
                format!("{name}[{variant}]")
            } else {
                name.to_string()
            }
        })
        .collect();

    info!(
        "[{model}] Loaded {} tools: [{}]",
        tools.len(),
        tool_displays.join(", ")
    );
}

/// Extract tool name from ToolSpec
fn tool_name(tool: &ToolSpec) -> &str {
    match tool {
        ToolSpec::Function(t) => &t.name,
        ToolSpec::LocalShell {} => "local_shell",
        ToolSpec::WebSearch {} => "web_search",
        ToolSpec::Freeform(t) => &t.name,
    }
}

/// Determine tool variant/execution mode annotation
fn tool_variant(tool: &ToolSpec) -> Option<&'static str> {
    match tool {
        ToolSpec::Function(t) => {
            match t.name.as_str() {
                // Shell variants
                "shell" => Some("array"),
                "shell_command" => Some("string"),
                "exec_command" => Some("PTY"),
                "write_stdin" => Some("PTY"),

                // MCP tools (server/tool_name format)
                name if name.contains('/') => Some("mcp"),

                // No variant annotation for other tools
                _ => None,
            }
        }
        ToolSpec::LocalShell {} => Some("API"),
        ToolSpec::WebSearch {} => Some("API"),
        ToolSpec::Freeform(_) => Some("freeform"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client_common::tools::ResponsesApiTool;
    use crate::tools::spec::JsonSchema;

    #[test]
    fn test_tool_variant_detection() {
        let shell = ToolSpec::Function(ResponsesApiTool {
            name: "shell".to_string(),
            description: "".to_string(),
            strict: false,
            parameters: JsonSchema::String { description: None },
        });
        assert_eq!(tool_variant(&shell), Some("array"));

        let shell_command = ToolSpec::Function(ResponsesApiTool {
            name: "shell_command".to_string(),
            description: "".to_string(),
            strict: false,
            parameters: JsonSchema::String { description: None },
        });
        assert_eq!(tool_variant(&shell_command), Some("string"));

        let exec_command = ToolSpec::Function(ResponsesApiTool {
            name: "exec_command".to_string(),
            description: "".to_string(),
            strict: false,
            parameters: JsonSchema::String { description: None },
        });
        assert_eq!(tool_variant(&exec_command), Some("PTY"));

        let local_shell = ToolSpec::LocalShell {};
        assert_eq!(tool_variant(&local_shell), Some("API"));

        let web_search = ToolSpec::WebSearch {};
        assert_eq!(tool_variant(&web_search), Some("API"));

        let mcp_tool = ToolSpec::Function(ResponsesApiTool {
            name: "github/create_pr".to_string(),
            description: "".to_string(),
            strict: false,
            parameters: JsonSchema::String { description: None },
        });
        assert_eq!(tool_variant(&mcp_tool), Some("mcp"));

        let regular_tool = ToolSpec::Function(ResponsesApiTool {
            name: "update_plan".to_string(),
            description: "".to_string(),
            strict: false,
            parameters: JsonSchema::String { description: None },
        });
        assert_eq!(tool_variant(&regular_tool), None);
    }

    #[test]
    fn test_tool_name_extraction() {
        let function = ToolSpec::Function(ResponsesApiTool {
            name: "test_tool".to_string(),
            description: "".to_string(),
            strict: false,
            parameters: JsonSchema::String { description: None },
        });
        assert_eq!(tool_name(&function), "test_tool");

        assert_eq!(tool_name(&ToolSpec::LocalShell {}), "local_shell");
        assert_eq!(tool_name(&ToolSpec::WebSearch {}), "web_search");
    }
}
