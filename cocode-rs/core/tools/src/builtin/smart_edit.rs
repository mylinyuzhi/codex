//! SmartEdit tool — LLM-assisted edit correction.
//!
//! An enhanced Edit tool that falls back to an LLM to correct matching
//! failures. When all string-matching tiers (exact, flexible, regex, trim)
//! fail, SmartEdit sends the file content, the failed search/replace pair,
//! and the user's semantic intent to a lightweight model call. The model
//! returns a corrected search/replace pair which is then re-attempted.
//!
//! Feature-gated behind `Feature::SmartEdit` (experimental, default off).

use super::edit_strategies::MatchStrategy;
use super::edit_strategies::diff_stats;
use super::edit_strategies::find_closest_match;
use super::edit_strategies::pre_correct_escaping;
use super::edit_strategies::trim_pair_if_possible;
use super::edit_strategies::try_match;
use super::prompts;
use crate::context::FileReadState;
use crate::context::ModelCallInput;
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
use cocode_protocol::Feature;
use cocode_protocol::PermissionResult;
use cocode_protocol::RiskSeverity;
use cocode_protocol::RiskType;
use cocode_protocol::SecurityRisk;
use cocode_protocol::ToolOutput;
use serde::Deserialize;
use serde_json::Value;
use tokio::fs;

/// LLM correction timeout.
const CORRECTION_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// System prompt for the correction LLM call.
const CORRECTION_SYSTEM_PROMPT: &str = "\
You are an expert code-editing assistant that fixes failed search-and-replace operations.

Given:
- The user's semantic edit instruction
- A failed old_string/new_string pair
- The error message explaining why it failed
- The full file content

Your job is to produce a CORRECTED search/replace pair that will succeed against the actual file content.

Rules:
- The corrected `search` MUST be an exact substring of the file content.
- Make minimal corrections — focus on whitespace, indentation, and context differences.
- If the edit is already applied or no changes are needed, set `no_changes_required` to true.
- Always explain your reasoning in `explanation`.";

/// Tool for performing string replacements with LLM correction fallback.
///
/// When standard matching strategies fail, SmartEdit uses a lightweight LLM
/// call to correct the search/replace pair before retrying.
pub struct SmartEditTool;

impl SmartEditTool {
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
        if path.exists() {
            return Err(crate::error::tool_error::ExecutionFailedSnafu {
                message: format!(
                    "Cannot create file: {} already exists. Use non-empty old_string to edit existing files.",
                    path.display()
                ),
            }
            .build());
        }

        if ctx.is_plan_mode && !is_safe_file(path, ctx.plan_file_path.as_deref()) {
            return Err(crate::error::tool_error::ExecutionFailedSnafu {
                message: format!(
                    "Plan mode: cannot create '{}'. Only the plan file can be modified during plan mode.",
                    path.display()
                ),
            }
            .build());
        }

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

        write_with_format_async(path, new_string, Encoding::Utf8, LineEnding::Lf)
            .await
            .map_err(|e| {
                crate::error::tool_error::ExecutionFailedSnafu {
                    message: format!("Failed to write file: {e}"),
                }
                .build()
            })?;

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

impl Default for SmartEditTool {
    fn default() -> Self {
        Self::new()
    }
}

/// Result of the LLM correction call.
#[derive(Debug, Deserialize)]
struct CorrectionResult {
    search: String,
    replace: String,
    no_changes_required: bool,
    #[allow(dead_code)]
    explanation: String,
}

/// Build the user prompt for the correction LLM call.
fn build_correction_user_prompt(
    instruction: &str,
    old_string: &str,
    new_string: &str,
    error_msg: &str,
    file_content: &str,
) -> String {
    format!(
        "## Edit Instruction\n{instruction}\n\n\
         ## Failed old_string\n```\n{old_string}\n```\n\n\
         ## Failed new_string\n```\n{new_string}\n```\n\n\
         ## Error\n{error_msg}\n\n\
         ## File Content\n```\n{file_content}\n```"
    )
}

/// JSON schema for the correction result.
fn correction_schema() -> Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "search": {
                "type": "string",
                "description": "Corrected search string that is an exact substring of the file content"
            },
            "replace": {
                "type": "string",
                "description": "Corrected replacement string"
            },
            "no_changes_required": {
                "type": "boolean",
                "description": "True if the edit is already applied or not needed"
            },
            "explanation": {
                "type": "string",
                "description": "Brief explanation of what was corrected"
            }
        },
        "required": ["search", "replace", "no_changes_required", "explanation"]
    })
}

