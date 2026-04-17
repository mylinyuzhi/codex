use coco_tool::DescriptionOptions;
use coco_tool::Tool;
use coco_tool::ToolError;
use coco_tool::ToolUseContext;
use coco_types::ToolId;
use coco_types::ToolInputSchema;
use coco_types::ToolName;
use coco_types::ToolResult;
use serde_json::Value;
use std::collections::HashMap;

// ── AskUserQuestionTool ──

pub struct AskUserQuestionTool;

#[async_trait::async_trait]
impl Tool for AskUserQuestionTool {
    fn id(&self) -> ToolId {
        ToolId::Builtin(ToolName::AskUserQuestion)
    }
    fn name(&self) -> &str {
        ToolName::AskUserQuestion.as_str()
    }
    fn description(&self, _: &Value, _options: &DescriptionOptions) -> String {
        "Ask the user a question and wait for their response. Supports \
         structured multi-choice questions with previews."
            .into()
    }
    fn input_schema(&self) -> ToolInputSchema {
        let mut p = HashMap::new();
        p.insert(
            "questions".into(),
            serde_json::json!({
                "type": "array",
                "description": "Questions to ask the user (1-4 questions)",
                "minItems": 1,
                "maxItems": 4,
                "items": {
                    "type": "object",
                    "properties": {
                        "question": {
                            "type": "string",
                            "description": "The question text"
                        },
                        "header": {
                            "type": "string",
                            "description": "Short label displayed as a chip/tag (max 20 chars)"
                        },
                        "options": {
                            "type": "array",
                            "description": "Available choices (2-4 options)",
                            "minItems": 2,
                            "maxItems": 4,
                            "items": {
                                "type": "object",
                                "properties": {
                                    "label": {
                                        "type": "string",
                                        "description": "Display text for this option (1-5 words)"
                                    },
                                    "description": {
                                        "type": "string",
                                        "description": "Explanation of what this option means"
                                    },
                                    "preview": {
                                        "type": "string",
                                        "description": "Optional preview content when option is focused"
                                    }
                                },
                                "required": ["label", "description"]
                            }
                        },
                        "multiSelect": {
                            "type": "boolean",
                            "description": "Allow multiple selections (default: false)"
                        }
                    },
                    "required": ["question", "header", "options"]
                }
            }),
        );
        ToolInputSchema { properties: p }
    }

    fn requires_user_interaction(&self) -> bool {
        true
    }

    /// TS `AskUserQuestionTool.tsx`: `isConcurrencySafe() { return true }`.
    /// Multiple questions issued in the same turn are presented together by
    /// the TUI, so the executor can batch them concurrently rather than
    /// serializing.
    fn is_concurrency_safe(&self, _: &Value) -> bool {
        true
    }

    async fn execute(
        &self,
        input: Value,
        _ctx: &ToolUseContext,
    ) -> Result<ToolResult<Value>, ToolError> {
        let questions = input
            .get("questions")
            .cloned()
            .unwrap_or(Value::Array(vec![]));

        // Return the questions as the result. The TUI/CLI layer intercepts
        // this tool's output, presents the UI, and fills in answers.
        Ok(ToolResult {
            data: serde_json::json!({"questions": questions}),
            new_messages: vec![],
        })
    }
}

// ── ToolSearchTool ──
//
// TS: `tools/ToolSearchTool/ToolSearchTool.ts:358-406`. Two query modes:
//
//   1. **Direct selection**: `select:Tool1,Tool2,Tool3` — the model
//      explicitly names which deferred tools to load. Comma-separated,
//      whitespace-tolerant, case-insensitive matching against tool
//      names and aliases. No ranking — every matched tool is returned.
//
//   2. **Keyword search**: any other query string. Substring match
//      against name, description, search_hint, and aliases. Ranked by
//      hit priority (name > hint > description), capped at `max_results`.
//
// Direct selection is how the model "un-defers" MCP tools that are
// hidden by default (`should_defer() = true`). Without ToolSearch, the
// model would never know those tools exist. With it, the system prompt
// can tell the model "use ToolSearch with query=select:MyTool to load
// the MyTool MCP tool" and the tool gets surfaced back into the
// runtime registry.
//
// For now, coco-rs ToolSearch returns metadata only — the actual
// promotion of deferred tools into the active registry is handled at a
// higher layer (query engine) via context modifiers. The `select:` path
// sets a `selected_tools` array in the result payload that the query
// layer can pick up.

