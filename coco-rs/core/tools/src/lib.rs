//! All built-in tool implementations (static tools + MCPTool dynamic wrapper).
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
pub fn register_all_tools(registry: &coco_tool_runtime::ToolRegistry) {
    use std::sync::Arc;

    // File I/O (8 — `ApplyPatchTool` only surfaces for models that
    // declare it via `ToolOverrides::with_extra`; other models see
    // it as hidden.)
    registry.register(Arc::new(BashTool));
    registry.register(Arc::new(ReadTool));
    registry.register(Arc::new(WriteTool));
    registry.register(Arc::new(EditTool));
    registry.register(Arc::new(GlobTool));
    registry.register(Arc::new(GrepTool));
    registry.register(Arc::new(NotebookEditTool));
    registry.register(Arc::new(ApplyPatchTool));

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

    // Plan & Worktree (5)
    registry.register(Arc::new(EnterPlanModeTool));
    registry.register(Arc::new(ExitPlanModeTool));
    registry.register(Arc::new(VerifyPlanExecutionTool));
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

    // Shell (3)
    registry.register(Arc::new(PowerShellTool));
    registry.register(Arc::new(ReplTool));
    registry.register(Arc::new(SleepTool));

    // `StructuredOutputTool` is **intentionally excluded** from the
    // default base set. Mirrors TS `tools.ts:300-307` where the name is
    // in `specialTools` and filtered out of `getAllBaseTools()`; the
    // only entry-point is the explicit injection in non-interactive
    // bootstrap paths via [`register_structured_output_tool`].
}

