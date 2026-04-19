//! Glob tool — fast file-pattern search, modeled on the TS Claude Code
//! `GlobTool` which shells out to `rg --files --glob <pattern> --sort=modified`.
//!
//! # Safety & concurrency model
//!
//! - `is_read_only(_) = true` — no filesystem modifications.
//! - `is_concurrency_safe(_) = true` — no shared mutable state; two calls
//!   may execute in parallel via the `StreamingToolExecutor`.
//! - `is_destructive` / `interrupt_behavior` / `requires_user_interaction`
//!   all use the trait defaults, matching the TS tool which also does not
//!   override these.
//!
//! # Execution pipeline
//!
//! The walker is constructed from [`IgnoreService`] with `.gitignore`
//! disabled (to match TS `--no-ignore`) and hidden files enabled (to match
//! TS `--hidden`). File discovery, compiled-glob matching, and mtime
//! collection all run inside [`tokio::task::spawn_blocking`], wrapped in
//! [`tokio::time::timeout`] for a bounded 20-second budget (overridable via
//! the `CLAUDE_CODE_GLOB_TIMEOUT_SECONDS` env var).
//!
//! # Sort order
//!
//! TS passes `--sort=modified` to ripgrep, which sorts **ascending** by
//! modification time (oldest first). This is verified by
//! `rg --files --sort=modified`. We match that ordering deliberately so output
//! is byte-compatible with Claude Code — see [`run_glob_search`].
//!
//! # Cancellation & worktree isolation
//!
//! `ctx.cancel` is checked per directory entry during the walk, and
//! `ctx.cwd_override` is honored when set (for worktree-isolated subagents).

use coco_file_ignore::IgnoreConfig;
use coco_file_ignore::IgnoreService;
use coco_tool::DescriptionOptions;
use coco_tool::SearchReadInfo;
use coco_tool::Tool;
use coco_tool::ToolError;
use coco_tool::ToolUseContext;
use coco_tool::ValidationResult;
use coco_types::PermissionDecision;
use coco_types::ToolId;
use coco_types::ToolInputSchema;
use coco_types::ToolName;
use coco_types::ToolResult;
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;
use std::time::SystemTime;
use tokio_util::sync::CancellationToken;

/// Default max results when glob_limits.max_results is None (TS: 100).
const DEFAULT_MAX_RESULTS: usize = 100;

/// Timeout seconds for glob operations.
const DEFAULT_TIMEOUT_SECS: u64 = 20;

/// Tool description shown to the model. Byte-for-byte copy of TS Claude Code
/// `tools/GlobTool/prompt.ts::DESCRIPTION`.
const GLOB_DESCRIPTION: &str = "\
- Fast file pattern matching tool that works with any codebase size
- Supports glob patterns like \"**/*.js\" or \"src/**/*.ts\"
- Returns matching file paths sorted by modification time
- Use this tool when you need to find files by name patterns
- When you are doing an open ended search that may require multiple rounds of globbing and grepping, use the Agent tool instead";

/// Glob tool — fast file pattern matching.
/// Returns matching file paths sorted by modification time (most recent first).
pub struct GlobTool;

#[async_trait::async_trait]
impl Tool for GlobTool {
    fn id(&self) -> ToolId {
        ToolId::Builtin(ToolName::Glob)
    }

    fn name(&self) -> &str {
        ToolName::Glob.as_str()
    }

    fn description(&self, _input: &Value, _options: &DescriptionOptions) -> String {
        GLOB_DESCRIPTION.into()
    }

    fn input_schema(&self) -> ToolInputSchema {
        // Descriptions byte-for-byte match TS `GlobTool.ts` inputSchema.
        let mut props = HashMap::new();
        props.insert(
            "pattern".into(),
            serde_json::json!({
                "type": "string",
                "description": "The glob pattern to match files against"
            }),
        );
        props.insert(
            "path".into(),
            serde_json::json!({
                "type": "string",
                "description": "The directory to search in. If not specified, the current working directory will be used. IMPORTANT: Omit this field to use the default directory. DO NOT enter \"undefined\" or \"null\" - simply omit it for the default behavior. Must be a valid directory path if provided."
            }),
        );
        ToolInputSchema { properties: props }
    }

    /// Glob never modifies state (TS: `isReadOnly() = true`).
    fn is_read_only(&self, _input: &Value) -> bool {
        true
    }

