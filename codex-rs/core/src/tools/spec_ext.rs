//! Extension for tool loading logging
//!
//! Provides utilities to log loaded tools with metadata like shell variants
//! and execution modes. Also provides ext tool registration to minimize
//! modifications to spec.rs for easier upstream sync.

use crate::client_common::tools::ToolSpec;
use crate::tools::registry::ConfiguredToolSpec;
use crate::tools::registry::ToolRegistryBuilder;
use crate::tools::spec::ToolsConfig;
use std::sync::Arc;
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

/// Try to register rich grep (ripgrep). Returns true if registered.
/// If false, caller should register the original grep_files handler.
pub fn try_register_rich_grep(builder: &mut ToolRegistryBuilder, config: &ToolsConfig) -> bool {
    if config.include_rich_grep {
        use crate::tools::ext::ripgrep::create_ripgrep_tool;
        use crate::tools::handlers::ext::ripgrep::RipGrepHandler;
        builder.push_spec_with_parallel_support(create_ripgrep_tool(), true);
        builder.register_handler("grep_files", Arc::new(RipGrepHandler));
        true
    } else {
        false
    }
}

/// Try to register enhanced list_dir. Returns true if registered.
/// If false, caller should register the original list_dir handler.
pub fn try_register_enhanced_list_dir(
    builder: &mut ToolRegistryBuilder,
    config: &ToolsConfig,
) -> bool {
    if config.include_enhanced_list_dir {
        use crate::tools::ext::list_dir::create_enhanced_list_dir_tool;
        use crate::tools::handlers::ext::list_dir::EnhancedListDirHandler;
        builder.push_spec_with_parallel_support(create_enhanced_list_dir_tool(), true);
        builder.register_handler("list_dir", Arc::new(EnhancedListDirHandler));
        true
    } else {
        false
    }
}

/// Register smart_edit tool if enabled.
pub fn register_smart_edit(builder: &mut ToolRegistryBuilder, config: &ToolsConfig) {
    if config.include_smart_edit {
        use crate::tools::ext::smart_edit::create_smart_edit_tool;
        use crate::tools::handlers::ext::smart_edit::SmartEditHandler;
        builder.push_spec(create_smart_edit_tool());
        builder.register_handler("smart_edit", Arc::new(SmartEditHandler));
    }
}

/// Register glob_files tool (always enabled).
pub fn register_glob_files(builder: &mut ToolRegistryBuilder) {
    use crate::tools::ext::glob_files::create_glob_files_tool;
    use crate::tools::handlers::ext::glob_files::GlobFilesHandler;
    builder.push_spec_with_parallel_support(create_glob_files_tool(), true);
    builder.register_handler("glob_files", Arc::new(GlobFilesHandler));
}

/// Register think tool (always enabled for all models).
///
/// Think is a no-op tool that logs thoughts for transparency.
/// Useful for complex reasoning, brainstorming, and planning.
pub fn register_think(builder: &mut ToolRegistryBuilder) {
    use crate::tools::ext::think::create_think_tool;
    use crate::tools::handlers::ext::think::ThinkHandler;
    builder.push_spec_with_parallel_support(create_think_tool(), true);
    builder.register_handler("think", Arc::new(ThinkHandler));
}

/// Register write_file tool (always enabled for all models).
///
/// Write File creates new files or overwrites existing files.
/// This is a mutating tool that requires approval.
pub fn register_write_file(builder: &mut ToolRegistryBuilder) {
    use crate::tools::ext::write_file::create_write_file_tool;
    use crate::tools::handlers::ext::write_file::WriteFileHandler;
    builder.push_spec(create_write_file_tool());
    builder.register_handler("write_file", Arc::new(WriteFileHandler));
}

/// Register web_fetch tool if feature is enabled.
///
/// Web Fetch fetches content from URLs and converts HTML to plain text.
/// This is a mutating tool that requires approval.
pub fn register_web_fetch(builder: &mut ToolRegistryBuilder, config: &ToolsConfig) {
    if config.include_web_fetch {
        use crate::tools::ext::web_fetch::create_web_fetch_tool;
        use crate::tools::handlers::ext::web_fetch::WebFetchHandler;
        builder.push_spec_with_parallel_support(create_web_fetch_tool(), true);
        builder.register_handler("web_fetch", Arc::new(WebFetchHandler));
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
