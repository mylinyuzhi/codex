//! All 42 built-in tool implementations (41 static + MCPTool dynamic wrapper).
//!
//! Each tool implements the `coco_tool_runtime::Tool` trait.
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
pub fn register_all_tools(registry: &mut coco_tool_runtime::ToolRegistry) {
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
    registry: &mut coco_tool_runtime::ToolRegistry,
    server_name: &str,
    mcp_tools: Vec<coco_tool_runtime::McpToolSchema>,
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
pub fn deregister_mcp_server(registry: &mut coco_tool_runtime::ToolRegistry, server_name: &str) {
    registry.deregister_by_server(server_name);
}

/// Record a Read-tool file read in FileReadState for @mention dedup,
/// changed-file detection, and Read-tool `file_unchanged` dedup. Uses
/// `set_from_read` so the path is flagged as Read-origin — Edit/Write
/// entries don't get this flag, so dedup-aware readers skip stub-ing
/// against post-edit entries.
///
/// `effective_*` is the truncated range stored on `FileReadEntry` (None
/// = "no truncation in that dimension"). `input_*` is the literal value
/// the model passed; the dedup check matches against the model-visible
/// inputs, not the effective ones, so the two are kept separately.
pub(crate) async fn record_file_read(
    ctx: &coco_tool_runtime::ToolUseContext,
    path: &Path,
    content: String,
    effective_offset: Option<i32>,
    effective_limit: Option<i32>,
    input_offset: Option<i32>,
    input_limit: Option<i32>,
) {
    if let Some(frs) = &ctx.file_read_state
        && let Ok(abs_path) = tokio::fs::canonicalize(path).await
        && let Ok(mtime) = coco_context::file_mtime_ms(&abs_path).await
    {
        let mut frs = frs.write().await;
        frs.set_from_read(
            abs_path,
            coco_context::FileReadEntry {
                content,
                mtime_ms: mtime,
                offset: effective_offset,
                limit: effective_limit,
            },
            input_offset,
            input_limit,
        );
    }
}

/// Record a file edit in FileReadState (new content, new mtime, clears offset/limit).
pub(crate) async fn record_file_edit(
    ctx: &coco_tool_runtime::ToolUseContext,
    path: &Path,
    new_content: String,
) {
    if let Some(frs) = &ctx.file_read_state
        && let Ok(abs_path) = tokio::fs::canonicalize(path).await
        && let Ok(mtime) = coco_context::file_mtime_ms(&abs_path).await
    {
        let mut frs = frs.write().await;
        frs.update_after_edit(&abs_path, new_content, mtime);
    }
}

/// Check whether a write to `path` with the given `content` should be
/// blocked because it would leak a secret into team memory.
///
/// TS: `services/teamMemorySync/teamMemSecretGuard.ts:checkTeamMemSecrets`
/// — invoked from `FileWriteTool.ts:157` and `FileEditTool.ts` to reject
/// writes that put API keys / tokens / credentials into a team-memory
/// path (which would be synced to all repository collaborators).
///
/// Path detection is layered: first the authoritative resolution via
/// `coco_memory::team_paths::is_team_mem_path` (using
/// `MemoryConfig::resolve_memory_dir(project_root)` from the resolved
/// tool context config, then a substring fallback
/// for paths that don't match the resolved layout. See
/// `is_team_memory_path` for the gating logic.
///
/// Detection uses `coco_secret_redact::redact_secrets`, which returns a
/// `Cow::Borrowed` when nothing matched and `Cow::Owned` when at least
/// one secret was found. The return value is `Some(error_msg)` only
/// when secrets were detected; the caller surfaces this as a tool
/// error so the model can rewrite the content.
///
/// **Limitation**: this is a best-effort guard, not a security
/// boundary. The user can write secrets to any non-team-memory path
/// without triggering the check, and the regex set in
/// `coco-secret-redact` covers common patterns but isn't exhaustive.
/// The intent matches TS: prevent the most common accident of putting
/// `API_KEY=sk-...` into a synced memory file.
pub(crate) fn check_team_mem_secret(
    ctx: &coco_tool_runtime::ToolUseContext,
    path: &std::path::Path,
    content: &str,
) -> Option<String> {
    if !is_team_memory_path(ctx, path) {
        return None;
    }

    // `redact_secrets` returns `Cow::Borrowed` when no patterns matched
    // (zero-copy fast path). If it returns an owned String, at least one
    // secret was redacted — the unredacted content carries those
    // secrets and must not hit disk.
    let redacted = coco_secret_redact::redact_secrets(content);
    if matches!(redacted, std::borrow::Cow::Borrowed(_)) {
        return None;
    }

    Some(format!(
        "Refusing to write {}: content contains potential secrets (API keys, \
         tokens, or credentials). Team memory is shared with all repository \
         collaborators. Remove the sensitive content and try again.",
        path.display()
    ))
}

/// Layered team-memory path detection.
///
///  1. **Resolved-path check** — use the resolved memory config from
///     `ToolUseContext` for this project root (cwd override or process
///     cwd) and call
///     `coco_memory::team_paths::is_team_mem_path`. This is the
///     authoritative TS-aligned path that handles custom memory dirs
///     set via `COCO_REMOTE_MEMORY_DIR` or `COCO_MEMORY_PATH_OVERRIDE`.
///  2. **Substring fallback** — match `**/.claude/memory/team/**` as
///     a heuristic for paths whose resolved memory dir doesn't match
///     the on-disk path (custom mount points, symlinks, mid-session
///     cwd changes, test fixtures under tempdir). False positives on
///     this branch are gated by the secret-detector second stage so
///     they only trigger a rejection when a secret is actually
///     present.
fn is_team_memory_path(ctx: &coco_tool_runtime::ToolUseContext, path: &std::path::Path) -> bool {
    // Stage 1: authoritative resolution via coco-memory.
    let project_root = ctx
        .cwd_override
        .clone()
        .or_else(|| std::env::current_dir().ok());
    if let Some(root) = project_root {
        let memory_config = coco_memory::config::MemoryConfig::from(ctx.memory_config.clone());
        let memory_dir = memory_config.resolve_memory_dir(&root);
        if coco_memory::team_paths::is_team_mem_path(path, &memory_dir) {
            return true;
        }
    }

    // Stage 2: substring fallback. Catches paths where the resolved
    // memory dir doesn't match the file's on-disk location (custom
    // mount points, symlinks, mid-session cwd changes, test fixtures).
    let path_str = path.to_string_lossy();
    path_str.contains("/.claude/memory/team/") || path_str.contains("\\.claude\\memory\\team\\")
}

/// Push the read file path into `ctx.nested_memory_attachment_triggers`
/// so the app/query layer can load any nested CLAUDE.md / memory files
/// in the file's ancestry on the next turn boundary.
///
/// TS: `FileReadTool.ts:848,870,1038`
/// `context.nestedMemoryAttachmentTriggers?.add(fullFilePath)`. Drained
/// by `getNestedMemoryAttachments` (TS `utils/attachments.ts:2165`)
/// after the tool batch completes.
///
/// Fire-and-forget; no error path because the trigger set is purely
/// advisory — failure to record means at worst the next turn misses
/// a nested memory load, never a tool failure.
pub(crate) async fn track_nested_memory_attachment(ctx: &coco_tool_runtime::ToolUseContext, path: &Path) {
    let canonical = tokio::fs::canonicalize(path)
        .await
        .unwrap_or_else(|_| path.to_path_buf());
    let mut triggers = ctx.nested_memory_attachment_triggers.write().await;
    triggers.insert(canonical.display().to_string());
}

/// Discover any `.claude/skills` directories in the file's ancestry and
/// push them into `ctx.dynamic_skill_dir_triggers` for the app/query
/// layer to pick up after the tool batch.
///
/// TS: `FileReadTool.ts:578-591`, `FileWriteTool.ts`, `FileEditTool.ts`
/// (same pattern in all three) — fire-and-forget call to
/// `discoverSkillDirsForPaths` followed by adding the results to
/// `context.dynamicSkillDirTriggers` and `addSkillDirectories`.
///
/// In coco-rs the actual skill loading happens at the app/query layer
/// when it drains the trigger set after the tool batch — this helper
/// is just the discovery + record half. Cwd resolution falls back to
/// `ctx.cwd_override` (worktree-isolated subagents) then the process
/// cwd; if neither is available, the call is a no-op.
pub(crate) async fn track_skill_discovery(ctx: &coco_tool_runtime::ToolUseContext, path: &Path) {
    let cwd = ctx
        .cwd_override
        .clone()
        .or_else(|| std::env::current_dir().ok());
    let Some(cwd) = cwd else { return };

    let canonical = tokio::fs::canonicalize(path)
        .await
        .unwrap_or_else(|_| path.to_path_buf());
    let dirs = coco_skills::discover_skill_dirs_for_paths(&[canonical.as_path()], &cwd);

    if dirs.is_empty() {
        return;
    }

    let mut triggers = ctx.dynamic_skill_dir_triggers.write().await;
    for dir in dirs {
        triggers.insert(dir.display().to_string());
    }
}

/// Track a file edit for checkpoint/rewind before modifying.
///
/// TS: `fileHistoryTrackEdit()` — called from FileEditTool, FileWriteTool,
/// NotebookEditTool, BashTool before file modifications.
/// Silently no-ops if file history is not configured on the context.
pub(crate) async fn track_file_edit(ctx: &coco_tool_runtime::ToolUseContext, path: &Path) {
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
pub fn register_core_tools(registry: &mut coco_tool_runtime::ToolRegistry) {
    use std::sync::Arc;
    registry.register(Arc::new(BashTool));
    registry.register(Arc::new(ReadTool));
    registry.register(Arc::new(WriteTool));
    registry.register(Arc::new(EditTool));
    registry.register(Arc::new(GlobTool));
    registry.register(Arc::new(GrepTool));
}
