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
        "Search for available tools by keyword, returning matching tool names and descriptions."
            .into()
    }
    fn input_schema(&self) -> ToolInputSchema {
        let mut p = HashMap::new();
        p.insert(
            "query".into(),
            serde_json::json!({"type": "string", "description": "Search query to match against tool names, aliases, and hints"}),
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
        let query = input
            .get("query")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_lowercase();

        if query.is_empty() {
            return Err(ToolError::InvalidInput {
                message: "query parameter is required".into(),
                error_code: None,
            });
        }

        let max_results = input
            .get("max_results")
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(5) as usize;

        let mut matches: Vec<Value> = Vec::new();

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
                matches.push(serde_json::json!({
                    "name": tool.name(),
                    "description": tool.description(&Value::Null, &DescriptionOptions::default()),
                    "deferred": tool.should_defer(),
                }));
            }

            if matches.len() >= max_results {
                break;
            }
        }

        if matches.is_empty() {
            Ok(ToolResult {
                data: serde_json::json!({"message": format!("No tools found matching '{query}'"), "results": []}),
                new_messages: vec![],
            })
        } else {
            Ok(ToolResult {
                data: serde_json::json!({"results": matches}),
                new_messages: vec![],
            })
        }
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

        // Resolve attachments
        let mut resolved_attachments: Vec<Value> = Vec::new();
        if let Some(attachments) = input.get("attachments").and_then(|v| v.as_array()) {
            for attachment in attachments {
                if let Some(path_str) = attachment.as_str() {
                    let path = if std::path::Path::new(path_str).is_absolute() {
                        std::path::PathBuf::from(path_str)
                    } else {
                        std::env::current_dir().unwrap_or_default().join(path_str)
                    };

                    let meta = tokio::fs::metadata(&path).await;
                    let exists = meta.is_ok();
                    let size = meta.as_ref().map(|m| m.len()).unwrap_or(0);
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
// TS: NotebookEditTool.ts — full Jupyter notebook cell editing with
// replace/insert/delete modes, cell ID and index lookup, output clearing.

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
        _ctx: &ToolUseContext,
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
            .and_then(|v| v.as_i64())
            .unwrap_or(4);
        let nbformat_minor = notebook
            .get("nbformat_minor")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);

        let cells = notebook
            .get_mut("cells")
            .and_then(|v| v.as_array_mut())
            .ok_or_else(|| ToolError::ExecutionFailed {
                message: "Notebook does not contain a 'cells' array".into(),
                source: None,
            })?;

        // Resolve cell index from cell_id
        let cell_index = resolve_cell_index(cells, cell_id)?;

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

                // Check nbformat for cell ID support (>= 4.5)
                if nbformat > 4 || (nbformat == 4 && nbformat_minor >= 5) {
                    let id = format!("cell-{}", cells.len());
                    new_cell["id"] = Value::String(id);
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

        Ok(ToolResult {
            data: serde_json::json!({
                "message": result_msg,
                "notebook_path": notebook_path,
                "cell_index": cell_index,
                "edit_mode": edit_mode,
            }),
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
    if let Some(n) = cell_id.strip_prefix("cell-") {
        if let Ok(idx) = n.parse::<usize>() {
            return Ok(idx);
        }
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