/// Parse a `select:Tool1,Tool2,...` query into a list of tool names.
/// Returns `None` if the query isn't in select mode. Whitespace around
/// each name is trimmed; empty names are dropped.
///
/// **Prefix is case-insensitive** — `select:`, `Select:`, `SELECT:` all
/// trigger select mode. TS `ToolSearchTool.ts:363` uses the regex
/// `/^select:(.+)$/i` (the `/i` flag is case-insensitive). We mirror
/// that behavior by lowercasing the prefix check.
pub(super) fn parse_select_query(query: &str) -> Option<Vec<String>> {
    // Case-insensitive prefix match: if the first 7 chars (lowercased)
    // equal `"select:"`, strip them. Otherwise return None.
    if query.len() < 7 {
        return None;
    }
    let prefix = &query[..7];
    if !prefix.eq_ignore_ascii_case("select:") {
        return None;
    }
    let rest = &query[7..];
    Some(
        rest.split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect(),
    )
}

pub struct ToolSearchTool;

#[async_trait::async_trait]
impl Tool for ToolSearchTool {
    fn id(&self) -> ToolId {
        ToolId::Builtin(ToolName::ToolSearch)
    }
    fn name(&self) -> &str {
        ToolName::ToolSearch.as_str()
    }
    fn description(&self, _: &Value, _options: &DescriptionOptions) -> String {
        "Search for available tools by keyword, or directly select tools by name. \
         Use 'select:Tool1,Tool2' to load specific deferred tools, or a plain keyword \
         query to search by name/description/alias."
            .into()
    }
    fn input_schema(&self) -> ToolInputSchema {
        let mut p = HashMap::new();
        p.insert(
            "query".into(),
            serde_json::json!({
                "type": "string",
                "description": "Keyword search query, or 'select:Tool1,Tool2' for direct selection"
            }),
        );
        p.insert(
            "max_results".into(),
            serde_json::json!({"type": "number", "description": "Maximum number of results (default 5)"}),
        );
        ToolInputSchema { properties: p }
    }
    fn is_read_only(&self, _: &Value) -> bool {
        true
    }
    fn is_concurrency_safe(&self, _: &Value) -> bool {
        true
    }

