//! Subagent tool definitions and execution.
//!
//! Provides tool specs and execution for tools available to subagents.
//! This is a simplified version of the main tool system, focused on
//! read-only tools that don't require full ToolInvocation context.

use crate::client_common::tools::ResponsesApiTool;
use crate::client_common::tools::ToolSpec;
use crate::tools::spec::JsonSchema;
use codex_file_ignore::IgnoreConfig;
use codex_file_ignore::IgnoreService;
use glob::Pattern;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;
use std::time::SystemTime;

/// Tool names supported by subagents.
pub const SUBAGENT_TOOL_GLOB: &str = "Glob";
pub const SUBAGENT_TOOL_GREP: &str = "Grep";
pub const SUBAGENT_TOOL_READ: &str = "Read";

/// Get all available tool specs for subagent filtering.
pub fn get_all_subagent_tool_specs() -> Vec<ToolSpec> {
    vec![
        create_glob_tool_spec(),
        create_grep_tool_spec(),
        create_read_tool_spec(),
    ]
}

/// Get tool spec by name.
pub fn get_tool_spec_by_name(name: &str) -> Option<ToolSpec> {
    match name {
        SUBAGENT_TOOL_GLOB | "glob_files" => Some(create_glob_tool_spec()),
        SUBAGENT_TOOL_GREP | "grep_files" => Some(create_grep_tool_spec()),
        SUBAGENT_TOOL_READ | "read_file" => Some(create_read_tool_spec()),
        _ => None,
    }
}

/// Create Glob tool spec (file pattern matching).
fn create_glob_tool_spec() -> ToolSpec {
    let mut properties = BTreeMap::new();

    properties.insert(
        "pattern".to_string(),
        JsonSchema::String {
            description: Some(
                "Glob pattern to match files (e.g., \"**/*.rs\", \"src/**/*.ts\")".to_string(),
            ),
        },
    );

    properties.insert(
        "path".to_string(),
        JsonSchema::String {
            description: Some(
                "Directory to search in. Defaults to current working directory.".to_string(),
            ),
        },
    );

    ToolSpec::Function(ResponsesApiTool {
        name: SUBAGENT_TOOL_GLOB.to_string(),
        description: "Find files by glob pattern. Returns matching file paths.".to_string(),
        strict: false,
        parameters: JsonSchema::Object {
            properties,
            required: Some(vec!["pattern".to_string()]),
            additional_properties: Some(false.into()),
        },
    })
}

/// Create Grep tool spec (content search).
fn create_grep_tool_spec() -> ToolSpec {
    let mut properties = BTreeMap::new();

    properties.insert(
        "pattern".to_string(),
        JsonSchema::String {
            description: Some("Regex pattern to search for in file contents.".to_string()),
        },
    );

    properties.insert(
        "path".to_string(),
        JsonSchema::String {
            description: Some("Directory or file to search in.".to_string()),
        },
    );

    properties.insert(
        "glob".to_string(),
        JsonSchema::String {
            description: Some("Glob pattern to filter files (e.g., \"*.rs\").".to_string()),
        },
    );

    ToolSpec::Function(ResponsesApiTool {
        name: SUBAGENT_TOOL_GREP.to_string(),
        description: "Search file contents using regex. Returns matching lines with file paths."
            .to_string(),
        strict: false,
        parameters: JsonSchema::Object {
            properties,
            required: Some(vec!["pattern".to_string()]),
            additional_properties: Some(false.into()),
        },
    })
}

/// Create Read tool spec (file reading).
fn create_read_tool_spec() -> ToolSpec {
    let mut properties = BTreeMap::new();

    properties.insert(
        "file_path".to_string(),
        JsonSchema::String {
            description: Some("Absolute path to the file to read.".to_string()),
        },
    );

    properties.insert(
        "offset".to_string(),
        JsonSchema::Number {
            description: Some("Line number to start reading from.".to_string()),
        },
    );

    properties.insert(
        "limit".to_string(),
        JsonSchema::Number {
            description: Some("Number of lines to read.".to_string()),
        },
    );

    ToolSpec::Function(ResponsesApiTool {
        name: SUBAGENT_TOOL_READ.to_string(),
        description: "Read a file from the filesystem.".to_string(),
        strict: false,
        parameters: JsonSchema::Object {
            properties,
            required: Some(vec!["file_path".to_string()]),
            additional_properties: Some(false.into()),
        },
    })
}

/// Execute a tool call in subagent context.
///
/// Returns (success, output) tuple.
pub fn execute_tool(tool_name: &str, arguments: &str, cwd: &Path) -> (bool, String) {
    match tool_name {
        SUBAGENT_TOOL_GLOB | "glob_files" => execute_glob(arguments, cwd),
        SUBAGENT_TOOL_GREP | "grep_files" => execute_grep(arguments, cwd),
        SUBAGENT_TOOL_READ | "read_file" => execute_read(arguments, cwd),
        _ => (false, format!("Unknown tool: {tool_name}")),
    }
}

