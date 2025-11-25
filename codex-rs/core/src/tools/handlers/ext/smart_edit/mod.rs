//! Smart Edit Handler - Instruction-based code editing with intelligent matching
//!
//! This module provides the SmartEditHandler which implements instruction-based
//! file editing with three-tier matching strategies and LLM-powered correction.
//!
//! ## Features
//! - Three-tier matching: Exact → Flexible → Regex
//! - Semantic LLM correction with instruction context
//! - Concurrent modification detection (SHA256)
//! - Line ending preservation (CRLF/LF)
//! - Indentation preservation

pub(crate) mod common;
mod correction;
mod strategies;

use crate::error::CodexErr;
use crate::function_tool::FunctionCallError;
use crate::tools::context::ToolInvocation;
use crate::tools::context::ToolOutput;
use crate::tools::context::ToolPayload;
use crate::tools::registry::ToolHandler;
use crate::tools::registry::ToolKind;
use async_trait::async_trait;
use common::detect_line_ending;
use common::hash_content;
use correction::attempt_llm_correction;
use serde::Deserialize;
use serde::Serialize;
use std::fs;
use strategies::ReplacementResult;
use strategies::try_all_strategies;

/// Smart Edit tool arguments
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SmartEditArgs {
    pub file_path: String,
    pub instruction: String, // Key differentiator: semantic context
    pub old_string: String,
    pub new_string: String,

    #[serde(default = "default_expected")]
    pub expected_replacements: i32,
}

fn default_expected() -> i32 {
    1
}

/// Smart Edit Handler (stateless)
///
/// Uses ModelClient from turn context - no configuration needed.
pub struct SmartEditHandler;

#[async_trait]
impl ToolHandler for SmartEditHandler {
    fn kind(&self) -> ToolKind {
        ToolKind::Function
    }

    fn matches_kind(&self, payload: &ToolPayload) -> bool {
        matches!(payload, ToolPayload::Function { .. })
    }

    async fn handle(&self, invocation: ToolInvocation) -> Result<ToolOutput, FunctionCallError> {
        // 1. Parse and validate arguments
        let arguments = match &invocation.payload {
            ToolPayload::Function { arguments } => arguments,
            _ => {
                return Err(FunctionCallError::RespondToModel(
                    "Invalid payload type for smart_edit".to_string(),
                ))
            }
        };

        let args: SmartEditArgs = serde_json::from_str(arguments)
            .map_err(|e| FunctionCallError::RespondToModel(format!("Invalid arguments: {e}")))?;

        validate_args(&args)?;

        // 2. Resolve file path
        let file_path = invocation.turn.resolve_path(Some(args.file_path));

        // 3. Handle file creation (empty old_string)
        if args.old_string.is_empty() {
            return create_new_file(&file_path, &args.new_string);
        }

        // 4. Read file
        let (content, line_ending) = read_file_with_line_ending(&file_path)?;

        // 5. Normalize and compute hash
        let normalized = content.replace("\r\n", "\n");
        let initial_hash = hash_content(&normalized);

        // 6. Try three-tier strategies
        let result = try_all_strategies(&args.old_string, &args.new_string, &normalized);

        if check_success(&result, args.expected_replacements) {
            // Success! Write file and return
            return write_file_and_respond(&file_path, &result, &line_ending);
        }

        // 7. Detect concurrent modifications
        let (content_for_llm, error_msg) =
            detect_concurrent_modification(&file_path, &normalized, &initial_hash, &result)?;

        // 8. LLM correction with instruction context
        let corrected = attempt_llm_correction(
            &invocation.turn.client, // Use existing ModelClient
            &args.instruction,
            &args.old_string,
            &args.new_string,
            &content_for_llm,
            &error_msg,
        )
        .await
        .map_err(|e| FunctionCallError::RespondToModel(format!("LLM correction failed: {e}")))?;

        // Check if no changes required
        if corrected.no_changes_required {
            return Ok(ToolOutput::Function {
                content: format!("No changes needed: {}", corrected.explanation),
                content_items: None,
                success: Some(true),
            });
        }

        // 9. Retry with corrected parameters
        let retry_result =
            try_all_strategies(&corrected.search, &corrected.replace, &content_for_llm);

        if check_success(&retry_result, args.expected_replacements) {
            write_file_with_explanation(
                &file_path,
                &retry_result,
                &line_ending,
                &corrected.explanation,
            )
        } else {
            Err(FunctionCallError::RespondToModel(format!(
                "Edit failed after LLM correction. {}\n\
                 LLM explanation: {}\n\
                 Found {} occurrences (expected {}).",
                error_msg,
                corrected.explanation,
                retry_result.occurrences,
                args.expected_replacements
            )))
        }
    }
}

/// Validate arguments
fn validate_args(args: &SmartEditArgs) -> Result<(), FunctionCallError> {
    if args.expected_replacements < 1 {
        return Err(FunctionCallError::RespondToModel(
            "expected_replacements must be at least 1".to_string(),
        ));
    }

    if !args.old_string.is_empty() && args.old_string == args.new_string {
        return Err(FunctionCallError::RespondToModel(
            "old_string and new_string cannot be identical (no change would occur)".to_string(),
        ));
    }

    Ok(())
}

/// Check if replacement result matches expected count
fn check_success(result: &ReplacementResult, expected: i32) -> bool {
    result.occurrences == expected
}