    async fn execute(
        &self,
        input: Value,
        ctx: &ToolUseContext,
    ) -> Result<ToolResult<Value>, ToolError> {
        let raw_query = input
            .get("query")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();

        if raw_query.is_empty() {
            return Err(ToolError::InvalidInput {
                message: "query parameter is required".into(),
                error_code: None,
            });
        }

        // Count of deferred tools — both modes include this in the output.
        let total_deferred_tools = ctx.tools.all().filter(|t| t.should_defer()).count();

        // Direct selection mode: `select:Tool1,Tool2,...`
        //
        // TS `ToolSearchTool.ts:37-45, 110-126, 363-405` returns a
        // UNIFIED output shape for both select and keyword modes:
        //
        //   { matches: string[], query: string,
        //     total_deferred_tools: number,
        //     pending_mcp_servers?: string[] }
        //
        // There's NO separate `mode` field and NO `selected_tools` /
        // `missing` fields — TS just filters the requested names down
        // to the ones that resolved and puts the found names in
        // `matches`. Tools that fail to resolve are silently dropped.
        //
        // Previously coco-rs returned `{mode, requested, selected_tools,
        // missing}` which broke downstream code expecting the TS shape;
        // R2 from round-2 deep-review fixes that.
        if let Some(names) = parse_select_query(&raw_query) {
            if names.is_empty() {
                return Err(ToolError::InvalidInput {
                    message: "select: query must name at least one tool (e.g. 'select:Read,Grep')"
                        .into(),
                    error_code: None,
                });
            }
            // Resolve each requested name (case-insensitive on name +
            // aliases). TS uses `findToolByName` which does the same
            // case-insensitive lookup.
            let mut matches: Vec<String> = Vec::new();
            for name in &names {
                let name_lower = name.to_lowercase();
                let hit = ctx.tools.all().into_iter().find(|t| {
                    t.name().eq_ignore_ascii_case(name)
                        || t.aliases()
                            .iter()
                            .any(|a| a.eq_ignore_ascii_case(&name_lower))
                });
                if let Some(tool) = hit {
                    matches.push(tool.name().to_string());
                }
            }
            return Ok(ToolResult {
                data: serde_json::json!({
                    "matches": matches,
                    "query": raw_query,
                    "total_deferred_tools": total_deferred_tools,
                }),
                new_messages: vec![],
            });
        }

        // Keyword search mode — same output shape as select mode.
        //
        // Note: `matches` is an array of tool NAMES (strings), not
        // full objects. Downstream code resolves the names via the
        // registry if it needs descriptions. This matches TS exactly.
        let query = raw_query.to_lowercase();

        let max_results = input
            .get("max_results")
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(5) as usize;

        let mut matches: Vec<String> = Vec::new();

        for tool in ctx.tools.all() {
            let name_lower = tool.name().to_lowercase();
            let desc_lower = tool
                .description(&Value::Null, &DescriptionOptions::default())
                .to_lowercase();
            let hint_lower = tool
                .search_hint()
                .map(str::to_lowercase)
                .unwrap_or_default();
            let alias_match = tool
                .aliases()
                .iter()
                .any(|a| a.to_lowercase().contains(&query));

            if name_lower.contains(&query)
                || desc_lower.contains(&query)
                || hint_lower.contains(&query)
                || alias_match
            {
                matches.push(tool.name().to_string());
            }

            if matches.len() >= max_results {
                break;
            }
        }

        Ok(ToolResult {
            data: serde_json::json!({
                "matches": matches,
                "query": raw_query,
                "total_deferred_tools": total_deferred_tools,
            }),
            new_messages: vec![],
        })
    }
}

// ── ConfigTool ──

pub struct ConfigTool;

/// Known configuration keys for documentation.
const KNOWN_CONFIG_KEYS: &[&str] = &[
    "model",
    "provider",
    "thinking_level",
    "max_budget_usd",
    "permission_mode",
    "sandbox_mode",
    "custom_system_prompt",
    "append_system_prompt",
    "verbose",
    "debug",
];

#[async_trait::async_trait]
impl Tool for ConfigTool {
    fn id(&self) -> ToolId {
        ToolId::Builtin(ToolName::Config)
    }
    fn name(&self) -> &str {
        ToolName::Config.as_str()
    }
    fn description(&self, _: &Value, _options: &DescriptionOptions) -> String {
        "Manage configuration settings. Supports get, set, list, and reset actions.".into()
    }
    fn input_schema(&self) -> ToolInputSchema {
        let mut p = HashMap::new();
        p.insert(
            "action".into(),
            serde_json::json!({"type": "string", "enum": ["get", "set", "list", "reset"], "description": "Configuration action to perform"}),
        );
        p.insert(
            "key".into(),
            serde_json::json!({"type": "string", "description": "Configuration key (for get/set/reset)"}),
        );
        p.insert(
            "value".into(),
            serde_json::json!({"description": "Configuration value (for set)"}),
        );
        ToolInputSchema { properties: p }
    }

    /// TS `ConfigTool.ts`: `isConcurrencySafe() { return true }`. Read paths
    /// (get/list) are obviously safe; mutating paths (set/reset) currently
    /// just emit an instructional message rather than writing config, so
    /// they're safe too. Should the tool ever start mutating a shared
    /// settings file, demote to input-conditional safety like BashTool.
    fn is_concurrency_safe(&self, _: &Value) -> bool {
        true
    }

