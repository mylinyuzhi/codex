//! Smart Edit tool implementation.
//!
//! Provides intelligent file editing with automatic error correction.

use async_trait::async_trait;
use serde::Deserialize;
use std::fs;

use crate::function_tool::FunctionCallError;
use crate::tools::context::ToolInvocation;
use crate::tools::context::ToolOutput;
use crate::tools::context::ToolPayload;
use crate::tools::registry::ToolHandler;
use crate::tools::registry::ToolKind;

/// SHA256 hash for content verification
fn hash_content(content: &str) -> String {
    use sha2::Digest;
    use sha2::Sha256;
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// Detect line ending style
fn detect_line_ending(content: &str) -> &'static str {
    if content.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    }
}

pub(crate) mod correction;
pub(crate) mod strategies;

/// Smart Edit tool handler
pub struct SmartEditHandler;

/// Arguments for smart_edit tool call
#[derive(Debug, Deserialize)]
struct SmartEditArgs {
    file_path: String,
    old_string: String,
    new_string: String,
    #[serde(default = "default_expected_replacements")]
    expected_replacements: i32,
    instruction: String,
}

fn default_expected_replacements() -> i32 {
    1
}

#[async_trait]
impl ToolHandler for SmartEditHandler {
    fn kind(&self) -> ToolKind {
        ToolKind::Function
    }

