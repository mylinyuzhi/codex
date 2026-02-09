//! Edit tool for string replacement in files.
//!
//! Supports three matching strategies (tried in order):
//! 1. **Exact** — precise string matching (default)
//! 2. **Flexible** — whitespace-tolerant fallback when exact match fails
//! 3. **Regex** — token-based fuzzy matching (first occurrence only)
//!
//! Also supports file creation via `old_string == ""` and SHA256-based
//! concurrent modification detection.

use super::edit_strategies::MatchStrategy;
use super::edit_strategies::diff_stats;
use super::edit_strategies::find_closest_match;
use super::edit_strategies::pre_correct_escaping;
use super::edit_strategies::trim_pair_if_possible;
use super::edit_strategies::try_match;
use super::prompts;
use crate::context::FileReadState;
use crate::context::ToolContext;
use crate::error::Result;
use crate::tool::Tool;
use async_trait::async_trait;
use cocode_file_encoding::Encoding;
use cocode_file_encoding::LineEnding;
use cocode_file_encoding::detect_encoding;
use cocode_file_encoding::detect_line_ending;
use cocode_file_encoding::normalize_line_endings;
use cocode_file_encoding::preserve_trailing_newline;
use cocode_file_encoding::write_with_format_async;
use cocode_plan_mode::is_safe_file;
use cocode_protocol::ApprovalRequest;
use cocode_protocol::ConcurrencySafety;
use cocode_protocol::ContextModifier;
use cocode_protocol::PermissionResult;
use cocode_protocol::RiskSeverity;
use cocode_protocol::RiskType;
use cocode_protocol::SecurityRisk;
use cocode_protocol::ToolOutput;
use serde_json::Value;
use tokio::fs;

/// Tool for performing string replacements in files.
///
/// Requires the file to have been read first (tracked via FileTracker).
/// Supports file creation when `old_string` is empty.
pub struct EditTool;

impl EditTool {
    /// Create a new Edit tool.
    pub fn new() -> Self {
        Self
    }

    /// Create a new file (when `old_string == ""`).
    async fn create_new_file(
        &self,
        path: &std::path::Path,
        new_string: &str,
        ctx: &mut ToolContext,
    ) -> Result<ToolOutput> {
        // Reject if file already exists
        if path.exists() {
            return Err(crate::error::tool_error::ExecutionFailedSnafu {
                message: format!(
                    "Cannot create file: {} already exists. Use non-empty old_string to edit existing files.",
                    path.display()
                ),
            }
            .build());
        }

        // Plan mode check
        if ctx.is_plan_mode && !is_safe_file(path, ctx.plan_file_path.as_deref()) {
            return Err(crate::error::tool_error::ExecutionFailedSnafu {
                message: format!(
                    "Plan mode: cannot create '{}'. Only the plan file can be modified during plan mode.",
                    path.display()
                ),
            }
            .build());
        }

        // Create parent directories
        if let Some(parent) = path.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent).await.map_err(|e| {
                    crate::error::tool_error::ExecutionFailedSnafu {
                        message: format!("Failed to create directory: {e}"),
                    }
                    .build()
                })?;
            }
        }

        // Write file with UTF-8 / LF defaults (same as Write tool for new files)
        write_with_format_async(path, new_string, Encoding::Utf8, LineEnding::Lf)
            .await
            .map_err(|e| {
                crate::error::tool_error::ExecutionFailedSnafu {
                    message: format!("Failed to write file: {e}"),
                }
                .build()
            })?;

        // Track modification and update read state
        let normalized = normalize_line_endings(new_string, LineEnding::Lf);
        ctx.record_file_modified(path).await;
        let new_mtime = fs::metadata(path)
            .await
            .ok()
            .and_then(|m| m.modified().ok());
        ctx.record_file_read_with_state(
            path,
            FileReadState::complete(normalized.clone(), new_mtime),
        )
        .await;

        let mut result = ToolOutput::text(format!("Created new file: {}", path.display()));
        result.modifiers.push(ContextModifier::FileRead {
            path: path.to_path_buf(),
            content: normalized,
        });
        Ok(result)
    }
}

impl Default for EditTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for EditTool {
    fn name(&self) -> &str {
        "Edit"
    }

