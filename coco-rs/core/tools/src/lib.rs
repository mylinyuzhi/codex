//! All 42 built-in tool implementations (41 static + MCPTool dynamic wrapper).
//!
//! Each tool implements the `coco_tool::Tool` trait.
//! This crate provides the implementations; coco-tool defines the interface.

use std::path::Path;

pub mod input_types;
pub mod tools;

pub use input_types::ConfigAction;
pub use input_types::GrepOutputMode;
pub use input_types::LspAction;

// Re-export all tools
pub use tools::*;

/// Register all built-in tools into a ToolRegistry.
///
/// MCPTool instances are registered separately via `register_mcp_tools()`
/// after MCP servers connect and report their tools.
pub fn register_all_tools(registry: &mut coco_tool::ToolRegistry) {
    use std::sync::Arc;

    // File I/O (7)
    registry.register(Arc::new(BashTool));
    registry.register(Arc::new(ReadTool));
    registry.register(Arc::new(WriteTool));
    registry.register(Arc::new(EditTool));
    registry.register(Arc::new(GlobTool));
    registry.register(Arc::new(GrepTool));
    registry.register(Arc::new(NotebookEditTool));

    // Web (2)
    registry.register(Arc::new(WebFetchTool));
    registry.register(Arc::new(WebSearchTool));

    // Agent & Team (5)
    registry.register(Arc::new(AgentTool));
    registry.register(Arc::new(SkillTool));
    registry.register(Arc::new(SendMessageTool));
    registry.register(Arc::new(TeamCreateTool));
    registry.register(Arc::new(TeamDeleteTool));

    // Task Management (7)
    registry.register(Arc::new(TaskCreateTool));
    registry.register(Arc::new(TaskGetTool));
    registry.register(Arc::new(TaskListTool));
    registry.register(Arc::new(TaskUpdateTool));
    registry.register(Arc::new(TaskStopTool));
    registry.register(Arc::new(TaskOutputTool));
    registry.register(Arc::new(TodoWriteTool));

    // Plan & Worktree (4)
    registry.register(Arc::new(EnterPlanModeTool));
    registry.register(Arc::new(ExitPlanModeTool));
    registry.register(Arc::new(EnterWorktreeTool));
    registry.register(Arc::new(ExitWorktreeTool));

    // Utility (5)
    registry.register(Arc::new(AskUserQuestionTool));
    registry.register(Arc::new(ToolSearchTool));
    registry.register(Arc::new(ConfigTool));
    registry.register(Arc::new(BriefTool));
    registry.register(Arc::new(LspTool));

    // MCP management (3)
    registry.register(Arc::new(McpAuthTool));
    registry.register(Arc::new(ListMcpResourcesTool));
    registry.register(Arc::new(ReadMcpResourceTool));

    // Scheduling (4)
    registry.register(Arc::new(CronCreateTool));
    registry.register(Arc::new(CronDeleteTool));
    registry.register(Arc::new(CronListTool));
    registry.register(Arc::new(RemoteTriggerTool));

    // Shell (4)
    registry.register(Arc::new(PowerShellTool));
    registry.register(Arc::new(ReplTool));
    registry.register(Arc::new(SleepTool));
    registry.register(Arc::new(SyntheticOutputTool));
}

/// Register MCP server tools as dynamic McpTool wrappers.
///
/// Called when MCP servers connect and report their tools. Each MCP server
/// tool gets a McpTool wrapper registered in the ToolRegistry.
///
/// Handles reconnection: deregisters old tools from the server first,
/// then registers the new ones. Safe to call multiple times for the
/// same server (idempotent after deregister).
pub fn register_mcp_tools(
    registry: &mut coco_tool::ToolRegistry,
    server_name: &str,
    mcp_tools: Vec<coco_tool::McpToolSchema>,
) {
    use std::sync::Arc;

    // Deregister old tools from this server (handles reconnect)
    registry.deregister_by_server(server_name);

    for schema in mcp_tools {
        registry.register(Arc::new(McpTool::new(
            schema.server_name,
            schema.tool_name,
            schema.description.unwrap_or_default(),
            schema.input_schema,
            schema.annotations,
        )));
    }
}

/// Deregister all tools from a specific MCP server.
///
/// Called when an MCP server disconnects to clean up stale tools.
pub fn deregister_mcp_server(registry: &mut coco_tool::ToolRegistry, server_name: &str) {
    registry.deregister_by_server(server_name);
}

/// Record a file read in FileReadState for @mention dedup and changed-file detection.
pub(crate) async fn record_file_read(
    ctx: &coco_tool::ToolUseContext,
    path: &Path,
    content: String,
    offset: Option<i32>,
    limit: Option<i32>,
) {
    if let Some(frs) = &ctx.file_read_state {
        if let Ok(abs_path) = std::fs::canonicalize(path) {
            if let Ok(mtime) = coco_context::file_mtime_ms(&abs_path).await {
                let mut frs = frs.write().await;
                frs.set(
                    abs_path,
                    coco_context::FileReadEntry {
                        content,
                        mtime_ms: mtime,
                        offset,
                        limit,
                    },
                );
            }
        }
    }
}

/// Record a file edit in FileReadState (new content, new mtime, clears offset/limit).
pub(crate) async fn record_file_edit(
    ctx: &coco_tool::ToolUseContext,
    path: &Path,
    new_content: String,
) {
    if let Some(frs) = &ctx.file_read_state {
        if let Ok(abs_path) = std::fs::canonicalize(path) {
            if let Ok(mtime) = coco_context::file_mtime_ms(&abs_path).await {
                let mut frs = frs.write().await;
                frs.update_after_edit(&abs_path, new_content, mtime);
            }
        }
    }
}

/// Track a file edit for checkpoint/rewind before modifying.
///
/// TS: `fileHistoryTrackEdit()` — called from FileEditTool, FileWriteTool,
/// NotebookEditTool, BashTool before file modifications.
/// Silently no-ops if file history is not configured on the context.
pub(crate) async fn track_file_edit(ctx: &coco_tool::ToolUseContext, path: &Path) {
    if let (Some(fh), Some(config_home), Some(sid)) = (
        &ctx.file_history,
        &ctx.config_home,
        &ctx.session_id_for_history,
    ) {
        // Use user_message_id (the originating user message UUID), NOT tool_use_id.
        // TS: fileHistoryTrackEdit(filePath, parentMessage.uuid)
        if let Some(msg_id) = &ctx.user_message_id {
            let mut fh = fh.write().await;
            if let Err(e) = fh.track_edit(path, msg_id, config_home, sid).await {
                tracing::warn!("file history track_edit failed: {e}");
            }
        }
    }
}

/// Register only core tools (for lightweight setups).
pub fn register_core_tools(registry: &mut coco_tool::ToolRegistry) {
    use std::sync::Arc;
    registry.register(Arc::new(BashTool));
    registry.register(Arc::new(ReadTool));
    registry.register(Arc::new(WriteTool));
    registry.register(Arc::new(EditTool));
    registry.register(Arc::new(GlobTool));
    registry.register(Arc::new(GrepTool));
}