/// Register the `StructuredOutput` synthetic tool with a user-supplied
/// JSON schema.
///
/// Mirrors TS `main.tsx:1879-1901`: only the non-interactive bootstrap
/// paths (headless print mode, SDK NDJSON) call this after parsing
/// `--json-schema`. TUI never reaches it — `tui_runner` never invokes
/// this function, and the tool is absent from
/// [`register_all_tools`] so interactive sessions never see it.
///
/// Returns the parsed/compiled tool's reference so callers can install
/// matching Stop-hook enforcement (TS
/// `registerStructuredOutputEnforcement` lives at the same call site).
///
/// Errors are propagated as `String` (Ajv-equivalent: invalid schema
/// shape, unsupported keyword, …). TS `createSyntheticOutputTool`
/// returns `{error}` in the same shape; coco-rs leaves it to the caller
/// to log + decide whether to abort the run.
pub fn register_structured_output_tool(
    registry: &coco_tool_runtime::ToolRegistry,
    schema: serde_json::Value,
) -> Result<std::sync::Arc<StructuredOutputTool>, String> {
    use std::sync::Arc;
    let tool = Arc::new(StructuredOutputTool::new(schema)?);
    registry.register(tool.clone());
    Ok(tool)
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
    registry: &coco_tool_runtime::ToolRegistry,
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
pub fn deregister_mcp_server(registry: &coco_tool_runtime::ToolRegistry, server_name: &str) {
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
/// Reject Edit/Write/NotebookEdit calls whose target falls outside the
/// caller-installed write fence on `ToolUseContext::allowed_write_roots`.
///
/// Empty fence = no restriction (the common case). When non-empty
/// (forked memory-extraction / auto-dream subagents), the path must
/// be a descendant of one of the listed roots after `..` normalization.
///
/// TS: `services/extractMemories/extractMemories.ts:createAutoMemCanUseTool`
/// (memdir-only sandbox for the extraction agent).
pub(crate) fn check_write_root_fence(
    ctx: &coco_tool_runtime::ToolUseContext,
    path: &std::path::Path,
) -> Option<String> {
    if ctx.allowed_write_roots.is_empty() {
        return None;
    }
    // Resolve relative paths against cwd_override (worktree-isolated
    // subagents) or the process cwd. Without this step, a relative
    // path like `notes.md` would lexically-normalize to itself, never
    // match an absolute fence root, and **either** be over-rejected
    // (when the model intended a memdir-relative write) or — worse,
    // when a tool resolves the path itself via OS cwd — slip past the
    // fence entirely.
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else if let Some(cwd) = ctx.cwd_override.as_ref() {
        cwd.join(path)
    } else {
        std::env::current_dir()
            .map(|c| c.join(path))
            .unwrap_or_else(|_| path.to_path_buf())
    };
    let normalized = normalize_lexical(&absolute);
    let allowed = ctx.allowed_write_roots.iter().any(|root| {
        let root = normalize_lexical(root);
        normalized.starts_with(&root)
    });
    if allowed {
        return None;
    }
    Some(format!(
        "Refusing to write {}: this agent is sandboxed and may only write under one of {}.",
        path.display(),
        ctx.allowed_write_roots
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    ))
}

fn normalize_lexical(path: &std::path::Path) -> std::path::PathBuf {
    let mut out = std::path::PathBuf::new();
    for c in path.components() {
        match c {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            other => out.push(other),
        }
    }
    out
}

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
        // The config home is derived from `CocoConfigDir` env or
        // defaulted by the bootstrap layer. We don't have direct access
        // here, so fall back to `~/.coco` — same default the resolver
        // uses for the layered settings path.
        let config_home = std::env::var_os("HOME")
            .map(|h| std::path::PathBuf::from(h).join(".coco"))
            .unwrap_or_else(|| std::path::PathBuf::from(".coco"));
        let directories = coco_memory::path::MemoryDir::resolve(
            &config_home,
            &root,
            ctx.memory_config.directory.as_deref(),
        );
        if coco_memory::path::memory_scope_for_path(path, &directories.personal)
            == coco_memory::path::MemoryScope::Team
        {
            return true;
        }
    }

    // Stage 2: substring fallback. Catches paths where the resolved
    // memory dir doesn't match the file's on-disk location (custom
    // mount points, symlinks, mid-session cwd changes, test fixtures).
    // Both `.claude/memory/team/` (legacy / TS layout) and `.coco/.../memory/team/`
    // are accepted — the secret-detector second stage gates false positives.
    let path_str = path.to_string_lossy();
    path_str.contains("/memory/team/") || path_str.contains("\\memory\\team\\")
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
pub(crate) async fn track_nested_memory_attachment(
    ctx: &coco_tool_runtime::ToolUseContext,
    path: &Path,
) {
    let canonical = tokio::fs::canonicalize(path)
        .await
        .unwrap_or_else(|_| path.to_path_buf());
    let mut triggers = ctx.nested_memory_attachment_triggers.write().await;
    triggers.insert(canonical.display().to_string());
}

/// Record a file touched by Read/Write/Edit/Bash for two end-of-batch
/// follow-ups:
///
/// 1. **Nested-dir discovery** — walk up the file's ancestry to find
///    any `.claude/skills/` directories not yet loaded; push them into
///    `ctx.dynamic_skill_dir_triggers`. TS:
///    `FileReadTool.ts:578-591` (`discoverSkillDirsForPaths` +
///    `addSkillDirectories`).
/// 2. **Conditional-skill activation** — push the file path itself
///    into `ctx.dynamic_skill_path_triggers` so the app/query drain
///    can match it against any skill's `paths` frontmatter via
///    `SkillsSource::activate_skills_for_paths`. TS:
///    `activateConditionalSkillsForPaths(filePaths, cwd)` from the
///    same Read/Write/Edit/Bash post-success hook.
///
/// Both are deferred to the app/query post-batch drain so concurrent
/// safe-tool execution can share one activation pass. Cwd resolution
/// falls back to `ctx.cwd_override` (worktree-isolated subagents)
/// then the process cwd; if neither is available, the call is a no-op
/// (no path-relative gitignore matching possible).
pub(crate) async fn track_skill_triggers(ctx: &coco_tool_runtime::ToolUseContext, path: &Path) {
    let cwd = ctx
        .cwd_override
        .clone()
        .or_else(|| std::env::current_dir().ok());
    let Some(cwd) = cwd else { return };

    // (2) Conditional activation runs against the **raw** file path
    // (TS parity: `activateConditionalSkillsForPaths` uses the input
    // path as-is). Canonicalization is wrong here — if cwd is itself a
    // symlink, the canonical file path won't have cwd as a prefix and
    // the activation pass would silently skip the file.
    {
        let mut path_triggers = ctx.dynamic_skill_path_triggers.write().await;
        path_triggers.insert(path.display().to_string());
    }

    // (1) Nested-dir discovery walks the file's filesystem ancestry to
    // find `.claude/skills/` dirs — canonicalization is fine here
    // because the dir-walk needs the real filesystem layout.
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
pub fn register_core_tools(registry: &coco_tool_runtime::ToolRegistry) {
    use std::sync::Arc;
    registry.register(Arc::new(BashTool));
    registry.register(Arc::new(ReadTool));
    registry.register(Arc::new(WriteTool));
    registry.register(Arc::new(EditTool));
    registry.register(Arc::new(GlobTool));
    registry.register(Arc::new(GrepTool));
}
