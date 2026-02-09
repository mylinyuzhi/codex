//! LS tool for directory listing with tree-like view.

use super::prompts;
use crate::context::ToolContext;
use crate::error::Result;
use crate::tool::Tool;
use async_trait::async_trait;
use cocode_file_ignore::IgnoreConfig;
use cocode_file_ignore::IgnoreService;
use cocode_protocol::ApprovalRequest;
use cocode_protocol::ConcurrencySafety;
use cocode_protocol::PermissionResult;
use cocode_protocol::ToolOutput;
use serde_json::Value;
use std::cmp::Ordering;
use std::path::Path;

const INDENTATION_SPACES: usize = 2;
/// Maximum entries to collect before stopping the walker.
/// Prevents excessive memory/CPU usage on large repositories.
const MAX_COLLECT: usize = 2000;

/// Tool for listing directory contents with tree-style output.
///
/// This is a safe, read-only tool that can run concurrently with other tools.
pub struct LsTool {
    default_limit: i32,
    default_depth: i32,
}

impl LsTool {
    /// Create a new LS tool with default settings.
    pub fn new() -> Self {
        Self {
            default_limit: 25,
            default_depth: 1,
        }
    }
}

impl Default for LsTool {
    fn default() -> Self {
        Self::new()
    }
}

/// Directory entry with metadata for sorting and display.
#[derive(Clone)]
struct DirEntry {
    /// Relative path used as sort key (e.g. "src/main.rs").
    name: String,
    /// Filename only (e.g. "main.rs").
    display_name: String,
    /// 0-indexed depth for indentation.
    depth: usize,
    /// Entry kind.
    kind: DirEntryKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DirEntryKind {
    Directory,
    File,
    Symlink,
    Other,
}

/// Collect directory entries using IgnoreService.
///
/// Returns `(entries, truncated)`. When the number of collected entries reaches
/// `MAX_COLLECT`, the walker is stopped early and `truncated` is set to `true`.
fn collect_entries(
    root: &Path,
    max_depth: usize,
    ignore_service: &IgnoreService,
) -> (Vec<DirEntry>, bool) {
    let mut walker = ignore_service.create_walk_builder(root);
    walker.max_depth(Some(max_depth));

    let mut entries = Vec::new();

    for entry_result in walker.build() {
        let entry = match entry_result {
            Ok(e) => e,
            Err(_) => continue,
        };

        // Skip the root directory itself
        if entry.path() == root {
            continue;
        }

        let rel_path = match entry.path().strip_prefix(root) {
            Ok(p) => p,
            Err(_) => continue,
        };

        let entry_depth = rel_path.components().count();

        let file_type = match entry.file_type() {
            Some(ft) => ft,
            None => continue,
        };

        let kind = if file_type.is_symlink() {
            DirEntryKind::Symlink
        } else if file_type.is_dir() {
            DirEntryKind::Directory
        } else if file_type.is_file() {
            DirEntryKind::File
        } else {
            DirEntryKind::Other
        };

        let display_name = entry
            .path()
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();

        let sort_key = rel_path.to_string_lossy().replace('\\', "/");

        entries.push(DirEntry {
            name: sort_key,
            display_name,
            depth: entry_depth - 1, // 0-indexed
            kind,
        });

        if entries.len() >= MAX_COLLECT {
            return (entries, true);
        }
    }

    (entries, false)
}

/// Sort entries: directories first at each level, then alphabetical.
fn sort_entries(entries: &mut [DirEntry]) {
    entries.sort_by(|a, b| {
        let a_parts: Vec<&str> = a.name.split('/').collect();
        let b_parts: Vec<&str> = b.name.split('/').collect();

        let min_len = a_parts.len().min(b_parts.len());
        for i in 0..min_len {
            let a_is_last = i == a_parts.len() - 1;
            let b_is_last = i == b_parts.len() - 1;

            if a_parts[i] != b_parts[i] {
                let a_is_dir_at_level = !a_is_last || a.kind == DirEntryKind::Directory;
                let b_is_dir_at_level = !b_is_last || b.kind == DirEntryKind::Directory;

                if a_is_dir_at_level && !b_is_dir_at_level {
                    return Ordering::Less;
                }
                if !a_is_dir_at_level && b_is_dir_at_level {
                    return Ordering::Greater;
                }

                return a_parts[i].cmp(b_parts[i]);
            }
        }

        a_parts.len().cmp(&b_parts.len())
    });
}

/// Format a single entry line with indentation and type suffix.
fn format_entry_line(entry: &DirEntry) -> String {
    let indent = " ".repeat(entry.depth * INDENTATION_SPACES);
    let mut name = entry.display_name.clone();
    match entry.kind {
        DirEntryKind::Directory => name.push('/'),
        DirEntryKind::Symlink => name.push('@'),
        DirEntryKind::Other => name.push('?'),
        DirEntryKind::File => {}
    }
    format!("{indent}{name}")
}

#[async_trait]
impl Tool for LsTool {
    fn name(&self) -> &str {
        "LS"
    }

