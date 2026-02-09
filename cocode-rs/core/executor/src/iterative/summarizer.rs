//! Iteration summarization and commit message generation.
//!
//! Provides both LLM-based and file-based (fallback) methods for:
//! - Generating iteration summaries
//! - Generating git commit messages
//!
//! # LLM Callback Helpers
//!
//! This module provides helper functions for creating LLM-powered callbacks:
//!
//! - [`create_summarize_fn`] - Creates a summarization callback using hyper-sdk
//! - [`create_commit_msg_fn`] - Creates a commit message callback using hyper-sdk
//!
//! These helpers use the provided model to generate summaries and commit messages.
//! The recommended approach is to use the same model as the main conversation.
//!
//! # Example
//!
//! ```ignore
//! use cocode_executor::iterative::{create_summarize_fn, create_commit_msg_fn};
//! use hyper_sdk::ModelBuilder;
//! use std::sync::Arc;
//!
//! // Create model
//! let model = Arc::new(ModelBuilder::new("claude-3-5-sonnet-20241022")
//!     .api_key("sk-...")
//!     .build()?);
//!
//! // Create callbacks
//! let summarize_fn = create_summarize_fn(model.clone());
//! let commit_msg_fn = create_commit_msg_fn(model);
//!
//! // Use with IterativeExecutor
//! let executor = IterativeExecutor::new(IterationCondition::Count { max: 5 })
//!     .with_summarize_fn(summarize_fn)
//!     .with_commit_msg_fn(commit_msg_fn);
//! ```

use std::collections::HashMap;
use std::path::Path;
use std::pin::Pin;
use std::sync::Arc;

use crate::iterative::context::IterationRecord;

/// Type alias for async summarization callback.
pub type SummarizeFn = Arc<
    dyn Fn(
            i32,         // iteration
            Vec<String>, // changed_files
            String,      // task description
        ) -> Pin<Box<dyn std::future::Future<Output = anyhow::Result<String>> + Send>>
        + Send
        + Sync,
>;

/// Type alias for async commit message generation callback.
pub type CommitMessageFn = Arc<
    dyn Fn(
            i32,         // iteration
            String,      // task
            Vec<String>, // changed_files
            String,      // summary
        ) -> Pin<Box<dyn std::future::Future<Output = anyhow::Result<String>> + Send>>
        + Send
        + Sync,
>;

/// Summarizes a series of iteration records into a human-readable report.
pub struct Summarizer;

impl Summarizer {
    /// Produce a summary string from a set of iteration records.
    ///
    /// The summary includes the total iteration count, aggregate duration, and
    /// the result of each iteration.
    pub fn summarize_iterations(records: &[IterationRecord]) -> String {
        if records.is_empty() {
            return "No iterations executed.".to_string();
        }

        let total_ms: i64 = records.iter().map(|r| r.duration_ms).sum();
        let count = records.len();

        let mut lines = vec![format!(
            "Completed {count} iteration(s) in {total_ms}ms total."
        )];

        for record in records {
            let status = if record.success { "OK" } else { "FAILED" };
            let commit = record.commit_status();
            lines.push(format!(
                "  [{iter}] ({dur}ms) [{status}] {commit}: {summary}",
                iter = record.iteration,
                dur = record.duration_ms,
                summary = if record.summary.is_empty() {
                    &record.result
                } else {
                    &record.summary
                }
            ));
        }

        lines.join("\n")
    }
}

/// Generate a file-based summary for an iteration (fallback when LLM not available).
///
/// Groups files by extension and generates a descriptive summary.
pub fn generate_file_based_summary(
    iteration: i32,
    changed_files: &[String],
    success: bool,
) -> String {
    let status = if success { "succeeded" } else { "failed" };

    if changed_files.is_empty() {
        return format!("Iteration {iteration} {status} with no file changes.");
    }

    // Group files by extension
    let mut by_ext: HashMap<&str, Vec<&str>> = HashMap::new();
    for file in changed_files {
        let ext = Path::new(file)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("other");
        by_ext.entry(ext).or_default().push(file);
    }

    // Build summary
    let file_count = changed_files.len();
    let ext_summary: Vec<String> = by_ext
        .iter()
        .map(|(ext, files)| format!("{} .{ext} file(s)", files.len()))
        .collect();

    format!(
        "Iteration {iteration} {status}. Modified {file_count} file(s): {}.",
        ext_summary.join(", ")
    )
}