// ============================================================================
// Tool Execution Implementations
// ============================================================================

#[derive(Debug, Deserialize)]
struct GlobArgs {
    pattern: String,
    path: Option<String>,
}

fn execute_glob(arguments: &str, cwd: &Path) -> (bool, String) {
    let args: GlobArgs = match serde_json::from_str(arguments) {
        Ok(a) => a,
        Err(e) => return (false, format!("Invalid arguments: {e}")),
    };

    let search_path = resolve_path(&args.path, cwd);

    if !search_path.exists() {
        return (
            false,
            format!("Path does not exist: {}", search_path.display()),
        );
    }

    let ignore_config = IgnoreConfig {
        respect_gitignore: true,
        respect_ignore: true,
        include_hidden: true,
        follow_links: false,
        custom_excludes: Vec::new(),
    };
    let ignore_service = IgnoreService::new(ignore_config);
    let walker = ignore_service.create_walk_builder(&search_path);

    let glob_pattern = match Pattern::new(&args.pattern) {
        Ok(p) => p,
        Err(e) => return (false, format!("Invalid glob pattern: {e}")),
    };

    let mut entries: Vec<(PathBuf, Option<SystemTime>)> = Vec::new();
    const LIMIT: usize = 200;

    for entry_result in walker.build() {
        let entry = match entry_result {
            Ok(e) => e,
            Err(_) => continue,
        };

        if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
            continue;
        }

        let rel_path = match entry.path().strip_prefix(&search_path) {
            Ok(p) => p,
            Err(_) => continue,
        };

        let path_str = rel_path.to_string_lossy();

        if glob_pattern.matches_with(
            &path_str,
            glob::MatchOptions {
                case_sensitive: false,
                require_literal_separator: false,
                require_literal_leading_dot: false,
            },
        ) {
            let mtime = entry.metadata().ok().and_then(|m| m.modified().ok());
            entries.push((entry.path().to_path_buf(), mtime));
        }

        if entries.len() >= LIMIT {
            break;
        }
    }

    // Sort by mtime (recent first)
    sort_by_mtime(&mut entries);

    let results: Vec<String> = entries
        .iter()
        .map(|(p, _)| {
            p.strip_prefix(&search_path)
                .map(|r| r.display().to_string())
                .unwrap_or_else(|_| p.display().to_string())
        })
        .collect();

    if results.is_empty() {
        (
            true,
            format!("No files found matching pattern \"{}\"", args.pattern),
        )
    } else {
        let output = format!(
            "Found {} file(s) matching \"{}\":\n{}",
            results.len(),
            args.pattern,
            results.join("\n")
        );
        (true, output)
    }
}

#[derive(Debug, Deserialize)]
struct GrepArgs {
    pattern: String,
    path: Option<String>,
    glob: Option<String>,
}

fn execute_grep(arguments: &str, cwd: &Path) -> (bool, String) {
    let args: GrepArgs = match serde_json::from_str(arguments) {
        Ok(a) => a,
        Err(e) => return (false, format!("Invalid arguments: {e}")),
    };

    let search_path = resolve_path(&args.path, cwd);

    if !search_path.exists() {
        return (
            false,
            format!("Path does not exist: {}", search_path.display()),
        );
    }

    // Build ripgrep command
    let mut cmd = std::process::Command::new("rg");
    cmd.arg("--json")
        .arg("--max-count=100")
        .arg("--no-heading")
        .arg(&args.pattern);

    if let Some(glob) = &args.glob {
        cmd.arg("--glob").arg(glob);
    }

    cmd.arg(&search_path);

    match cmd.output() {
        Ok(output) => {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let matches = parse_rg_json_output(&stdout);
                if matches.is_empty() {
                    (
                        true,
                        format!("No matches found for pattern \"{}\"", args.pattern),
                    )
                } else {
                    (
                        true,
                        format!("Found {} match(es):\n{}", matches.len(), matches.join("\n")),
                    )
                }
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr);
                if stderr.is_empty() {
                    (
                        true,
                        format!("No matches found for pattern \"{}\"", args.pattern),
                    )
                } else {
                    (false, format!("Grep error: {stderr}"))
                }
            }
        }
        Err(e) => (false, format!("Failed to execute ripgrep: {e}")),
    }
}

fn parse_rg_json_output(output: &str) -> Vec<String> {
    let mut results = Vec::new();

    for line in output.lines() {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(line) {
            if json.get("type").and_then(|t| t.as_str()) == Some("match") {
                if let Some(data) = json.get("data") {
                    let path = data
                        .get("path")
                        .and_then(|p| p.get("text"))
                        .and_then(|t| t.as_str())
                        .unwrap_or("");
                    let line_num = data
                        .get("line_number")
                        .and_then(|n| n.as_i64())
                        .unwrap_or(0);
                    let text = data
                        .get("lines")
                        .and_then(|l| l.get("text"))
                        .and_then(|t| t.as_str())
                        .unwrap_or("");

                    results.push(format!("{}:{}: {}", path, line_num, text.trim()));
                }
            }
        }
    }

    results
}