    async fn execute(
        &self,
        input: Value,
        _ctx: &ToolUseContext,
    ) -> Result<ToolResult<Value>, ToolError> {
        let action = input
            .get("action")
            .and_then(|v| v.as_str())
            .unwrap_or("list");
        let key = input.get("key").and_then(|v| v.as_str()).unwrap_or("");

        let result = match action {
            "list" => {
                serde_json::json!({
                    "message": "Available configuration keys",
                    "keys": KNOWN_CONFIG_KEYS,
                })
            }
            "get" => {
                if key.is_empty() {
                    return Err(ToolError::InvalidInput {
                        message: "key parameter is required for 'get' action".into(),
                        error_code: None,
                    });
                }
                serde_json::json!({
                    "message": format!("Configuration value for '{key}' is managed by ConfigManager. Use the CLI 'config' subcommand to view or edit settings."),
                    "key": key,
                })
            }
            "set" => {
                if key.is_empty() {
                    return Err(ToolError::InvalidInput {
                        message: "key parameter is required for 'set' action".into(),
                        error_code: None,
                    });
                }
                let value = input.get("value").cloned().unwrap_or(Value::Null);
                serde_json::json!({
                    "message": format!("To set '{key}', use the CLI 'config set {key} <value>' command or edit the config file directly."),
                    "key": key,
                    "value": value,
                })
            }
            "reset" => {
                if key.is_empty() {
                    return Err(ToolError::InvalidInput {
                        message: "key parameter is required for 'reset' action".into(),
                        error_code: None,
                    });
                }
                serde_json::json!({
                    "message": format!("To reset '{key}' to default, use the CLI 'config reset {key}' command."),
                    "key": key,
                })
            }
            other => {
                return Err(ToolError::InvalidInput {
                    message: format!("Unknown action '{other}'. Must be get, set, list, or reset"),
                    error_code: None,
                });
            }
        };

        Ok(ToolResult {
            data: result,
            new_messages: vec![],
        })
    }
}

// ── BriefTool ──
//
// TS: BriefTool.ts — sends structured messages to the user with optional
// file attachments. Status distinguishes normal replies from proactive updates.

pub struct BriefTool;

#[async_trait::async_trait]
impl Tool for BriefTool {
    fn id(&self) -> ToolId {
        ToolId::Builtin(ToolName::Brief)
    }
    fn name(&self) -> &str {
        ToolName::Brief.as_str()
    }
    fn description(&self, _: &Value, _options: &DescriptionOptions) -> String {
        "Send a structured message to the user with optional file attachments.".into()
    }
    fn input_schema(&self) -> ToolInputSchema {
        let mut p = HashMap::new();
        p.insert(
            "message".into(),
            serde_json::json!({"type": "string", "description": "Markdown-formatted message to the user"}),
        );
        p.insert(
            "attachments".into(),
            serde_json::json!({"type": "array", "items": {"type": "string"}, "description": "File paths (absolute or relative to cwd) to attach"}),
        );
        p.insert(
            "status".into(),
            serde_json::json!({"type": "string", "enum": ["normal", "proactive"], "description": "Message intent: 'normal' for direct replies, 'proactive' for unsolicited updates"}),
        );
        ToolInputSchema { properties: p }
    }
    fn is_read_only(&self, _: &Value) -> bool {
        true
    }

    /// TS `BriefTool.ts`: `isConcurrencySafe() { return true }`. Brief
    /// messages are a side-channel to the user — multiple briefs in the
    /// same turn are independent and stamped with their own timestamps.
    fn is_concurrency_safe(&self, _: &Value) -> bool {
        true
    }