/// Generate a fallback commit message when LLM is not available.
pub fn generate_fallback_commit_message(iteration: i32, changed_files: &[String]) -> String {
    let file_count = changed_files.len();
    let files_display = if file_count <= 5 {
        changed_files.join(", ")
    } else {
        format!(
            "{}, ... ({} more)",
            changed_files[..5].join(", "),
            file_count - 5
        )
    };

    format!("[iter-{iteration}] Iteration {iteration} changes\n\nModified files: {files_display}")
}

/// Generate iteration summary with optional LLM callback.
///
/// If summarize_fn is provided and returns Ok, uses LLM-generated summary.
/// Otherwise falls back to file-based summary.
pub async fn generate_summary(
    iteration: i32,
    changed_files: &[String],
    task: &str,
    success: bool,
    summarize_fn: Option<&SummarizeFn>,
) -> String {
    // Try LLM-based summary if callback provided
    if let Some(summarize) = summarize_fn {
        match summarize(iteration, changed_files.to_vec(), task.to_string()).await {
            Ok(summary) if !summary.is_empty() => {
                tracing::debug!(iteration, "Generated LLM summary for iteration");
                return summary;
            }
            Err(e) => {
                tracing::warn!(error = %e, "LLM summary failed, using fallback");
            }
            _ => {}
        }
    }

    // Fallback to file-based summary
    generate_file_based_summary(iteration, changed_files, success)
}

/// Generate commit message with optional LLM callback.
///
/// If commit_msg_fn is provided and returns Ok, uses LLM-generated message.
/// Otherwise falls back to standard format.
pub async fn generate_commit_message(
    iteration: i32,
    task: &str,
    changed_files: &[String],
    summary: &str,
    commit_msg_fn: Option<&CommitMessageFn>,
) -> String {
    // Try LLM-based commit message if callback provided
    if let Some(gen_msg) = commit_msg_fn {
        match gen_msg(
            iteration,
            task.to_string(),
            changed_files.to_vec(),
            summary.to_string(),
        )
        .await
        {
            Ok(msg) if !msg.is_empty() => {
                tracing::debug!(iteration, "Generated LLM commit message");
                return msg;
            }
            Err(e) => {
                tracing::warn!(error = %e, "LLM commit message failed, using fallback");
            }
            _ => {}
        }
    }

    // Fallback to standard format
    generate_fallback_commit_message(iteration, changed_files)
}

/// Create a default summarization callback using hyper-sdk.
///
/// This helper creates a [`SummarizeFn`] that uses the provided model to generate
/// iteration summaries. It's recommended to use the same model as the main conversation.
///
/// # Arguments
///
/// * `model` - A hyper-sdk Model implementation (e.g., from ModelBuilder)
///
/// # Returns
///
/// A [`SummarizeFn`] that can be passed to [`IterativeExecutor::with_summarize_fn`].
///
/// # Example
///
/// ```ignore
/// let model = Arc::new(ModelBuilder::new("claude-3-5-sonnet-20241022")
///     .api_key("sk-...")
///     .build()?);
/// let summarize_fn = create_summarize_fn(model);
/// ```
pub fn create_summarize_fn<M>(model: Arc<M>) -> SummarizeFn
where
    M: hyper_sdk::Model + Send + Sync + 'static,
{
    Arc::new(move |iteration, changed_files, task| {
        let model = model.clone();
        Box::pin(async move {
            let user_prompt = prompts::format_summary_prompt(&task, &changed_files);

            let request = hyper_sdk::GenerateRequest::new(vec![
                hyper_sdk::Message::system(prompts::ITERATION_SUMMARY_SYSTEM),
                hyper_sdk::Message::user(&user_prompt),
            ]);

            let response = model
                .generate(request)
                .await
                .map_err(|e| anyhow::anyhow!("LLM summary generation failed: {e}"))?;

            let text = response.text().trim().to_string();
            if text.is_empty() {
                return Err(anyhow::anyhow!("LLM returned empty summary"));
            }

            tracing::debug!(iteration, len = text.len(), "Generated LLM summary");
            Ok(text)
        })
    })
}