#[derive(Debug, Deserialize)]
struct ReadArgs {
    file_path: String,
    offset: Option<i64>,
    limit: Option<i64>,
}

fn execute_read(arguments: &str, cwd: &Path) -> (bool, String) {
    let args: ReadArgs = match serde_json::from_str(arguments) {
        Ok(a) => a,
        Err(e) => return (false, format!("Invalid arguments: {e}")),
    };

    let file_path = if Path::new(&args.file_path).is_absolute() {
        PathBuf::from(&args.file_path)
    } else {
        cwd.join(&args.file_path)
    };

    if !file_path.exists() {
        return (
            false,
            format!("File does not exist: {}", file_path.display()),
        );
    }

    match std::fs::read_to_string(&file_path) {
        Ok(content) => {
            let lines: Vec<&str> = content.lines().collect();
            let offset = args.offset.unwrap_or(0) as usize;
            let limit = args.limit.unwrap_or(2000) as usize;

            let selected: Vec<String> = lines
                .iter()
                .skip(offset)
                .take(limit)
                .enumerate()
                .map(|(i, line)| format!("{:>6}\t{}", offset + i + 1, line))
                .collect();

            (true, selected.join("\n"))
        }
        Err(e) => (false, format!("Failed to read file: {e}")),
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

fn resolve_path(path: &Option<String>, cwd: &Path) -> PathBuf {
    match path {
        Some(p) if !p.is_empty() => {
            let p = Path::new(p);
            if p.is_absolute() {
                p.to_path_buf()
            } else {
                cwd.join(p)
            }
        }
        _ => cwd.to_path_buf(),
    }
}

fn sort_by_mtime(entries: &mut [(PathBuf, Option<SystemTime>)]) {
    let now = SystemTime::now();
    let threshold = Duration::from_secs(24 * 60 * 60);

    entries.sort_by(|a, b| {
        let is_recent_a = is_recent(&a.1, now, threshold);
        let is_recent_b = is_recent(&b.1, now, threshold);

        match (is_recent_a, is_recent_b) {
            (true, true) => compare_mtime_desc(&a.1, &b.1),
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            (false, false) => a.0.cmp(&b.0),
        }
    });
}

fn is_recent(mtime: &Option<SystemTime>, now: SystemTime, threshold: Duration) -> bool {
    mtime
        .and_then(|t| now.duration_since(t).ok())
        .map(|d| d < threshold)
        .unwrap_or(false)
}

fn compare_mtime_desc(a: &Option<SystemTime>, b: &Option<SystemTime>) -> std::cmp::Ordering {
    match (a, b) {
        (Some(ta), Some(tb)) => tb.cmp(ta),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_get_all_tool_specs() {
        let specs = get_all_subagent_tool_specs();
        assert_eq!(specs.len(), 3);
    }

    #[test]
    fn test_get_tool_spec_by_name() {
        assert!(get_tool_spec_by_name(SUBAGENT_TOOL_GLOB).is_some());
        assert!(get_tool_spec_by_name(SUBAGENT_TOOL_GREP).is_some());
        assert!(get_tool_spec_by_name(SUBAGENT_TOOL_READ).is_some());
        assert!(get_tool_spec_by_name("Unknown").is_none());
    }

    #[test]
    fn test_execute_glob() {
        let temp = tempdir().unwrap();
        let dir = temp.path();

        std::fs::write(dir.join("test.rs"), "fn main() {}").unwrap();
        std::fs::write(dir.join("test.txt"), "hello").unwrap();

        let args = serde_json::json!({"pattern": "*.rs"}).to_string();
        let (success, output) = execute_glob(&args, dir);

        assert!(success);
        assert!(output.contains("test.rs"));
        assert!(!output.contains("test.txt"));
    }

    #[test]
    fn test_execute_read() {
        let temp = tempdir().unwrap();
        let file = temp.path().join("test.txt");

        std::fs::write(&file, "line1\nline2\nline3").unwrap();

        let args = serde_json::json!({"file_path": file.to_str().unwrap()}).to_string();
        let (success, output) = execute_read(&args, temp.path());

        assert!(success);
        assert!(output.contains("line1"));
        assert!(output.contains("line2"));
    }

    #[test]
    fn test_execute_read_nonexistent() {
        let temp = tempdir().unwrap();
        let args = serde_json::json!({"file_path": "/nonexistent/file.txt"}).to_string();
        let (success, output) = execute_read(&args, temp.path());

        assert!(!success);
        assert!(output.contains("does not exist"));
    }
}