/// Write back a successful edit result and return the tool output.
async fn write_edit_result(
    path: &std::path::Path,
    content: &str,
    replaced_content: &str,
    match_strategy: MatchStrategy,
    encoding: Encoding,
    line_ending: LineEnding,
    ctx: &mut ToolContext,
    extra_note: &str,
) -> Result<ToolOutput> {
    let new_content = preserve_trailing_newline(content, replaced_content);
    write_with_format_async(path, &new_content, encoding, line_ending)
        .await
        .map_err(|e| {
            crate::error::tool_error::ExecutionFailedSnafu {
                message: format!("Failed to write file: {e}"),
            }
            .build()
        })?;

    let normalized_content = normalize_line_endings(&new_content, LineEnding::Lf);
    ctx.record_file_modified(path).await;
    let new_mtime = fs::metadata(path)
        .await
        .ok()
        .and_then(|m| m.modified().ok());
    ctx.record_file_read_with_state(
        path,
        FileReadState::complete(normalized_content.clone(), new_mtime),
    )
    .await;

    let stats = diff_stats(content, &new_content);
    let strategy_note = match match_strategy {
        MatchStrategy::Exact => String::new(),
        other => format!(" (matched via {other} strategy)"),
    };
    let mut result = ToolOutput::text(format!(
        "Successfully edited {}{stats}{strategy_note}{extra_note}",
        path.display()
    ));
    result.modifiers.push(ContextModifier::FileRead {
        path: path.to_path_buf(),
        content: normalized_content,
    });

    Ok(result)
}

#[async_trait]
impl Tool for SmartEditTool {
    fn name(&self) -> &str {
        "SmartEdit"
    }