    fn description(&self) -> &str {
        prompts::LS_DESCRIPTION
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Absolute path to the directory to list"
                },
                "depth": {
                    "type": "integer",
                    "description": "Maximum traversal depth (default: 1, immediate children only)"
                },
                "offset": {
                    "type": "integer",
                    "description": "1-indexed start entry for pagination (default: 1)"
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum number of entries to return (default: 25)"
                }
            },
            "required": ["path"]
        })
    }

    fn concurrency_safety(&self) -> ConcurrencySafety {
        ConcurrencySafety::Safe
    }

    fn feature_gate(&self) -> Option<cocode_protocol::Feature> {
        Some(cocode_protocol::Feature::Ls)
    }

    async fn check_permission(&self, input: &Value, ctx: &ToolContext) -> PermissionResult {
        let list_path = input
            .get("path")
            .and_then(|v| v.as_str())
            .map(|p| ctx.resolve_path(p))
            .unwrap_or_else(|| ctx.cwd.clone());

        if crate::sensitive_files::is_sensitive_directory(&list_path) {
            return PermissionResult::NeedsApproval {
                request: ApprovalRequest {
                    request_id: format!("ls-sensitive-{}", list_path.display()),
                    tool_name: self.name().to_string(),
                    description: format!("Listing sensitive directory: {}", list_path.display()),
                    risks: vec![],
                    allow_remember: true,
                    proposed_prefix_pattern: None,
                },
            };
        }

        if crate::sensitive_files::is_outside_cwd(&list_path, &ctx.cwd) {
            return PermissionResult::NeedsApproval {
                request: ApprovalRequest {
                    request_id: format!("ls-outside-cwd-{}", list_path.display()),
                    tool_name: self.name().to_string(),
                    description: format!(
                        "Listing directory outside working directory: {}",
                        list_path.display()
                    ),
                    risks: vec![],
                    allow_remember: true,
                    proposed_prefix_pattern: None,
                },
            };
        }

        PermissionResult::Allowed
    }

    async fn execute(&self, input: Value, ctx: &mut ToolContext) -> Result<ToolOutput> {
        let path_str = input["path"].as_str().ok_or_else(|| {
            crate::error::tool_error::InvalidInputSnafu {
                message: "path must be a string",
            }
            .build()
        })?;

        let depth = input
            .get("depth")
            .and_then(|v| v.as_i64())
            .map(|v| v as i32)
            .unwrap_or(self.default_depth);

        let offset = input
            .get("offset")
            .and_then(|v| v.as_i64())
            .map(|v| v as i32)
            .unwrap_or(1);

        let limit = input
            .get("limit")
            .and_then(|v| v.as_i64())
            .map(|v| v as i32)
            .unwrap_or(self.default_limit);

        // Validate parameters
        if depth <= 0 {
            return Err(crate::error::tool_error::InvalidInputSnafu {
                message: "depth must be greater than zero",
            }
            .build());
        }
        if offset <= 0 {
            return Err(crate::error::tool_error::InvalidInputSnafu {
                message: "offset must be a 1-indexed entry number (>= 1)",
            }
            .build());
        }
        if limit <= 0 {
            return Err(crate::error::tool_error::InvalidInputSnafu {
                message: "limit must be greater than zero",
            }
            .build());
        }

        let resolved = ctx.resolve_path(path_str);

        if !resolved.exists() {
            return Err(crate::error::tool_error::ExecutionFailedSnafu {
                message: format!("Path does not exist: {}", resolved.display()),
            }
            .build());
        }

        if !resolved.is_dir() {
            return Err(crate::error::tool_error::ExecutionFailedSnafu {
                message: format!("Path is not a directory: {}", resolved.display()),
            }
            .build());
        }

        // Build ignore-aware walker
        let ignore_config = IgnoreConfig::default().with_hidden(true);
        let ignore_service = IgnoreService::new(ignore_config);

        // Collect and sort entries
        let (mut entries, truncated) = collect_entries(&resolved, depth as usize, &ignore_service);

        if ctx.is_cancelled() {
            return Ok(ToolOutput::text("[Cancelled]"));
        }

        sort_entries(&mut entries);

        // Handle empty directory
        if entries.is_empty() {
            return Ok(ToolOutput::text(format!(
                "Absolute path: {}\n[Empty directory]",
                resolved.display()
            )));
        }

        // Apply pagination
        let offset_idx = (offset - 1) as usize;
        if offset_idx >= entries.len() {
            return Err(crate::error::tool_error::InvalidInputSnafu {
                message: "offset exceeds directory entry count",
            }
            .build());
        }

        let remaining = entries.len() - offset_idx;
        let capped_limit = (limit as usize).min(remaining);
        let end_idx = offset_idx + capped_limit;
        let selected = &entries[offset_idx..end_idx];

        // Format output
        let mut output = Vec::with_capacity(selected.len() + 4);
        output.push(format!("Absolute path: {}", resolved.display()));
        output.push(format!(
            "[{} of {} entries shown]",
            selected.len(),
            entries.len()
        ));

        for entry in selected {
            output.push(format_entry_line(entry));
        }

        if end_idx < entries.len() {
            output.push("More entries available, use offset to see more".to_string());
        }

        if truncated {
            output.push(format!(
                "[Results truncated at {} entries — use a more specific path or reduce depth]",
                MAX_COLLECT
            ));
        }

        Ok(ToolOutput::text(output.join("\n")))
    }
}

#[cfg(test)]
#[path = "ls.test.rs"]
mod tests;