    async fn handle(&self, invocation: ToolInvocation) -> Result<ToolOutput, FunctionCallError> {
        // 1. Parse arguments
        let arguments = match &invocation.payload {
            ToolPayload::Function { arguments } => arguments,
            _ => {
                return Err(FunctionCallError::RespondToModel(
                    "smart_edit requires Function payload".into(),
                ));
            }
        };

        let args: SmartEditArgs = serde_json::from_str(arguments).map_err(|e| {
            FunctionCallError::RespondToModel(format!("Failed to parse arguments: {e}"))
        })?;

        // Validate expected_replacements
        if args.expected_replacements < 1 {
            return Err(FunctionCallError::RespondToModel(
                "expected_replacements must be at least 1".into(),
            ));
        }

        // 2. Resolve file path
        let file_path = invocation.turn.resolve_path(Some(args.file_path.clone()));

        // 3. Validate arguments (before file operations)
        if args.old_string == args.new_string {
            return Err(FunctionCallError::RespondToModel(
                "No changes to apply: old_string and new_string are identical".into(),
            ));
        }

        // 4. Read file content (with new file creation support)
        let (current_content, original_line_ending, is_new_file) =
            match fs::read_to_string(&file_path) {
                Ok(content) => {
                    let line_ending = detect_line_ending(&content);
                    (content, line_ending, false)
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    // File doesn't exist - check if this is new file creation
                    if args.old_string.is_empty() {
                        // Create parent directories
                        if let Some(parent) = file_path.parent() {
                            fs::create_dir_all(parent).map_err(|e| {
                                FunctionCallError::RespondToModel(format!(
                                    "Failed to create parent directories: {e}"
                                ))
                            })?;
                        }

                        // Write new file (use content as-is since no original line ending to preserve)
                        fs::write(&file_path, &args.new_string).map_err(|e| {
                            FunctionCallError::RespondToModel(format!(
                                "Failed to write new file: {e}"
                            ))
                        })?;

                        return Ok(ToolOutput::Function {
                            content: format!("Created new file: {}", args.file_path),
                            content_items: None,
                            success: Some(true),
                        });
                    } else {
                        return Err(FunctionCallError::RespondToModel(format!(
                            "File not found: {}. Use empty old_string to create a new file.",
                            args.file_path
                        )));
                    }
                }
                Err(e) => {
                    return Err(FunctionCallError::RespondToModel(format!(
                        "Failed to read file {}: {e}",
                        args.file_path
                    )));
                }
            };

        // Check for attempting to create file that already exists
        if args.old_string.is_empty() && !is_new_file {
            return Err(FunctionCallError::RespondToModel(format!(
                "File already exists: {}. Cannot create.",
                args.file_path
            )));
        }

        // 5. Compute initial content hash (for concurrent modification detection)
        let initial_content_hash = hash_content(&current_content);

        // 6. Try three-layer search strategies
        let initial_result =
            strategies::try_all_strategies(&args.old_string, &args.new_string, &current_content);

        // 7. Check if initial attempt succeeded
        if check_success(&initial_result, args.expected_replacements) {
            // Success! Restore CRLF if needed and write file
            let final_content = if original_line_ending == "\r\n" {
                initial_result.new_content.replace('\n', "\r\n")
            } else {
                initial_result.new_content
            };

            fs::write(&file_path, &final_content).map_err(|e| {
                FunctionCallError::RespondToModel(format!(
                    "Failed to write file {}: {e}",
                    args.file_path
                ))
            })?;

            return Ok(ToolOutput::Function {
                content: format!(
                    "Successfully edited {} ({} replacements, strategy: {})",
                    args.file_path, initial_result.occurrences, initial_result.strategy
                ),
                content_items: None,
                success: Some(true),
            });
        }

        // 8. Initial attempt failed - check for concurrent modifications before LLM correction
        let error_msg = format!(
            "Found {} occurrences (expected {})",
            initial_result.occurrences, args.expected_replacements
        );

        // Re-read file to detect concurrent modifications
        let on_disk_content = fs::read_to_string(&file_path).map_err(|e| {
            FunctionCallError::RespondToModel(format!(
                "Failed to re-read file {}: {e}",
                args.file_path
            ))
        })?;

        let on_disk_hash = hash_content(&on_disk_content);

        let (content_for_correction, error_msg_for_correction) = if initial_content_hash
            != on_disk_hash
        {
            // File was modified externally - use latest content
            (
                on_disk_content,
                format!(
                    "File has been modified externally since initial read. Using latest version. Original error: {}",
                    error_msg
                ),
            )
        } else {
            (current_content.clone(), error_msg.clone())
        };

        // 9. Try LLM correction
        let correction_result = correction::attempt_llm_correction(
            &invocation.turn.client,
            &args.instruction,
            &args.old_string,
            &args.new_string,
            &content_for_correction,
            &error_msg_for_correction,
            40, // 40 second timeout
        )
        .await;

        let corrected = match correction_result {
            Ok(c) => c,
            Err(e) => {
                // LLM correction failed
                return Err(FunctionCallError::RespondToModel(format!(
                    "Edit failed: {error_msg_for_correction}. LLM correction also failed: {e}"
                )));
            }
        };

        // 10. Check if LLM says no changes are required
        if corrected.no_changes_required {
            return Ok(ToolOutput::Function {
                content: format!(
                    "No changes required for {}. The file already meets the specified conditions.\n\
                     Explanation: {}",
                    args.file_path, corrected.explanation
                ),
                content_items: None,
                success: Some(true),
            });
        }

        // 11. Retry with corrected parameters on the content we used for correction
        let retry_result = strategies::try_all_strategies(
            &corrected.search,
            &corrected.replace,
            &content_for_correction,
        );

        if check_success(&retry_result, args.expected_replacements) {
            // LLM correction succeeded! Restore CRLF if needed and write file
            let final_content = if original_line_ending == "\r\n" {
                retry_result.new_content.replace('\n', "\r\n")
            } else {
                retry_result.new_content
            };

            fs::write(&file_path, &final_content).map_err(|e| {
                FunctionCallError::RespondToModel(format!(
                    "Failed to write file {}: {e}",
                    args.file_path
                ))
            })?;

            Ok(ToolOutput::Function {
                content: format!(
                    "Successfully edited {} with LLM correction\n\
                     Explanation: {}\n\
                     ({} replacements, strategy: {})",
                    args.file_path,
                    corrected.explanation,
                    retry_result.occurrences,
                    retry_result.strategy
                ),
                content_items: None,
                success: Some(true),
            })
        } else {
            // Even LLM correction didn't help
            Err(FunctionCallError::RespondToModel(format!(
                "Edit failed: {error_msg_for_correction}. \
                 LLM attempted correction but still found {} occurrences.\n\
                 LLM explanation: {}",
                retry_result.occurrences, corrected.explanation
            )))
        }
    }
}

/// Check if replacement result matches expected count
fn check_success(result: &strategies::ReplacementResult, expected: i32) -> bool {
    result.occurrences == expected && result.occurrences > 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_check_success() {
        let result = strategies::ReplacementResult {
            new_content: "test".into(),
            occurrences: 1,
            strategy: "exact",
        };

        assert!(check_success(&result, 1));
        assert!(!check_success(&result, 2));
        assert!(!check_success(&result, 0));
    }

    #[test]
    fn test_default_expected_replacements() {
        assert_eq!(default_expected_replacements(), 1);
    }
}
