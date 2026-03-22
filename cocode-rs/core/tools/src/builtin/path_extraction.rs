//! LLM-based path extraction from shell command output.
//!
//! This module provides an LLM-based implementation of the `PathExtractor` trait
//! from `cocode-shell`, enabling fast model pre-reading of files that commands
//! read or modify.
//!
//! ## Usage
//!
//! ```no_run
//! use cocode_tools::builtin::path_extraction::LlmPathExtractor;
//! use cocode_protocol::model::{ModelRoles, ModelRole, ModelSpec};
//! use cocode_api::LanguageModel;
//! use std::sync::Arc;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // Obtain a model implementing LanguageModel
//! // let model: Arc<dyn LanguageModel> = ...;
//! // let extractor = LlmPathExtractor::new(model);
//! // Use with ShellExecutor::with_path_extractor()
//! # Ok(())
//! # }
//! ```

use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use cocode_api::LanguageModel;
use cocode_api::LanguageModelCallOptions;
use cocode_api::LanguageModelMessage;
use cocode_protocol::model::ModelRole;
use cocode_protocol::model::ModelRoles;
use cocode_shell::path_extractor::BoxFuture;
use cocode_shell::path_extractor::PathExtractionResult;
use cocode_shell::path_extractor::PathExtractor;
use cocode_shell::path_extractor::filter_existing_files;
use cocode_shell::path_extractor::truncate_for_extraction;
use tracing::debug;
use tracing::warn;

/// System prompt for path extraction (matches Claude Code).
const PATH_EXTRACTION_PROMPT: &str = r#"Extract any file paths that this command reads or modifies from the output.
Rules:
- Return only file paths, one per line
- Include both relative and absolute paths
- Do not include directories (only files)
- If no file paths found, return empty response"#;

/// LLM-based path extractor using a fast model.
///
/// This extractor uses an LLM (typically a fast model like Haiku) to analyze
/// command output and extract file paths that the command read or modified.
///
/// The extractor is designed to be used with `ShellExecutor::with_path_extractor()`.
pub struct LlmPathExtractor {
    /// Language model for LLM API calls.
    model: Arc<dyn LanguageModel>,
}

impl LlmPathExtractor {
    /// Create a new LLM path extractor with the given model.
    pub fn new(model: Arc<dyn LanguageModel>) -> Self {
        Self { model }
    }

    /// Create from ModelRoles - uses Fast role (falls back to Main if not configured).
    ///
    /// Returns `None` if no model is configured (both fast and main are None).
    ///
    /// The `resolve_fn` callback resolves a `ModelRole` to a concrete `LanguageModel`
    /// implementation. This avoids a direct dependency on provider wiring.
    pub fn from_model_roles(
        roles: &ModelRoles,
        resolve_fn: impl Fn(&ModelRole) -> Option<Arc<dyn LanguageModel>>,
    ) -> Option<Self> {
        // Try Fast role first, fall back to Main
        let _spec = roles.get(ModelRole::Fast)?;
        let model = resolve_fn(&ModelRole::Fast)?;
        Some(Self::new(model))
    }

    /// Returns the model ID of the underlying model.
    pub fn model_id(&self) -> &str {
        self.model.model_id()
    }

    /// Parse paths from LLM response.
    fn parse_paths(response: &str) -> Vec<PathBuf> {
        response
            .lines()
            .filter_map(|line| {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    return None;
                }
                // Skip lines that look like explanatory text
                if trimmed.starts_with("The ")
                    || trimmed.starts_with("No ")
                    || trimmed.starts_with("Note:")
                    || trimmed.contains("file paths")
                    || trimmed.contains("not found")
                {
                    return None;
                }
                // Must look like a path (starts with / or ./ or has extension)
                if trimmed.starts_with('/')
                    || trimmed.starts_with("./")
                    || trimmed.starts_with("../")
                    || trimmed.contains('.')
                {
                    Some(PathBuf::from(trimmed))
                } else {
                    None
                }
            })
            .collect()
    }
}

impl std::fmt::Debug for LlmPathExtractor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LlmPathExtractor")
            .field("model_id", &self.model.model_id())
            .finish()
    }
}

impl PathExtractor for LlmPathExtractor {
    fn extract_paths<'a>(
        &'a self,
        command: &'a str,
        output: &'a str,
        cwd: &'a Path,
    ) -> BoxFuture<'a, cocode_shell::path_extractor::Result<PathExtractionResult>> {
        Box::pin(async move {
            let start = Instant::now();

            // Truncate output for efficiency
            let truncated_output = truncate_for_extraction(output);

            // Skip extraction for empty or very short output
            if truncated_output.trim().is_empty() {
                debug!("Skipping path extraction: empty output");
                return Ok(PathExtractionResult::empty());
            }

            // Build the prompt
            let user_message =
                format!("Command: {command}\n\nOutput:\n{truncated_output}\n\nExtract file paths:");

            // Create request using vercel-ai-provider types
            let options = LanguageModelCallOptions::new(vec![
                LanguageModelMessage::system(PATH_EXTRACTION_PROMPT),
                LanguageModelMessage::user_text(user_message),
            ]);

            // Generate response
            let result = match self.model.do_generate(options).await {
                Ok(r) => r,
                Err(e) => {
                    warn!("Path extraction API call failed: {e}");
                    return Ok(PathExtractionResult::empty());
                }
            };

            // Parse paths from response text content
            let response_text = result.text_content().unwrap_or_default();
            let raw_paths = Self::parse_paths(&response_text);

            // Filter to existing files
            let existing_paths = filter_existing_files(raw_paths, cwd);

            let extraction_ms = start.elapsed().as_millis() as i64;

            debug!(
                paths_found = existing_paths.len(),
                extraction_ms, "Extracted paths from command output"
            );

            Ok(PathExtractionResult::new(existing_paths, extraction_ms))
        })
    }

    fn is_enabled(&self) -> bool {
        // Always enabled when configured
        true
    }
}

#[cfg(test)]
#[path = "path_extraction.test.rs"]
mod tests;