/// Create a default commit message callback using hyper-sdk.
///
/// This helper creates a [`CommitMessageFn`] that uses the provided model to generate
/// git commit messages. It's recommended to use the same model as the main conversation.
///
/// # Arguments
///
/// * `model` - A hyper-sdk Model implementation (e.g., from ModelBuilder)
///
/// # Returns
///
/// A [`CommitMessageFn`] that can be passed to [`IterativeExecutor::with_commit_msg_fn`].
///
/// # Example
///
/// ```ignore
/// let model = Arc::new(ModelBuilder::new("claude-3-5-sonnet-20241022")
///     .api_key("sk-...")
///     .build()?);
/// let commit_msg_fn = create_commit_msg_fn(model);
/// ```
pub fn create_commit_msg_fn<M>(model: Arc<M>) -> CommitMessageFn
where
    M: hyper_sdk::Model + Send + Sync + 'static,
{
    Arc::new(move |iteration, task, changed_files, summary| {
        let model = model.clone();
        Box::pin(async move {
            let user_prompt =
                prompts::format_commit_msg_prompt(iteration, &task, &changed_files, &summary);

            let request = hyper_sdk::GenerateRequest::new(vec![
                hyper_sdk::Message::system(prompts::COMMIT_MSG_SYSTEM),
                hyper_sdk::Message::user(&user_prompt),
            ]);

            let response = model
                .generate(request)
                .await
                .map_err(|e| anyhow::anyhow!("LLM commit message generation failed: {e}"))?;

            let text = response.text().trim().to_string();
            if text.is_empty() {
                return Err(anyhow::anyhow!("LLM returned empty commit message"));
            }

            tracing::debug!(iteration, len = text.len(), "Generated LLM commit message");
            Ok(text)
        })
    })
}

/// LLM prompt templates for summarization.
pub mod prompts {
    /// Iteration summary system prompt.
    pub const ITERATION_SUMMARY_SYSTEM: &str = r#"You are a concise technical summarizer.
Your task is to summarize an AI agent's work in a single iteration.
Be factual and brief. Focus on what was actually done, not what was planned."#;

    /// Iteration summary user prompt template.
    pub const ITERATION_SUMMARY_USER: &str = r#"Summarize this agent iteration in 3-5 sentences:

1. What task was attempted
2. What was accomplished (files created/modified, features implemented)
3. Key decisions made or blockers encountered

Task: {task}
Changed files: {files}

This summary will be passed to the next iteration for context continuity.
Output ONLY the summary text, no explanations or formatting."#;

    /// Commit message system prompt.
    pub const COMMIT_MSG_SYSTEM: &str = r#"You are a git commit message generator.
Generate clear, conventional commit messages following this format:
- First line: [iter-N] Brief description (max 50 chars)
- Blank line
- Body: What was done (2-3 lines max)

Output ONLY the commit message, nothing else."#;

    /// Commit message user prompt template.
    pub const COMMIT_MSG_USER: &str = r#"Generate a git commit message for this iteration.

Iteration: {iteration}
Task (truncated): {task}
Changed files: {files}
Summary: {summary}

Output ONLY the commit message."#;

    /// Format the iteration summary user prompt.
    pub fn format_summary_prompt(task: &str, files: &[String]) -> String {
        let task_truncated = if task.len() > 500 {
            format!("{}...", &task[..500])
        } else {
            task.to_string()
        };

        let files_str = if files.len() <= 20 {
            files.join(", ")
        } else {
            format!(
                "{}, ... ({} more)",
                files[..20].join(", "),
                files.len() - 20
            )
        };

        ITERATION_SUMMARY_USER
            .replace("{task}", &task_truncated)
            .replace("{files}", &files_str)
    }

    /// Format the commit message user prompt.
    pub fn format_commit_msg_prompt(
        iteration: i32,
        task: &str,
        files: &[String],
        summary: &str,
    ) -> String {
        let task_truncated = if task.len() > 200 {
            format!("{}...", &task[..200])
        } else {
            task.to_string()
        };

        let files_str = if files.len() <= 10 {
            files.join(", ")
        } else {
            format!(
                "{}, ... ({} more)",
                files[..10].join(", "),
                files.len() - 10
            )
        };

        COMMIT_MSG_USER
            .replace("{iteration}", &iteration.to_string())
            .replace("{task}", &task_truncated)
            .replace("{files}", &files_str)
            .replace("{summary}", summary)
    }
}

#[cfg(test)]
#[path = "summarizer.test.rs"]
mod tests;