    async fn execute(
        &self,
        input: Value,
        ctx: &ToolUseContext,
    ) -> Result<ToolResult<Value>, ToolError> {
        let message = input
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        if message.is_empty() {
            return Err(ToolError::InvalidInput {
                message: "message parameter is required".into(),
                error_code: None,
            });
        }

        let status = input
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("normal");

        // Resolve attachments. Relative paths resolve against the
        // context cwd override (worktree-isolated subagents) before
        // falling back to the process cwd, so a teammate inside a
        // worktree sees its own files rather than the host process's.
        let resolve_root = ctx
            .cwd_override
            .clone()
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_default();
        let mut resolved_attachments: Vec<Value> = Vec::new();
        if let Some(attachments) = input.get("attachments").and_then(|v| v.as_array()) {
            for attachment in attachments {
                if let Some(path_str) = attachment.as_str() {
                    let path = if std::path::Path::new(path_str).is_absolute() {
                        std::path::PathBuf::from(path_str)
                    } else {
                        resolve_root.join(path_str)
                    };

                    let meta = tokio::fs::metadata(&path).await;
                    let exists = meta.is_ok();
                    let size = meta.as_ref().map(std::fs::Metadata::len).unwrap_or(0);
                    let is_image = path
                        .extension()
                        .and_then(|e| e.to_str())
                        .is_some_and(|ext| {
                            matches!(
                                ext.to_lowercase().as_str(),
                                "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg"
                            )
                        });

                    resolved_attachments.push(serde_json::json!({
                        "path": path.display().to_string(),
                        "exists": exists,
                        "size": size,
                        "is_image": is_image,
                    }));
                }
            }
        }

        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
            .to_string();

        Ok(ToolResult {
            data: serde_json::json!({
                "message": message,
                "status": status,
                "attachments": resolved_attachments,
                "timestamp": timestamp,
            }),
            new_messages: vec![],
        })
    }
}

// ── LspTool ──

pub struct LspTool;

#[async_trait::async_trait]
impl Tool for LspTool {
    fn id(&self) -> ToolId {
        ToolId::Builtin(ToolName::Lsp)
    }
    fn name(&self) -> &str {
        ToolName::Lsp.as_str()
    }
    fn description(&self, _: &Value, _options: &DescriptionOptions) -> String {
        "Query the Language Server Protocol for code intelligence (definitions, references, diagnostics, symbols, hover).".into()
    }
    fn input_schema(&self) -> ToolInputSchema {
        let mut p = HashMap::new();
        p.insert(
            "action".into(),
            serde_json::json!({"type": "string", "enum": ["definition", "references", "diagnostics", "symbols", "hover"], "description": "LSP action to perform"}),
        );
        p.insert(
            "path".into(),
            serde_json::json!({"type": "string", "description": "File path for the query"}),
        );
        p.insert(
            "symbol".into(),
            serde_json::json!({"type": "string", "description": "Symbol name to query"}),
        );
        ToolInputSchema { properties: p }
    }
    fn is_read_only(&self, _: &Value) -> bool {
        true
    }
    fn is_lsp(&self) -> bool {
        true
    }
    /// TS `LSPTool.ts`: `isConcurrencySafe() { return true }`. LSP queries
    /// are side-effect-free and safe to issue in parallel — the LSP server
    /// itself handles concurrent requests.
    fn is_concurrency_safe(&self, _: &Value) -> bool {
        true
    }

    async fn execute(
        &self,
        input: Value,
        _ctx: &ToolUseContext,
    ) -> Result<ToolResult<Value>, ToolError> {
        let action = input
            .get("action")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");

        Err(ToolError::ExecutionFailed {
            message: format!(
                "LSP server is not connected. Cannot perform '{action}' action. \
                 Ensure a language server is running and configured for the current project."
            ),
            source: None,
        })
    }
}

