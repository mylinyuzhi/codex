//! Recent-activity collapse + summary formatter.
//!
//! TS source:
//! - `utils/collapseReadSearch.ts:1074-1109` (`summarizeRecentActivities`)
//! - `utils/collapseReadSearch.ts:961-1066` (`getSearchReadSummaryText`)
//! - Per-tool `isSearchOrReadCommand` in `tools/{GlobTool,GrepTool,FileReadTool}`.
//!
//! Algorithm: walk the activity list from the tail and count
//! consecutive search/read entries. When two or more such entries
//! trail the list, collapse them into "Searching for N patterns,
//! reading M files…". Otherwise walk backward for the most recent
//! `summary` (TS `activityDescription` analog), falling back to the
//! last entry's `tool_name` when no summary is present.
//!
//! TS-DIVERGE: classify() only recognises the closed set of
//! canonical tool names (`Read`, `NotebookRead`, `Glob`, `Grep`). TS
//! also classifies bash subcommands (`find`, `rg`, `ls`) and MCP
//! tools via per-server `classifyMcpToolForCollapse`. Neither analog
//! exists in coco-rs yet; non-canonical tools always return `Other`
//! and break the trailing run.

use coco_types::TaskActivity;

/// Category mirrors TS `isSearchOrReadCommand` return shape but
/// flattened — there's no overlap (a tool is search XOR read XOR
/// other), so an enum is more honest than two booleans.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ToolCategory {
    Search,
    Read,
    Other,
}

/// Map a canonical tool name to its collapse category.
///
/// Source list: `tools/GlobTool/GlobTool.ts` (`isSearch: true`),
/// `tools/GrepTool/GrepTool.ts` (`isSearch: true`),
/// `tools/FileReadTool/FileReadTool.ts` (`isRead: true`).
///
/// Notebook read mirrors `NotebookRead` parity even though the TS
/// source has no `NotebookReadTool.isSearchOrReadCommand` override —
/// it's the obvious analogue and the alternative is showing notebook
/// reads as opaque blockers in the summary.
pub(crate) fn classify(tool_name: &str) -> ToolCategory {
    match tool_name {
        "Read" | "NotebookRead" => ToolCategory::Read,
        "Glob" | "Grep" => ToolCategory::Search,
        _ => ToolCategory::Other,
    }
}

/// Port of TS `summarizeRecentActivities` (`collapseReadSearch.ts:1074-1109`).
///
/// Returns `None` when the list is empty. Returns the collapsed
/// summary when ≥2 trailing entries are search/read. Otherwise walks
/// backward for the most recent activity with a populated `summary`
/// (TS `activityDescription` analog); falls back to the last entry's
/// `tool_name` only when no activity carries a summary.
pub(crate) fn summarize_trailing(activities: &[TaskActivity]) -> Option<String> {
    if activities.is_empty() {
        return None;
    }
    let mut search_count: usize = 0;
    let mut read_count: usize = 0;
    for activity in activities.iter().rev() {
        match classify(&activity.tool_name) {
            ToolCategory::Search => search_count += 1,
            ToolCategory::Read => read_count += 1,
            ToolCategory::Other => break,
        }
    }
    if search_count + read_count >= 2 {
        return Some(format_active_summary(search_count, read_count));
    }
    activities
        .iter()
        .rev()
        .find_map(|a| a.summary.clone())
        .or_else(|| activities.last().map(|a| a.tool_name.clone()))
}

/// Port of TS `getSearchReadSummaryText` (`collapseReadSearch.ts:961-1066`),
/// `isActive = true` branch only — coco-rs only renders this while a
/// turn is running, never post-hoc. Memory operations, REPL counts,
/// and list counts are TS-only concepts (no coco-rs analog yet).
pub(crate) fn format_active_summary(search_count: usize, read_count: usize) -> String {
    let mut parts: Vec<String> = Vec::with_capacity(2);
    if search_count > 0 {
        let unit = if search_count == 1 {
            "pattern"
        } else {
            "patterns"
        };
        let verb = if parts.is_empty() {
            "Searching for"
        } else {
            "searching for"
        };
        parts.push(format!("{verb} {search_count} {unit}"));
    }
    if read_count > 0 {
        let unit = if read_count == 1 { "file" } else { "files" };
        let verb = if parts.is_empty() {
            "Reading"
        } else {
            "reading"
        };
        parts.push(format!("{verb} {read_count} {unit}"));
    }
    let text = parts.join(", ");
    format!("{text}…")
}

#[cfg(test)]
#[path = "activity_summary.test.rs"]
mod tests;