/// Create a new file with the given content
fn create_new_file(
    file_path: &std::path::Path,
    content: &str,
) -> Result<ToolOutput, FunctionCallError> {
    fs::write(file_path, content).map_err(|e| {
        FunctionCallError::RespondToModel(format!(
            "Failed to create file {}: {e}",
            file_path.display()
        ))
    })?;

    Ok(ToolOutput::Function {
        content: format!("Created new file: {}", file_path.display()),
        content_items: None,
        success: Some(true),
    })
}

/// Read file and detect line ending
fn read_file_with_line_ending(
    file_path: &std::path::Path,
) -> Result<(String, &'static str), FunctionCallError> {
    let content = fs::read_to_string(file_path).map_err(|e| {
        FunctionCallError::RespondToModel(format!(
            "Failed to read file {}: {e}",
            file_path.display()
        ))
    })?;

    let line_ending = detect_line_ending(&content);
    Ok((content, line_ending))
}

/// Write file and return success response
fn write_file_and_respond(
    file_path: &std::path::Path,
    result: &ReplacementResult,
    original_line_ending: &str,
) -> Result<ToolOutput, FunctionCallError> {
    // Restore line ending
    let final_content = if original_line_ending == "\r\n" {
        result.new_content.replace('\n', "\r\n")
    } else {
        result.new_content.clone()
    };

    // Write file
    fs::write(file_path, &final_content).map_err(|e| {
        FunctionCallError::RespondToModel(format!(
            "Failed to write file {}: {e}",
            file_path.display()
        ))
    })?;

    Ok(ToolOutput::Function {
        content: format!(
            "Successfully edited {} using {} strategy ({} occurrence{})",
            file_path.display(),
            result.strategy,
            result.occurrences,
            if result.occurrences == 1 { "" } else { "s" }
        ),
        content_items: None,
        success: Some(true),
    })
}

/// Write file with LLM explanation
fn write_file_with_explanation(
    file_path: &std::path::Path,
    result: &ReplacementResult,
    original_line_ending: &str,
    explanation: &str,
) -> Result<ToolOutput, FunctionCallError> {
    // Restore line ending
    let final_content = if original_line_ending == "\r\n" {
        result.new_content.replace('\n', "\r\n")
    } else {
        result.new_content.clone()
    };

    // Write file
    fs::write(file_path, &final_content).map_err(|e| {
        FunctionCallError::RespondToModel(format!(
            "Failed to write file {}: {e}",
            file_path.display()
        ))
    })?;

    Ok(ToolOutput::Function {
        content: format!(
            "Successfully edited {} using {} strategy after LLM correction.\n\
             Occurrences: {}\n\
             Correction: {}",
            file_path.display(),
            result.strategy,
            result.occurrences,
            explanation
        ),
        content_items: None,
        success: Some(true),
    })
}

/// Detect concurrent modifications
fn detect_concurrent_modification(
    file_path: &std::path::Path,
    original_content: &str,
    initial_hash: &str,
    result: &ReplacementResult,
) -> Result<(String, String), FunctionCallError> {
    let error_msg = format!(
        "Found {} occurrences (expected different count or no match)",
        result.occurrences
    );

    // Re-read file from disk
    let on_disk_content = match fs::read_to_string(file_path) {
        Ok(content) => content,
        Err(_) => {
            // File disappeared - use original content
            return Ok((original_content.to_string(), error_msg));
        }
    };

    let on_disk_normalized = on_disk_content.replace("\r\n", "\n");
    let on_disk_hash = hash_content(&on_disk_normalized);

    if initial_hash != on_disk_hash {
        // File was modified externally → use latest version for LLM correction
        Ok((
            on_disk_normalized,
            format!(
                "File modified externally. Using latest version. Original error: {}",
                error_msg
            ),
        ))
    } else {
        // File unchanged → use original content
        Ok((original_content.to_string(), error_msg))
    }
}

/// Convert CodexErr to FunctionCallError
fn codex_err_to_function_call_error(e: CodexErr) -> FunctionCallError {
    FunctionCallError::RespondToModel(e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_args_valid() {
        let valid = SmartEditArgs {
            file_path: "test.rs".into(),
            instruction: "Update value".into(),
            old_string: "old".into(),
            new_string: "new".into(),
            expected_replacements: 1,
        };
        assert!(validate_args(&valid).is_ok());
    }

    #[test]
    fn test_validate_args_invalid_count() {
        let invalid = SmartEditArgs {
            file_path: "test.rs".into(),
            instruction: "Update".into(),
            old_string: "old".into(),
            new_string: "new".into(),
            expected_replacements: 0,
        };
        assert!(validate_args(&invalid).is_err());
    }

    #[test]
    fn test_validate_args_same_strings() {
        let invalid = SmartEditArgs {
            file_path: "test.rs".into(),
            instruction: "Update".into(),
            old_string: "same".into(),
            new_string: "same".into(),
            expected_replacements: 1,
        };
        assert!(validate_args(&invalid).is_err());
    }

    #[test]
    fn test_validate_args_empty_old_string_allowed() {
        let valid = SmartEditArgs {
            file_path: "test.rs".into(),
            instruction: "Create file".into(),
            old_string: "".into(),
            new_string: "new content".into(),
            expected_replacements: 1,
        };
        // Empty old_string is allowed (creates new file)
        assert!(validate_args(&valid).is_ok());
    }

    #[test]
    fn test_check_success() {
        let result = ReplacementResult {
            new_content: "updated".to_string(),
            occurrences: 2,
            strategy: "exact".to_string(),
        };

        assert!(check_success(&result, 2));
        assert!(!check_success(&result, 1));
        assert!(!check_success(&result, 3));
    }
}