// ── NotebookEditTool ──
//
// TS: `tools/NotebookEditTool/NotebookEditTool.ts:90-433` — full Jupyter
// notebook cell editing with replace/insert/delete modes, cell ID and
// index lookup, output clearing on replace, and nbformat-aware cell ID
// generation.
//
// The implementation below is TS-aligned on the wire shape:
//
// Input schema (TS:50-55 + :44-48):
//   - notebook_path  : string (required, absolute .ipynb path)
//   - cell_id        : string (required, cell UUID or "cell-N" index)
//   - new_source     : string (content for replace/insert)
//   - cell_type      : enum { code, markdown } (required for insert)
//                      **raw is NOT supported** — matches TS limitation
//   - edit_mode      : enum { replace, insert, delete }
//
// Cell ID generation (TS:381-386): uses `Math.random().toString(36)
// .substring(2, 15)` — a 13-char alphanumeric base-36 string — and only
// applies when the notebook's nbformat is ≥ 4.5. We do the same with
// rand::thread_rng so new cells round-trip identically between TS and
// Rust writers.

pub struct NotebookEditTool;

#[async_trait::async_trait]
impl Tool for NotebookEditTool {
    fn id(&self) -> ToolId {
        ToolId::Builtin(ToolName::NotebookEdit)
    }
    fn name(&self) -> &str {
        ToolName::NotebookEdit.as_str()
    }
    fn description(&self, _: &Value, _options: &DescriptionOptions) -> String {
        "Edit a cell in a Jupyter notebook (.ipynb file). Supports replace, insert, and delete operations.".into()
    }
    fn input_schema(&self) -> ToolInputSchema {
        let mut p = HashMap::new();
        p.insert(
            "notebook_path".into(),
            serde_json::json!({"type": "string", "description": "Absolute path to the .ipynb notebook file"}),
        );
        p.insert(
            "cell_id".into(),
            serde_json::json!({"type": "string", "description": "Cell ID or 'cell-N' numeric index"}),
        );
        p.insert(
            "new_source".into(),
            serde_json::json!({"type": "string", "description": "New source content for the cell"}),
        );
        p.insert(
            "cell_type".into(),
            serde_json::json!({"type": "string", "enum": ["code", "markdown"], "description": "Cell type (required for insert mode)"}),
        );
        p.insert(
            "edit_mode".into(),
            serde_json::json!({"type": "string", "enum": ["replace", "insert", "delete"], "description": "Edit operation: replace (default), insert (new cell), or delete"}),
        );
        ToolInputSchema { properties: p }
    }