    fn description(&self) -> &str {
        prompts::SMART_EDIT_DESCRIPTION
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
                "instruction": {
                    "type": "string",
                    "description": "Semantic description of the intended edit (e.g., 'rename variable x to y')"
                },
                "replace_all": {
                    "type": "boolean",
                    "description": "Replace all occurrences (default false)",
                    "default": false
                }
            },
            "required": ["file_path", "old_string", "new_string", "instruction"]
        })
    }

    fn concurrency_safety(&self) -> ConcurrencySafety {
        ConcurrencySafety::Unsafe
    }

    fn is_read_only(&self) -> bool {
        false
    }

    fn feature_gate(&self) -> Option<Feature> {
        Some(Feature::SmartEdit)
    }

    async fn check_permission(&self, input: &Value, ctx: &ToolContext) -> PermissionResult {
        if let Some(path_str) = input.get("file_path").and_then(|v| v.as_str()) {
            let path = ctx.resolve_path(path_str);

            if crate::sensitive_files::is_locked_directory(&path) {
                return PermissionResult::Denied {
                    reason: format!(
                        "Editing files in locked directory is not allowed: {}",
                        path.display()
                    ),
                };
            }

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

            if crate::sensitive_files::is_sensitive_file(&path) {
                return PermissionResult::NeedsApproval {
                    request: ApprovalRequest {
                        request_id: format!("sensitive-smart-edit-{}", path.display()),
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

            if crate::sensitive_files::is_sensitive_directory(&path) {
                return PermissionResult::NeedsApproval {
                    request: ApprovalRequest {
                        request_id: format!("sensitive-dir-smart-edit-{}", path.display()),
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

        PermissionResult::NeedsApproval {
            request: ApprovalRequest {
                request_id: format!(
                    "smart-edit-{}",
                    input
                        .get("file_path")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown")
                ),
                tool_name: self.name().to_string(),
                description: format!(
                    "SmartEdit: {}",
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
        let instruction = input["instruction"].as_str().ok_or_else(|| {
            crate::error::tool_error::InvalidInputSnafu {
                message: "instruction must be a string",
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

        if path.extension().is_some_and(|ext| ext == "ipynb") {
            return Err(crate::error::tool_error::ExecutionFailedSnafu {
                message: format!(
                    "Cannot use SmartEdit tool on Jupyter notebook files. \
                     Use the NotebookEdit tool instead to modify cells in '{}'.",
                    path.display()
                ),
            }
            .build());
        }

        if ctx.is_plan_mode && !is_safe_file(&path, ctx.plan_file_path.as_deref()) {
            return Err(crate::error::tool_error::ExecutionFailedSnafu {
                message: format!(
                    "Plan mode: cannot edit '{}'. Only the plan file can be modified during plan mode.",
                    path.display()
                ),
            }
            .build());
        }

        if !ctx.was_file_read(&path).await {
            return Err(crate::error::tool_error::ExecutionFailedSnafu {
                message: format!(
                    "File must be read before editing: {}. Use the Read tool first.",
                    path.display()
                ),
            }
            .build());
        }

        // ── Read file ───────────────────────────────────────────────
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

        // ── Pre-correction + Three-tier matching ────────────────────
        let (working_old, working_new) = pre_correct_escaping(old_string, new_string, &content);
        let match_result = try_match(&content, &working_old, &working_new, replace_all);

        // ── Trim fallback ───────────────────────────────────────────
        let first_attempt = match match_result {
            Ok(ok) => Ok(ok),
            Err(_) => {
                if let Some((trimmed_old, trimmed_new)) =
                    trim_pair_if_possible(&working_old, &working_new, &content)
                {
                    try_match(&content, &trimmed_old, &trimmed_new, replace_all)
                } else {
                    Err(crate::error::tool_error::ExecutionFailedSnafu {
                        message: "no strategy matched".to_string(),
                    }
                    .build())
                }
            }
        };

        // ── On success → write file ─────────────────────────────────
        if let Ok((replaced_content, match_strategy)) = first_attempt {
            return write_edit_result(
                &path,
                &content,
                &replaced_content,
                match_strategy,
                encoding,
                line_ending,
                ctx,
                "",
            )
            .await;
        }

        // ── On failure → LLM correction fallback ────────────────────
        let model_call_fn = match ctx.model_call_fn {
            Some(ref f) => f.clone(),
            None => {
                // No model_call_fn — return the standard error
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
        };

        ctx.emit_progress("SmartEdit: invoking LLM correction...")
            .await;

        let error_msg = {
            let hint = find_closest_match(&content, &working_old);
            format!(
                "old_string not found in file (tried exact, flexible, and regex matching). Hint: {hint}"
            )
        };

        // Re-read file from disk (may have changed during LLM call setup)
        let bytes = fs::read(&path).await.map_err(|e| {
            crate::error::tool_error::ExecutionFailedSnafu {
                message: format!("Failed to re-read file: {e}"),
            }
            .build()
        })?;
        let content = encoding.decode(&bytes).map_err(|e| {
            crate::error::tool_error::ExecutionFailedSnafu {
                message: format!("Failed to decode file: {e}"),
            }
            .build()
        })?;

        let user_prompt =
            build_correction_user_prompt(instruction, old_string, new_string, &error_msg, &content);

        let request = hyper_sdk::ObjectRequest::new(
            vec![
                hyper_sdk::Message::system(CORRECTION_SYSTEM_PROMPT),
                hyper_sdk::Message::user(user_prompt),
            ],
            correction_schema(),
        )
        .schema_name("CorrectionResult")
        .max_tokens(4096);

        let call_result = tokio::time::timeout(
            CORRECTION_TIMEOUT,
            model_call_fn(ModelCallInput { request }),
        )
        .await;

        let response = match call_result {
            Ok(Ok(result)) => result.response,
            Ok(Err(e)) => {
                tracing::warn!(error = %e, "SmartEdit LLM correction call failed");
                let hint = find_closest_match(&content, &working_old);
                return Err(crate::error::tool_error::ExecutionFailedSnafu {
                    message: format!(
                        "old_string not found in file (tried exact, flexible, regex matching, and LLM correction failed: {e}): {}\n\
                         Hint: {hint}\n\
                         The file may have changed. Use the Read tool to re-read the file and verify the exact content before retrying.",
                        path.display()
                    ),
                }
                .build());
            }
            Err(_) => {
                tracing::warn!("SmartEdit LLM correction timed out");
                let hint = find_closest_match(&content, &working_old);
                return Err(crate::error::tool_error::ExecutionFailedSnafu {
                    message: format!(
                        "old_string not found in file (tried exact, flexible, regex matching, and LLM correction timed out): {}\n\
                         Hint: {hint}\n\
                         The file may have changed. Use the Read tool to re-read the file and verify the exact content before retrying.",
                        path.display()
                    ),
                }
                .build());
            }
        };

        // Parse the correction result
        let correction: CorrectionResult = response.parse().map_err(|e| {
            crate::error::tool_error::ExecutionFailedSnafu {
                message: format!("Failed to parse LLM correction response: {e}"),
            }
            .build()
        })?;

        if correction.no_changes_required {
            return Ok(ToolOutput::text(format!(
                "SmartEdit: no changes required for {}. The edit appears to already be applied.",
                path.display()
            )));
        }

        // Re-run three-tier matching with corrected search/replace
        let second_attempt = try_match(
            &content,
            &correction.search,
            &correction.replace,
            replace_all,
        );

        let second_result = match second_attempt {
            Ok(ok) => Ok(ok),
            Err(_) => {
                if let Some((trimmed_old, trimmed_new)) =
                    trim_pair_if_possible(&correction.search, &correction.replace, &content)
                {
                    try_match(&content, &trimmed_old, &trimmed_new, replace_all)
                } else {
                    Err(crate::error::tool_error::ExecutionFailedSnafu {
                        message: "no strategy matched after LLM correction".to_string(),
                    }
                    .build())
                }
            }
        };

        match second_result {
            Ok((replaced_content, match_strategy)) => {
                write_edit_result(
                    &path,
                    &content,
                    &replaced_content,
                    match_strategy,
                    encoding,
                    line_ending,
                    ctx,
                    " (via LLM correction)",
                )
                .await
            }
            Err(_) => {
                let hint = find_closest_match(&content, &working_old);
                Err(crate::error::tool_error::ExecutionFailedSnafu {
                    message: format!(
                        "old_string not found in file (tried exact, flexible, regex matching, and LLM correction): {}\n\
                         Hint: {hint}\n\
                         The file may have changed. Use the Read tool to re-read the file and verify the exact content before retrying.",
                        path.display()
                    ),
                }
                .build())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as IoWrite;
    use std::path::PathBuf;
    use tempfile::NamedTempFile;

    fn make_context() -> ToolContext {
        let mut ctx = ToolContext::new("call-1", "session-1", PathBuf::from("/tmp"));
        // Enable SmartEdit feature for tests
        ctx.features.enable(Feature::SmartEdit);
        ctx
    }

    #[test]
    fn test_tool_properties() {
        let tool = SmartEditTool::new();
        assert_eq!(tool.name(), "SmartEdit");
        assert!(!tool.is_read_only());
        assert_eq!(tool.concurrency_safety(), ConcurrencySafety::Unsafe);
        assert_eq!(tool.feature_gate(), Some(Feature::SmartEdit));
    }

    #[tokio::test]
    async fn test_smart_edit_basic() {
        let mut file = NamedTempFile::new().unwrap();
        write!(file, "Hello World").unwrap();
        let path = file.path().to_str().unwrap().to_string();

        let tool = SmartEditTool::new();
        let mut ctx = make_context();
        ctx.record_file_read(file.path()).await;

        let input = serde_json::json!({
            "file_path": path,
            "old_string": "World",
            "new_string": "Rust",
            "instruction": "Replace World with Rust"
        });

        let result = tool.execute(input, &mut ctx).await.unwrap();
        assert!(!result.is_error);

        let content = std::fs::read_to_string(file.path()).unwrap();
        assert_eq!(content, "Hello Rust");
    }

    #[tokio::test]
    async fn test_smart_edit_requires_read_first() {
        let mut file = NamedTempFile::new().unwrap();
        write!(file, "Hello World").unwrap();
        let path = file.path().to_str().unwrap().to_string();

        let tool = SmartEditTool::new();
        let mut ctx = make_context();

        let input = serde_json::json!({
            "file_path": path,
            "old_string": "World",
            "new_string": "Rust",
            "instruction": "Replace World with Rust"
        });

        let result = tool.execute(input, &mut ctx).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_smart_edit_no_model_call_fn_falls_back_to_error() {
        let mut file = NamedTempFile::new().unwrap();
        write!(file, "Hello World").unwrap();
        let path = file.path().to_str().unwrap().to_string();

        let tool = SmartEditTool::new();
        let mut ctx = make_context();
        ctx.record_file_read(file.path()).await;

        // old_string doesn't match anything in the file
        let input = serde_json::json!({
            "file_path": path,
            "old_string": "Nonexistent Content",
            "new_string": "Replacement",
            "instruction": "Replace nonexistent content"
        });

        let result = tool.execute(input, &mut ctx).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("not found in file"));
    }

    #[tokio::test]
    async fn test_smart_edit_with_mock_model_call() {
        use crate::context::ModelCallResult;
        use std::sync::Arc;

        let mut file = NamedTempFile::new().unwrap();
        write!(file, "fn main() {{\n    let x = 1;\n}}").unwrap();
        let path = file.path().to_str().unwrap().to_string();

        let tool = SmartEditTool::new();
        let mut ctx = make_context();
        ctx.record_file_read(file.path()).await;

        // Set up mock model_call_fn that returns a corrected search/replace
        let mock_fn: crate::context::ModelCallFn = Arc::new(|_input| {
            Box::pin(async {
                Ok(ModelCallResult {
                    response: hyper_sdk::ObjectResponse::new(
                        "test-id",
                        "test-model",
                        serde_json::json!({
                            "search": "    let x = 1;",
                            "replace": "    let x = 42;",
                            "no_changes_required": false,
                            "explanation": "Fixed variable name in search string"
                        }),
                    ),
                })
            })
        });
        ctx.model_call_fn = Some(mock_fn);

        // Use a completely wrong variable name that won't match any strategy
        let input = serde_json::json!({
            "file_path": path,
            "old_string": "    let y = 1;",  // wrong variable name (y vs x)
            "new_string": "    let x = 42;",
            "instruction": "Change x from 1 to 42"
        });

        let result = tool.execute(input, &mut ctx).await.unwrap();
        assert!(!result.is_error);

        let content = std::fs::read_to_string(file.path()).unwrap();
        assert!(content.contains("let x = 42;"));
        let text = match &result.content {
            cocode_protocol::ToolResultContent::Text(t) => t,
            _ => panic!("Expected text"),
        };
        assert!(text.contains("LLM correction"));
    }

    #[tokio::test]
    async fn test_smart_edit_no_changes_required() {
        use crate::context::ModelCallResult;
        use std::sync::Arc;

        let mut file = NamedTempFile::new().unwrap();
        write!(file, "fn main() {{\n    let x = 42;\n}}").unwrap();
        let path = file.path().to_str().unwrap().to_string();

        let tool = SmartEditTool::new();
        let mut ctx = make_context();
        ctx.record_file_read(file.path()).await;

        let mock_fn: crate::context::ModelCallFn = Arc::new(|_input| {
            Box::pin(async {
                Ok(ModelCallResult {
                    response: hyper_sdk::ObjectResponse::new(
                        "test-id",
                        "test-model",
                        serde_json::json!({
                            "search": "",
                            "replace": "",
                            "no_changes_required": true,
                            "explanation": "The edit is already applied"
                        }),
                    ),
                })
            })
        });
        ctx.model_call_fn = Some(mock_fn);

        let input = serde_json::json!({
            "file_path": path,
            "old_string": "let x = 1;",
            "new_string": "let x = 42;",
            "instruction": "Change x from 1 to 42"
        });

        let result = tool.execute(input, &mut ctx).await.unwrap();
        assert!(!result.is_error);
        let text = match &result.content {
            cocode_protocol::ToolResultContent::Text(t) => t,
            _ => panic!("Expected text"),
        };
        assert!(text.contains("no changes required"));
    }

    #[tokio::test]
    async fn test_smart_edit_requires_instruction() {
        let tool = SmartEditTool::new();
        let mut ctx = make_context();

        let input = serde_json::json!({
            "file_path": "/tmp/test.txt",
            "old_string": "foo",
            "new_string": "bar"
            // missing instruction
        });

        let result = tool.execute(input, &mut ctx).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("instruction"));
    }
}
