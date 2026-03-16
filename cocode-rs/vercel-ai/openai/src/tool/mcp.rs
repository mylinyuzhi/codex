use std::collections::HashMap;

use serde_json::json;
use vercel_ai_provider::LanguageModelV4ProviderTool;

/// Create an mcp provider tool for the Responses API.
///
/// # Arguments
/// - `server_label` - Label identifying the MCP server
/// - `server_url` - URL of the MCP server
/// - `allowed_tools` - Optional list of allowed tool names
/// - `headers` - Optional HTTP headers for the MCP connection
/// - `require_approval` - Optional approval mode ("always", "never", or specific tool names)
pub fn openai_mcp_tool(
    server_label: &str,
    server_url: &str,
    allowed_tools: Option<Vec<String>>,
    headers: Option<HashMap<String, String>>,
    require_approval: Option<&str>,
) -> LanguageModelV4ProviderTool {
    let mut args: HashMap<String, serde_json::Value> = HashMap::new();
    args.insert("server_label".into(), json!(server_label));
    args.insert("server_url".into(), json!(server_url));
    if let Some(tools) = allowed_tools {
        args.insert("allowed_tools".into(), json!(tools));
    }
    if let Some(hdrs) = headers {
        args.insert("headers".into(), json!(hdrs));
    }
    if let Some(approval) = require_approval {
        args.insert("require_approval".into(), json!(approval));
    }
    LanguageModelV4ProviderTool {
        id: "openai.mcp".into(),
        name: "mcp".into(),
        args,
    }
}