    async fn execute(
        &self,
        input: Value,
        ctx: &ToolUseContext,
    ) -> Result<ToolResult<Value>, ToolError> {
        let notebook_path = input
            .get("notebook_path")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        if notebook_path.is_empty() {
            return Err(ToolError::InvalidInput {
                message: "notebook_path parameter is required".into(),
                error_code: None,
            });
        }

        let edit_mode = input
            .get("edit_mode")
            .and_then(|v| v.as_str())
            .unwrap_or("replace");

        let cell_id = input.get("cell_id").and_then(|v| v.as_str()).unwrap_or("");

        // Enforce read-before-edit, matching TS `NotebookEditTool.ts:218-237`.
        // Without this guard the model can edit a notebook it never saw (or
        // edit against a stale view after an external change), silently
        // clobbering data. Mirrors the same check in `FileEditTool` /
        // `FileWriteTool`. The check runs only when `file_read_state` is
        // populated — tests without a context still work.
        if let Some(frs) = &ctx.file_read_state
            && let Ok(abs_path) = std::fs::canonicalize(notebook_path)
        {
            let frs_read = frs.read().await;
            if frs_read.peek(&abs_path).is_none() {
                return Err(ToolError::ExecutionFailed {
                    message: format!(
                        "{notebook_path} has not been read yet. Read it first before editing it."
                    ),
                    source: None,
                });
            }
            // mtime drift check mirrors `FileEditTool.ts:451-467` — reject
            // edits staged against a view that is older than the current
            // disk mtime so we don't quietly overwrite external changes.
            if let Some(entry) = frs_read.peek(&abs_path)
                && let Ok(disk_mtime) = coco_context::file_mtime_ms(&abs_path).await
                && entry.mtime_ms != disk_mtime
            {
                return Err(ToolError::ExecutionFailed {
                    message: format!(
                        "{notebook_path} has been modified since it was last read. \
                         Read it again before editing."
                    ),
                    source: None,
                });
            }
        }

        // Read the notebook file
        let content = tokio::fs::read_to_string(notebook_path)
            .await
            .map_err(|e| ToolError::ExecutionFailed {
                message: format!("Failed to read notebook '{notebook_path}': {e}"),
                source: None,
            })?;

        let mut notebook: Value =
            serde_json::from_str(&content).map_err(|e| ToolError::ExecutionFailed {
                message: format!("Failed to parse notebook JSON: {e}"),
                source: None,
            })?;

        // Read nbformat before mutating
        let nbformat = notebook
            .get("nbformat")
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(4);
        let nbformat_minor = notebook
            .get("nbformat_minor")
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(0);

        let cells = notebook
            .get_mut("cells")
            .and_then(|v| v.as_array_mut())
            .ok_or_else(|| ToolError::ExecutionFailed {
                message: "Notebook does not contain a 'cells' array".into(),
                source: None,
            })?;

        // Resolve cell index from cell_id. For insert with an empty
        // cell_id we default to position 0 so the model can create the
        // first cell without having to pass "0" explicitly.
        let cell_index = if edit_mode == "insert" && cell_id.is_empty() {
            0
        } else {
            resolve_cell_index(cells, cell_id)?
        };

        // R5-T15: return the actual cell ID (string) rather than a bare
        // index. TS emits `new_cell_id` for insert and `cell_id` for
        // replace/delete so the model can reference cells by the ID it
        // wrote. `cell_index` is still returned for debuggability.
        let mut resolved_cell_id: Option<String> = None;
        let mut new_cell_id: Option<String> = None;

        let result_msg = match edit_mode {
            "replace" => {
                let new_source = input
                    .get("new_source")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                if cell_index >= cells.len() {
                    return Err(ToolError::InvalidInput {
                        message: format!(
                            "cell index {cell_index} out of range (notebook has {} cells)",
                            cells.len()
                        ),
                        error_code: None,
                    });
                }

                // Capture the resolved cell's id BEFORE mutating the
                // source — the id field is not touched by replace, so
                // reading it here matches TS semantics.
                resolved_cell_id = cells[cell_index]
                    .get("id")
                    .and_then(|v| v.as_str())
                    .map(String::from);

                cells[cell_index]["source"] = Value::Array(source_to_lines(new_source));
                // Reset execution state on replace
                cells[cell_index]["execution_count"] = Value::Null;
                cells[cell_index]["outputs"] = Value::Array(vec![]);

                format!("Replaced cell {cell_index} in '{notebook_path}'")
            }
            "insert" => {
                let new_source = input
                    .get("new_source")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let cell_type = input
                    .get("cell_type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("code");

                let mut new_cell = serde_json::json!({
                    "cell_type": cell_type,
                    "source": source_to_lines(new_source),
                    "metadata": {},
                });

                if cell_type == "code" {
                    new_cell["execution_count"] = Value::Null;
                    new_cell["outputs"] = Value::Array(vec![]);
                }

                // Cell ID generation — nbformat ≥ 4.5 only (TS:381-386).
                // TS uses `Math.random().toString(36).substring(2, 15)`
                // which is a 13-char base-36 alphanumeric. We match that
                // with a rand::thread_rng-based generator so new cells
                // look identical to TS-written ones.
                if nbformat > 4 || (nbformat == 4 && nbformat_minor >= 5) {
                    let generated = generate_cell_id();
                    new_cell["id"] = Value::String(generated.clone());
                    new_cell_id = Some(generated);
                }

                let insert_at = cell_index.min(cells.len());
                cells.insert(insert_at, new_cell);

                format!("Inserted {cell_type} cell at index {insert_at} in '{notebook_path}'")
            }
            "delete" => {
                if cell_index >= cells.len() {
                    return Err(ToolError::InvalidInput {
                        message: format!(
                            "cell index {cell_index} out of range (notebook has {} cells)",
                            cells.len()
                        ),
                        error_code: None,
                    });
                }
                // Capture the cell's id before removing it.
                resolved_cell_id = cells[cell_index]
                    .get("id")
                    .and_then(|v| v.as_str())
                    .map(String::from);
                cells.remove(cell_index);
                format!("Deleted cell {cell_index} from '{notebook_path}'")
            }
            other => {
                return Err(ToolError::InvalidInput {
                    message: format!(
                        "Unknown edit_mode '{other}'. Must be replace, insert, or delete"
                    ),
                    error_code: None,
                });
            }
        };

        // Write back
        let updated =
            serde_json::to_string_pretty(&notebook).map_err(|e| ToolError::ExecutionFailed {
                message: format!("Failed to serialize notebook: {e}"),
                source: None,
            })?;

        tokio::fs::write(notebook_path, &updated)
            .await
            .map_err(|e| ToolError::ExecutionFailed {
                message: format!("Failed to write notebook '{notebook_path}': {e}"),
                source: None,
            })?;

        // Build the TS-shaped response. For insert: include `new_cell_id`
        // (or null when nbformat < 4.5). For replace/delete: include
        // `cell_id` from the resolved cell. Always include `cell_index`
        // for debuggability.
        let mut data = serde_json::json!({
            "message": result_msg,
            "notebook_path": notebook_path,
            "cell_index": cell_index,
            "edit_mode": edit_mode,
        });
        if edit_mode == "insert" {
            // TS emits `new_cell_id` even when null (nbformat < 4.5).
            data["new_cell_id"] = match new_cell_id {
                Some(id) => Value::String(id),
                None => Value::Null,
            };
        } else if let Some(id) = resolved_cell_id {
            data["cell_id"] = Value::String(id);
        }

        Ok(ToolResult {
            data,
            new_messages: vec![],
        })
    }
}

/// Resolve a cell identifier to an index.
/// Supports: numeric string, "cell-N" format, or cell ID matching.
fn resolve_cell_index(cells: &[Value], cell_id: &str) -> Result<usize, ToolError> {
    if cell_id.is_empty() {
        return Err(ToolError::InvalidInput {
            message: "cell_id parameter is required".into(),
            error_code: None,
        });
    }

    // Try "cell-N" format
    if let Some(n) = cell_id.strip_prefix("cell-")
        && let Ok(idx) = n.parse::<usize>()
    {
        return Ok(idx);
    }

    // Try direct numeric
    if let Ok(idx) = cell_id.parse::<usize>() {
        return Ok(idx);
    }

    // Try matching cell ID field
    for (i, cell) in cells.iter().enumerate() {
        if cell.get("id").and_then(|v| v.as_str()) == Some(cell_id) {
            return Ok(i);
        }
    }

    Err(ToolError::InvalidInput {
        message: format!("Could not find cell with ID '{cell_id}'"),
        error_code: None,
    })
}

/// Convert source text to notebook line array format.
fn source_to_lines(source: &str) -> Vec<Value> {
    let lines: Vec<&str> = source.lines().collect();
    if lines.is_empty() {
        return vec![Value::String(String::new())];
    }
    lines
        .iter()
        .enumerate()
        .map(|(i, line)| {
            if i < lines.len() - 1 {
                Value::String(format!("{line}\n"))
            } else {
                Value::String((*line).to_string())
            }
        })
        .collect()
}

/// Generate a Jupyter cell ID.
///
/// TS `NotebookEditTool.ts:381-386` uses
/// `Math.random().toString(36).substring(2, 15)` — a 13-char lowercase
/// alphanumeric (base-36) string. We replicate the format exactly so
/// notebooks written by coco-rs round-trip visually identical with
/// TS-written notebooks.
pub(crate) fn generate_cell_id() -> String {
    use rand::Rng;
    const CHARSET: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
    let mut rng = rand::rng();
    (0..13)
        .map(|_| {
            let idx = rng.random_range(0..CHARSET.len());
            CHARSET[idx] as char
        })
        .collect()
}

#[cfg(test)]
#[path = "utility.test.rs"]
mod tests;
