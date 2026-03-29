//! MCP server testing CLI subcommand.
//!
//! Provides tools for debugging MCP server connectivity:
//! - `list-tools`: List available tools from a server
//! - `call-tool`: Call a specific tool with arguments
//! - `inspect`: Show server capabilities and metadata

use std::ffi::OsString;
use std::time::Duration;

use clap::Subcommand;
use cocode_rmcp_client::RmcpClient;

/// MCP CLI actions.
#[derive(Subcommand)]
pub enum McpAction {
    /// List available tools from an MCP server.
    ListTools {
        /// Server command for stdio transport (e.g., "node server.js").
        #[arg(long)]
        server: String,
    },
    /// Call a specific tool on an MCP server.
    CallTool {
        /// Server command for stdio transport (e.g., "node server.js").
        #[arg(long)]
        server: String,
        /// Tool name to call.
        #[arg(long)]
        tool: String,
        /// JSON arguments for the tool.
        #[arg(long, default_value = "{}")]
        args: String,
    },
    /// Show server capabilities and metadata.
    Inspect {
        /// Server command for stdio transport (e.g., "node server.js").
        #[arg(long)]
        server: String,
    },
}

/// Run an MCP CLI action.
pub async fn run(action: McpAction) -> anyhow::Result<()> {
    match action {
        McpAction::ListTools { server } => {
            let client = connect_stdio(&server).await?;
            initialize_client(&client).await?;
            let result = client
                .list_tools(None, Some(Duration::from_secs(30)))
                .await?;
            for tool in &result.tools {
                println!(
                    "  {} - {}",
                    tool.name,
                    tool.description.as_deref().unwrap_or("")
                );
            }
            println!("\n{} tool(s) available.", result.tools.len());
            Ok(())
        }
        McpAction::CallTool { server, tool, args } => {
            let client = connect_stdio(&server).await?;
            initialize_client(&client).await?;
            let arguments: serde_json::Value = serde_json::from_str(&args)?;
            let result = client
                .call_tool(tool, Some(arguments), Some(Duration::from_secs(60)))
                .await?;
            println!("{}", serde_json::to_string_pretty(&result)?);
            Ok(())
        }
        McpAction::Inspect { server } => {
            let client = connect_stdio(&server).await?;
            let init_result = initialize_client(&client).await?;
            println!("Server: {}", init_result.server_info.name);
            println!("Version: {}", init_result.server_info.version);
            println!("Protocol: {}", init_result.protocol_version);
            println!(
                "Capabilities: {}",
                serde_json::to_string_pretty(&init_result.capabilities)?
            );
            let tools = client
                .list_tools(None, Some(Duration::from_secs(30)))
                .await?;
            println!("Tools: {}", tools.tools.len());
            for tool in &tools.tools {
                println!(
                    "  {} - {}",
                    tool.name,
                    tool.description.as_deref().unwrap_or("")
                );
            }
            Ok(())
        }
    }
}

/// Connect to an MCP server via stdio transport.
async fn connect_stdio(server_cmd: &str) -> anyhow::Result<RmcpClient> {
    let parts: Vec<&str> = server_cmd.split_whitespace().collect();
    let (program, args) = parts
        .split_first()
        .ok_or_else(|| anyhow::anyhow!("Empty server command"))?;

    let os_args: Vec<OsString> = args.iter().map(OsString::from).collect();
    let client = RmcpClient::new_stdio_client(
        OsString::from(program),
        os_args,
        /*env=*/ None,
        /*env_vars=*/ &[],
        /*cwd=*/ None,
    )
    .await?;
    Ok(client)
}

/// Initialize the MCP client with default params and no elicitation handler.
async fn initialize_client(
    client: &RmcpClient,
) -> anyhow::Result<cocode_mcp_types::InitializeResult> {
    let params = cocode_mcp_types::InitializeRequestParams {
        protocol_version: "2025-06-18".to_string(),
        capabilities: cocode_mcp_types::ClientCapabilities {
            elicitation: None,
            experimental: None,
            roots: None,
            sampling: None,
        },
        client_info: cocode_mcp_types::Implementation {
            name: "cocode-mcp-cli".to_string(),
            title: None,
            version: env!("CARGO_PKG_VERSION").to_string(),
            user_agent: None,
        },
    };
    // No-op elicitation handler for CLI testing
    let send_elicitation: cocode_rmcp_client::SendElicitation = Box::new(|_id, _params| {
        Box::pin(async { Err(anyhow::anyhow!("Elicitation not supported in CLI mode")) })
    });
    let result = client
        .initialize(params, Some(Duration::from_secs(30)), send_elicitation)
        .await?;
    Ok(result)
}

#[cfg(test)]
#[path = "mcp.test.rs"]
mod tests;