    fn description(&self) -> &str {
        prompts::EDIT_DESCRIPTION
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "file_path": {
                    "type": "string",
                    "description": "The absolute path to the file to modify"
                },
                "old_string": {
                    "type": "string",
                    "description": "The text to replace. Use an empty string to create a new file."
                },
                "new_string": {
                    "type": "string",
                    "description": "The text to replace it with (must be different from old_string)"
                },
                "replace_all": {
                    "type": "boolean",
                    "description": "Replace all occurrences (default false)",
                    "default": false
                }
            },
            "required": ["file_path", "old_string", "new_string"]
        })
    }

    fn concurrency_safety(&self) -> ConcurrencySafety {
        ConcurrencySafety::Unsafe
    }

    fn is_read_only(&self) -> bool {
        false
    }

    async fn check_permission(&self, input: &Value, ctx: &ToolContext) -> PermissionResult {
        if let Some(path_str) = input.get("file_path").and_then(|v| v.as_str()) {
            let path = ctx.resolve_path(path_str);

            // Locked directory → Deny
            if crate::sensitive_files::is_locked_directory(&path) {
                return PermissionResult::Denied {
                    reason: format!(
                        "Editing files in locked directory is not allowed: {}",
                        path.display()
                    ),
                };
            }

            // Plan mode: only plan file allowed
            if ctx.is_plan_mode {
                if let Some(ref plan_file) = ctx.plan_file_path {
                    if path != *plan_file {
                        return PermissionResult::Denied {
                            reason: format!(
                                "Plan mode: cannot edit '{}'. Only the plan file can be modified.",
                                path.display()
                            ),
                        };
                    }
                }
            }

            // Sensitive file → NeedsApproval (high severity)
            if crate::sensitive_files::is_sensitive_file(&path) {
                return PermissionResult::NeedsApproval {
                    request: ApprovalRequest {
                        request_id: format!("sensitive-edit-{}", path.display()),
                        tool_name: self.name().to_string(),
                        description: format!("Modifying sensitive file: {}", path.display()),
                        risks: vec![SecurityRisk {
                            risk_type: RiskType::SensitiveFile,
                            severity: RiskSeverity::High,
                            message: format!(
                                "File '{}' may contain credentials or sensitive configuration",
                                path.display()
                            ),
                        }],
                        allow_remember: true,
                        proposed_prefix_pattern: None,
                    },
                };
            }

            // Sensitive directory (.git/, .vscode/, .idea/) → NeedsApproval
            if crate::sensitive_files::is_sensitive_directory(&path) {
                return PermissionResult::NeedsApproval {
                    request: ApprovalRequest {
                        request_id: format!("sensitive-dir-edit-{}", path.display()),
                        tool_name: self.name().to_string(),
                        description: format!(
                            "Editing file in sensitive directory: {}",
                            path.display()
                        ),
                        risks: vec![SecurityRisk {
                            risk_type: RiskType::SystemConfig,
                            severity: RiskSeverity::Medium,
                            message: format!(
                                "Directory '{}' contains project configuration",
                                path.display()
                            ),
                        }],
                        allow_remember: true,
                        proposed_prefix_pattern: None,
                    },
                };
            }
        }

        // All edits default to NeedsApproval
        PermissionResult::NeedsApproval {
            request: ApprovalRequest {
                request_id: format!(
                    "edit-{}",
                    input
                        .get("file_path")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown")
                ),
                tool_name: self.name().to_string(),
                description: format!(
                    "Edit: {}",
                    input
                        .get("file_path")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown")
                ),
                risks: vec![],
                allow_remember: true,
                proposed_prefix_pattern: None,
            },
        }
    }

    async fn execute(&self, input: Value, ctx: &mut ToolContext) -> Result<ToolOutput> {
        // ── Parse inputs ────────────────────────────────────────────
        let file_path = input["file_path"].as_str().ok_or_else(|| {
            crate::error::tool_error::InvalidInputSnafu {
                message: "file_path must be a string",
            }
            .build()
        })?;
        let old_string = input["old_string"].as_str().ok_or_else(|| {
            crate::error::tool_error::InvalidInputSnafu {
                message: "old_string must be a string",
            }
            .build()
        })?;
        let new_string = input["new_string"].as_str().ok_or_else(|| {
            crate::error::tool_error::InvalidInputSnafu {
                message: "new_string must be a string",
            }
            .build()
        })?;
        let replace_all = input["replace_all"].as_bool().unwrap_or(false);

        let path = ctx.resolve_path(file_path);

        // ── File creation (old_string == "") ────────────────────────
        if old_string.is_empty() {
            return self.create_new_file(&path, new_string, ctx).await;
        }

        // ── Validation ──────────────────────────────────────────────
        if old_string == new_string {
            return Err(crate::error::tool_error::InvalidInputSnafu {
                message: "old_string and new_string must be different",
            }
            .build());
        }

        // Check for .ipynb files - redirect to NotebookEdit
        if path.extension().is_some_and(|ext| ext == "ipynb") {
            return Err(crate::error::tool_error::ExecutionFailedSnafu {
                message: format!(
                    "Cannot use Edit tool on Jupyter notebook files. \
                     Use the NotebookEdit tool instead to modify cells in '{}'.",
                    path.display()
                ),
            }
            .build());
        }

        // Plan mode check
        if ctx.is_plan_mode && !is_safe_file(&path, ctx.plan_file_path.as_deref()) {
            return Err(crate::error::tool_error::ExecutionFailedSnafu {
                message: format!(
                    "Plan mode: cannot edit '{}'. Only the plan file can be modified during plan mode.",
                    path.display()
                ),
            }
            .build());
        }

        // Verify file was read first
        if !ctx.was_file_read(&path).await {
            return Err(crate::error::tool_error::ExecutionFailedSnafu {
                message: format!(
                    "File must be read before editing: {}. Use the Read tool first.",
                    path.display()
                ),
            }
            .build());
        }

        // ── Read file once for both staleness check and editing ─────
        let bytes = fs::read(&path).await.map_err(|e| {
            crate::error::tool_error::ExecutionFailedSnafu {
                message: format!("Failed to read file: {e}"),
            }
            .build()
        })?;
        let encoding = detect_encoding(&bytes);
        let content = encoding.decode(&bytes).map_err(|e| {
            crate::error::tool_error::ExecutionFailedSnafu {
                message: format!("Failed to decode file: {e}"),
            }
            .build()
        })?;
        let line_ending = detect_line_ending(&content);

        // ── SHA256 staleness check ──────────────────────────────────
        if let Some(read_state) = ctx.file_read_state(&path).await {
            if let Some(ref stored_hash) = read_state.content_hash {
                let normalized = normalize_line_endings(&content, LineEnding::Lf);
                let current_hash = FileReadState::compute_hash(&normalized);
                if *stored_hash != current_hash {
                    return Err(crate::error::tool_error::ExecutionFailedSnafu {
                        message: format!(
                            "File has been modified externally since last read: {}. Read the file again before editing.",
                            path.display()
                        ),
                    }
                    .build());
                }
            }
        }

        // ── Pre-correction (unescape LLM bugs) ─────────────────────
        let (working_old, working_new) = pre_correct_escaping(old_string, new_string, &content);

        // ── Three-tier matching ─────────────────────────────────────
        let match_result = try_match(&content, &working_old, &working_new, replace_all);

        // ── Trim fallback ───────────────────────────────────────────
        let (replaced_content, match_strategy) = match match_result {
            Ok(ok) => ok,
            Err(_) => {
                // Try trimmed pair → re-run three-tier
                if let Some((trimmed_old, trimmed_new)) =
                    trim_pair_if_possible(&working_old, &working_new, &content)
                {
                    match try_match(&content, &trimmed_old, &trimmed_new, replace_all) {
                        Ok(ok) => ok,
                        Err(e) => return Err(e),
                    }
                } else {
                    // All strategies failed — return enhanced error
                    let hint = find_closest_match(&content, &working_old);
                    return Err(crate::error::tool_error::ExecutionFailedSnafu {
                        message: format!(
                            "old_string not found in file (tried exact, flexible, and regex matching): {}\n\
                             Hint: {hint}\n\
                             The file may have changed. Use the Read tool to re-read the file and verify the exact content before retrying.",
                            path.display()
                        ),
                    }
                    .build());
                }
            }
        };

        // ── Write back preserving encoding / line ending ────────────
        let new_content = preserve_trailing_newline(&content, &replaced_content);
        write_with_format_async(&path, &new_content, encoding, line_ending)
            .await
            .map_err(|e| {
                crate::error::tool_error::ExecutionFailedSnafu {
                    message: format!("Failed to write file: {e}"),
                }
                .build()
            })?;

        // ── Track modification and update read state ────────────────
        let normalized_content = normalize_line_endings(&new_content, LineEnding::Lf);
        ctx.record_file_modified(&path).await;
        let new_mtime = fs::metadata(&path)
            .await
            .ok()
            .and_then(|m| m.modified().ok());
        ctx.record_file_read_with_state(
            &path,
            FileReadState::complete(normalized_content.clone(), new_mtime),
        )
        .await;

        let stats = diff_stats(&content, &new_content);
        let strategy_note = match match_strategy {
            MatchStrategy::Exact => String::new(),
            other => format!(" (matched via {other} strategy)"),
        };
        let mut result = ToolOutput::text(format!(
            "Successfully edited {}{stats}{strategy_note}",
            path.display()
        ));
        result.modifiers.push(ContextModifier::FileRead {
            path: path.clone(),
            content: normalized_content,
        });

        Ok(result)
    }
}

#[cfg(test)]
#[path = "edit.test.rs"]
mod tests;
