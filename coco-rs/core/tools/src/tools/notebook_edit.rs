//! NotebookEditTool — Jupyter `.ipynb` cell editing.
//!
//! TS: `tools/NotebookEditTool/NotebookEditTool.ts:90-433` — full
//! Jupyter notebook cell editing with replace/insert/delete modes,
//! cell ID and index lookup, output clearing on replace, and
//! nbformat-aware cell ID generation.
//!
//! Wire shape (TS:50-55 + :44-48):
//!   - notebook_path  : string (required, absolute .ipynb path)
//!   - cell_id        : string (required, cell UUID or "cell-N" index)
//!   - new_source     : string (content for replace/insert)
//!   - cell_type      : enum { code, markdown } (required for insert)
//!     **raw is NOT supported** — matches TS limitation
//!   - edit_mode      : enum { replace, insert, delete }
//!
//! Cell ID generation (TS:381-386): uses
//! `Math.random().toString(36).substring(2, 15)` — a 13-char alphanumeric
//! base-36 string — and only applies when the notebook's nbformat is ≥
//! 4.5. We do the same with rand::thread_rng so new cells round-trip
//! identically between TS and Rust writers.
//!
//! File-history hook: `track_file_edit` runs before the
//! `tokio::fs::write` so a pre-edit backup is captured for the rewind
//! subsystem (TS: `NotebookEditTool.ts:312` calls `fileHistoryTrackEdit`).

use std::collections::HashMap;

use async_trait::async_trait;
use coco_messages::ToolResult;
use coco_tool_runtime::DescriptionOptions;
use coco_tool_runtime::Tool;
use coco_tool_runtime::ToolError;
use coco_tool_runtime::ToolResultContentPart;
use coco_tool_runtime::ToolUseContext;
use coco_types::ToolId;
use coco_types::ToolInputSchema;
use coco_types::ToolName;
use serde_json::Value;

pub struct NotebookEditTool;

#[async_trait]
impl Tool for NotebookEditTool {
    fn id(&self) -> ToolId {
        ToolId::Builtin(ToolName::NotebookEdit)
    }
    fn name(&self) -> &str {
        ToolName::NotebookEdit.as_str()
    }
    fn is_enabled(&self, ctx: &ToolUseContext) -> bool {
        ctx.features.enabled(coco_types::Feature::NotebookEdit)
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

    /// Render the edit envelope as the prebuilt `message` field so the
    /// model gets the human-readable summary directly. notebook_path /
    /// cell_index / cell_id are TUI/state concerns.
    fn render_for_model(&self, data: &Value) -> Vec<ToolResultContentPart> {
        let text = data
            .get("message")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| serde_json::to_string(data).unwrap_or_default());
        vec![ToolResultContentPart::Text {
            text,
            provider_options: None,
        }]
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

        // TS `NotebookEditTool.ts:380-390`: the id surfaced to the
        // model is `cell_id` from input for replace/delete and a
        // freshly-generated 13-char base-36 string for insert — but
        // only when nbformat ≥ 4.5. Older notebooks have no cell-id
        // field, so TS renders the literal string `undefined`.
        let supports_ids = nbformat > 4 || (nbformat == 4 && nbformat_minor >= 5);
        let displayed_cell_id = |id: Option<&str>| -> String {
            match id {
                Some(s) if supports_ids && !s.is_empty() => s.to_string(),
                _ => "undefined".to_string(),
            }
        };

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

                let id = displayed_cell_id(Some(cell_id));
                format!("Updated cell {id} with {new_source}")
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
                if supports_ids {
                    let generated = generate_cell_id();
                    new_cell["id"] = Value::String(generated.clone());
                    new_cell_id = Some(generated);
                }

                let insert_at = cell_index.min(cells.len());
                cells.insert(insert_at, new_cell);

                let id = displayed_cell_id(new_cell_id.as_deref());
                format!("Inserted cell {id} with {new_source}")
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
                let id = displayed_cell_id(Some(cell_id));
                format!("Deleted cell {id}")
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

        // Sandboxed write fence (memory-extraction / auto-dream
        // subagents). No-op when `ctx.allowed_write_roots` is empty.
        if let Some(err) = crate::check_write_root_fence(ctx, std::path::Path::new(notebook_path)) {
            return Err(ToolError::ExecutionFailed {
                message: err,
                source: None,
            });
        }

        // Capture pre-edit content for file history before mutating.
        // TS: NotebookEditTool.ts:312 calls fileHistoryTrackEdit
        // before serialization. Mirrors Edit/Write/apply_patch ordering.
        crate::track_file_edit(ctx, std::path::Path::new(notebook_path)).await;

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
            app_state_patch: None,
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
#[path = "notebook_edit.test.rs"]
mod tests;