    /// Safe to run in parallel with other concurrency-safe tools. Batches
    /// with Grep/Read/etc. via the `StreamingToolExecutor`.
    /// TS: `isConcurrencySafe() = true`.
    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        true
    }

    /// Result persistence threshold — matches TS `maxResultSizeChars: 100_000`.
    fn max_result_size_chars(&self) -> i32 {
        100_000
    }

    fn get_activity_description(&self, input: &Value) -> Option<String> {
        let pattern = input.get("pattern").and_then(|v| v.as_str())?;
        Some(format!("Searching for {pattern}"))
    }

    fn is_search_or_read_command(&self, _input: &Value) -> Option<SearchReadInfo> {
        Some(SearchReadInfo {
            is_search: true,
            ..SearchReadInfo::default()
        })
    }

    /// R6-T20: block globbing under a path that's in the ignore list.
    /// Individual results matching an ignore glob are also filtered
    /// inside `run_glob_search`.
    async fn check_permissions(&self, input: &Value, _ctx: &ToolUseContext) -> PermissionDecision {
        let Some(path) = input.get("path").and_then(|v| v.as_str()) else {
            return PermissionDecision::Allow {
                updated_input: None,
                feedback: None,
            };
        };
        crate::tools::read_permissions::check_read_permission(Path::new(path))
    }

    fn validate_input(&self, input: &Value, _ctx: &ToolUseContext) -> ValidationResult {
        if input.get("pattern").and_then(|v| v.as_str()).is_none() {
            return ValidationResult::invalid("missing required field: pattern");
        }
        ValidationResult::Valid
    }

    async fn execute(
        &self,
        input: Value,
        ctx: &ToolUseContext,
    ) -> Result<ToolResult<Value>, ToolError> {
        let pattern = input
            .get("pattern")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidInput {
                message: "missing pattern".into(),
                error_code: None,
            })?;

        // Resolve the working directory. Worktree-isolated agents set
        // `ctx.cwd_override`; otherwise we fall back to the process CWD.
        // Relative `path` arguments are resolved against this base.
        let cwd = ctx
            .cwd_override
            .clone()
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| PathBuf::from("/"));

        let search_path = match input.get("path").and_then(|v| v.as_str()) {
            Some(p) => {
                let path = Path::new(p);
                if path.is_absolute() {
                    path.to_path_buf()
                } else {
                    cwd.join(p)
                }
            }
            None => cwd.clone(),
        };

        if !search_path.exists() {
            return Err(ToolError::ExecutionFailed {
                message: format!("search path does not exist: {}", search_path.display()),
                source: None,
            });
        }

        // Read limit from ctx.glob_limits (TS: globLimits?.maxResults ?? 100)
        let max_results = ctx
            .glob_limits
            .max_results
            .map(|n| n as usize)
            .unwrap_or(DEFAULT_MAX_RESULTS);

        let timeout_secs = std::env::var("CLAUDE_CODE_GLOB_TIMEOUT_SECONDS")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .filter(|&v| v > 0)
            .unwrap_or(DEFAULT_TIMEOUT_SECS);

        // Move owned values into the blocking closure — no redundant clones.
        let cancel = ctx.cancel.clone();
        let pattern_owned = pattern.to_string();
        let search_future = tokio::task::spawn_blocking(move || {
            run_glob_search(&pattern_owned, &search_path, &cwd, max_results, &cancel)
        });

        let (paths, truncated) =
            tokio::time::timeout(Duration::from_secs(timeout_secs), search_future)
                .await
                .map_err(|_| ToolError::Timeout {
                    timeout_ms: (timeout_secs * 1000) as i64,
                })?
                .map_err(|e| ToolError::ExecutionFailed {
                    message: format!("glob search task failed: {e}"),
                    source: None,
                })?
                .map_err(|e| ToolError::ExecutionFailed {
                    message: e,
                    source: None,
                })?;

        let output = if paths.is_empty() {
            "No files found".to_string()
        } else {
            let mut text = paths.join("\n");
            if truncated {
                text.push_str(
                    "\n(Results are truncated. Consider using a more specific path or pattern.)",
                );
            }
            text
        };

        Ok(ToolResult {
            data: serde_json::json!(output),
            new_messages: vec![],
            app_state_patch: None,
        })
    }
}

// ---------------------------------------------------------------------------
// Core glob search (synchronous, runs inside spawn_blocking)
// ---------------------------------------------------------------------------

fn run_glob_search(
    pattern: &str,
    search_path: &Path,
    base_dir: &Path,
    max_results: usize,
    cancel: &CancellationToken,
) -> Result<(Vec<String>, bool), String> {
    // Build the glob matcher
    let glob = globset::GlobBuilder::new(pattern)
        .literal_separator(false)
        .build()
        .map_err(|e| format!("invalid glob pattern: {e}"))?
        .compile_matcher();

    // TS: --no-ignore (don't respect .gitignore) + --hidden (show hidden files)
    let ignore_config = IgnoreConfig::default()
        .with_hidden(true)
        .with_gitignore(false);
    let ignore_service = IgnoreService::new(ignore_config);
    let walker_builder = ignore_service.create_walk_builder(search_path);

    let mut matches: Vec<(PathBuf, SystemTime)> = Vec::new();

    for entry in walker_builder.build().flatten() {
        if cancel.is_cancelled() {
            break;
        }

        let path = entry.path();
        if !path.is_file() {
            continue;
        }

        // R6-T20: hide files that match the file-read ignore patterns
        // from the result list. Matches TS `GlobTool` which pipes its
        // result through `checkReadPermissionForTool` before emitting.
        if crate::tools::read_permissions::is_read_ignored(path) {
            continue;
        }

        // Match against the path relative to the search directory, not cwd.
        let rel_path = path.strip_prefix(search_path).unwrap_or(path);
        if glob.is_match(rel_path) {
            let mtime = path
                .metadata()
                .and_then(|m| m.modified())
                .unwrap_or(SystemTime::UNIX_EPOCH);
            matches.push((path.to_path_buf(), mtime));
        }
    }

    // TS-parity: Claude Code passes `--sort=modified` to ripgrep, which sorts
    // ascending by modification time (oldest first). Verified via
    // `rg --files --sort=modified`. We match that ordering deliberately so
    // output matches Claude Code byte-for-byte — do not flip to newest-first
    // without a matching change in the TS tool.
    matches.sort_by(|a, b| a.1.cmp(&b.1));

    // Check truncation before limiting
    let truncated = matches.len() > max_results;
    if truncated {
        matches.truncate(max_results);
    }

    // Convert to relative paths
    let paths: Vec<String> = matches
        .into_iter()
        .map(|(p, _)| {
            p.strip_prefix(base_dir)
                .unwrap_or(&p)
                .to_string_lossy()
                .to_string()
        })
        .collect();

    Ok((paths, truncated))
}

#[cfg(test)]
#[path = "glob.test.rs"]
mod tests;
