//! Advanced Bash tool features ported from TS BashTool/.
//!
//! TS: tools/BashTool/BashTool.tsx, bashCommandHelpers.ts,
//! commandSemantics.ts, utils.ts
//!
//! Provides output processing (truncation with intelligent boundaries),
//! command classification (search/read/list/silent), progress tracking,
//! auto-backgrounding detection, CWD tracking, image output handling,
//! and command description extraction.

use std::collections::HashSet;
use std::sync::LazyLock;
use std::time::Instant;

/// Show progress spinner after this threshold.
/// TS: PROGRESS_THRESHOLD_MS = 2000
const PROGRESS_THRESHOLD_MS: u64 = 2000;

/// Auto-background blocking commands after this budget in assistant mode.
/// TS: ASSISTANT_BLOCKING_BUDGET_MS = 15_000
const ASSISTANT_BLOCKING_BUDGET_MS: u64 = 15_000;

/// Maximum image file size for base64 detection (20 MB).
const MAX_IMAGE_FILE_SIZE: usize = 20 * 1024 * 1024;

// ── Command classification sets ──
// TS: BASH_SEARCH_COMMANDS, BASH_READ_COMMANDS, BASH_LIST_COMMANDS, etc.

static SEARCH_COMMANDS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    [
        "find", "grep", "rg", "ag", "ack", "locate", "which", "whereis",
    ]
    .into_iter()
    .collect()
});

static READ_COMMANDS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    [
        "cat", "head", "tail", "less", "more", // view
        "wc", "stat", "file", "strings", // analysis
        "jq", "awk", "cut", "sort", "uniq", "tr", // data processing
    ]
    .into_iter()
    .collect()
});

static LIST_COMMANDS: LazyLock<HashSet<&'static str>> =
    LazyLock::new(|| ["ls", "tree", "du"].into_iter().collect());

static SEMANTIC_NEUTRAL_COMMANDS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    ["echo", "printf", "true", "false", ":"]
        .into_iter()
        .collect()
});

static SILENT_COMMANDS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    [
        "mv", "cp", "rm", "mkdir", "rmdir", "chmod", "chown", "chgrp", "touch", "ln", "cd",
        "export", "unset", "wait",
    ]
    .into_iter()
    .collect()
});

/// Commands that should not be auto-backgrounded.
/// TS: DISALLOWED_AUTO_BACKGROUND_COMMANDS
static DISALLOWED_AUTO_BACKGROUND: LazyLock<HashSet<&'static str>> =
    LazyLock::new(|| ["sleep"].into_iter().collect());

/// Common long-running commands (for analytics/classification).
/// TS: COMMON_BACKGROUND_COMMANDS
static COMMON_BACKGROUND_COMMANDS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    [
        "npm",
        "yarn",
        "pnpm",
        "node",
        "python",
        "python3",
        "go",
        "cargo",
        "make",
        "docker",
        "terraform",
        "webpack",
        "vite",
        "jest",
        "pytest",
        "curl",
        "wget",
        "build",
        "test",
        "serve",
        "watch",
        "dev",
    ]
    .into_iter()
    .collect()
});

// ── Command classification ──

/// Result of classifying a command as search, read, or list.
/// TS: isSearchOrReadBashCommand() return type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandClassification {
    pub is_search: bool,
    pub is_read: bool,
    pub is_list: bool,
}

impl CommandClassification {
    /// Whether the command is collapsible in the UI.
    pub fn is_collapsible(&self) -> bool {
        self.is_search || self.is_read || self.is_list
    }
}

/// Classify a bash command as search, read, list, or none.
///
/// For pipelines, ALL non-neutral parts must be search/read/list for the
/// whole command to be collapsible.
///
/// TS: isSearchOrReadBashCommand()
pub fn classify_command(command: &str) -> CommandClassification {
    let parts = split_command_with_operators(command);
    if parts.is_empty() {
        return CommandClassification {
            is_search: false,
            is_read: false,
            is_list: false,
        };
    }

    let mut has_search = false;
    let mut has_read = false;
    let mut has_list = false;
    let mut has_non_neutral = false;
    let mut skip_next_as_redirect = false;

    for part in &parts {
        if skip_next_as_redirect {
            skip_next_as_redirect = false;
            continue;
        }

        match part.as_str() {
            ">" | ">>" | ">&" => {
                skip_next_as_redirect = true;
                continue;
            }
            "||" | "&&" | "|" | ";" => continue,
            _ => {}
        }

        let base_command = part.split_whitespace().next().unwrap_or("");
        if base_command.is_empty() {
            continue;
        }

        if SEMANTIC_NEUTRAL_COMMANDS.contains(base_command) {
            continue;
        }

        has_non_neutral = true;
        let is_part_search = SEARCH_COMMANDS.contains(base_command);
        let is_part_read = READ_COMMANDS.contains(base_command);
        let is_part_list = LIST_COMMANDS.contains(base_command);

        if !is_part_search && !is_part_read && !is_part_list {
            return CommandClassification {
                is_search: false,
                is_read: false,
                is_list: false,
            };
        }

        if is_part_search {
            has_search = true;
        }
        if is_part_read {
            has_read = true;
        }
        if is_part_list {
            has_list = true;
        }
    }

    if !has_non_neutral {
        return CommandClassification {
            is_search: false,
            is_read: false,
            is_list: false,
        };
    }

    CommandClassification {
        is_search: has_search,
        is_read: has_read,
        is_list: has_list,
    }
}

/// Check if a command is expected to produce no stdout on success.
///
/// TS: isSilentBashCommand()
pub fn is_silent_command(command: &str) -> bool {
    let parts = split_command_with_operators(command);
    if parts.is_empty() {
        return false;
    }

    let mut has_non_fallback = false;
    let mut last_operator: Option<&str> = None;
    let mut skip_next_as_redirect = false;

    for part in &parts {
        if skip_next_as_redirect {
            skip_next_as_redirect = false;
            continue;
        }

        match part.as_str() {
            ">" | ">>" | ">&" => {
                skip_next_as_redirect = true;
                continue;
            }
            op @ ("||" | "&&" | "|" | ";") => {
                last_operator = Some(op);
                continue;
            }
            _ => {}
        }

        let base_command = part.split_whitespace().next().unwrap_or("");
        if base_command.is_empty() {
            continue;
        }

        // Fallback commands after || (e.g., `rm file || echo "not found"`) are neutral
        if last_operator == Some("||") && SEMANTIC_NEUTRAL_COMMANDS.contains(base_command) {
            continue;
        }

        has_non_fallback = true;
        if !SILENT_COMMANDS.contains(base_command) {
            return false;
        }
    }

    has_non_fallback
}

// ── Auto-backgrounding ──

/// Whether a command is allowed to be automatically backgrounded.
///
/// TS: isAutobackgroundingAllowed()
pub fn is_auto_backgrounding_allowed(command: &str) -> bool {
    let base = command.split_whitespace().next().unwrap_or("");
    !DISALLOWED_AUTO_BACKGROUND.contains(base)
}

/// Detect standalone or leading `sleep N` patterns that should use Monitor.
///
/// Returns a description of the blocked pattern, or `None` if allowed.
///
/// TS: detectBlockedSleepPattern()
pub fn detect_blocked_sleep_pattern(command: &str) -> Option<String> {
    let parts = split_simple(command);
    if parts.is_empty() {
        return None;
    }

    let first = parts[0].trim();
    // Match `sleep N` where N is an integer >= 2
    let rest_after_sleep = first.strip_prefix("sleep")?;
    let rest_after_sleep = rest_after_sleep.trim();
    let secs: u64 = rest_after_sleep.parse().ok()?;

    if secs < 2 {
        return None;
    }

    let rest: String = parts[1..].join(" ").trim().to_string();
    if rest.is_empty() {
        Some(format!("standalone sleep {secs}"))
    } else {
        Some(format!("sleep {secs} followed by: {rest}"))
    }
}

// ── Command semantics ──

/// Semantic interpretation of a command's exit code.
///
/// TS: CommandSemantic, COMMAND_SEMANTICS
#[derive(Debug, Clone)]
pub struct CommandInterpretation {
    pub is_error: bool,
    pub message: Option<String>,
}

/// Interpret a command result using semantic rules.
///
/// TS: interpretCommandResult()
pub fn interpret_command_result(
    command: &str,
    exit_code: i32,
    _stdout: &str,
    _stderr: &str,
) -> CommandInterpretation {
    let base_command = command.split_whitespace().next().unwrap_or("");

    match base_command {
        // grep/rg: 0=matches found, 1=no matches, 2+=error
        "grep" | "rg" => CommandInterpretation {
            is_error: exit_code >= 2,
            message: if exit_code == 1 {
                Some("No matches found".into())
            } else {
                None
            },
        },
        // find: 0=success, 1=partial success, 2+=error
        "find" => CommandInterpretation {
            is_error: exit_code >= 2,
            message: if exit_code == 1 {
                Some("Some directories were inaccessible".into())
            } else {
                None
            },
        },
        // diff: 0=no differences, 1=differences found, 2+=error
        "diff" => CommandInterpretation {
            is_error: exit_code >= 2,
            message: if exit_code == 1 {
                Some("Files differ".into())
            } else {
                None
            },
        },
        // test/[: 0=true, 1=false, 2+=error
        "test" | "[" => CommandInterpretation {
            is_error: exit_code >= 2,
            message: if exit_code == 1 {
                Some("Condition is false".into())
            } else {
                None
            },
        },
        // Default: non-zero is error
        _ => CommandInterpretation {
            is_error: exit_code != 0,
            message: if exit_code != 0 {
                Some(format!("Command failed with exit code {exit_code}"))
            } else {
                None
            },
        },
    }
}

// ── Output processing ──

/// Truncate output with intelligent boundaries (line-aware).
///
/// TS: EndTruncatingAccumulator + stripEmptyLines + output truncation logic
pub fn truncate_output_intelligent(output: &str, max_bytes: usize) -> (String, bool) {
    if output.len() <= max_bytes {
        return (strip_empty_lines(output), false);
    }

    // Find the last newline within the budget to avoid splitting mid-line
    let truncated = &output[..max_bytes];
    let break_point = truncated.rfind('\n').map(|p| p + 1).unwrap_or(max_bytes);

    let result = &output[..break_point];
    let stripped = strip_empty_lines(result);
    let note = format!(
        "\n... (output truncated, {total} bytes total)",
        total = output.len()
    );
    (format!("{stripped}{note}"), true)
}

/// Strip leading and trailing lines that contain only whitespace.
///
/// TS: stripEmptyLines()
pub fn strip_empty_lines(content: &str) -> String {
    let lines: Vec<&str> = content.split('\n').collect();

    let start = lines
        .iter()
        .position(|l| !l.trim().is_empty())
        .unwrap_or(lines.len());
    let end = lines
        .iter()
        .rposition(|l| !l.trim().is_empty())
        .map(|e| e + 1)
        .unwrap_or(0);

    if start >= end {
        return String::new();
    }

    lines[start..end].join("\n")
}

// ── Image output detection ──

/// Check if content is a base64-encoded image data URL.
///
/// TS: isImageOutput()
pub fn is_image_output(content: &str) -> bool {
    // Match `data:image/<subtype>;base64,`
    content.len() < MAX_IMAGE_FILE_SIZE
        && content.starts_with("data:image/")
        && content.contains(";base64,")
}

/// Parse a data-URI into media type and base64 payload.
///
/// TS: parseDataUri()
pub fn parse_data_uri(s: &str) -> Option<(&str, &str)> {
    let s = s.trim();
    let rest = s.strip_prefix("data:")?;
    let semi_pos = rest.find(';')?;
    let media_type = &rest[..semi_pos];
    let after_semi = &rest[semi_pos + 1..];
    let payload = after_semi.strip_prefix("base64,")?;
    Some((media_type, payload))
}

// ── Progress tracking ──

/// Tracks progress for a running bash command.
///
/// TS: BashProgress + progress tracking in runShellCommand
#[derive(Debug)]
pub struct BashProgressTracker {
    start_time: Instant,
    last_output: String,
    total_lines: i64,
    total_bytes: i64,
    progress_emitted: bool,
}

impl Default for BashProgressTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl BashProgressTracker {
    pub fn new() -> Self {
        Self {
            start_time: Instant::now(),
            last_output: String::new(),
            total_lines: 0,
            total_bytes: 0,
            progress_emitted: false,
        }
    }

    /// Update with new output data.
    pub fn update(&mut self, output: &str, total_bytes: i64) {
        self.last_output = output.to_string();
        self.total_lines = output.lines().count() as i64;
        self.total_bytes = total_bytes;
    }

    /// Whether we should emit a progress update (past the 2-second threshold).
    pub fn should_emit_progress(&self) -> bool {
        self.elapsed_ms() >= PROGRESS_THRESHOLD_MS
    }

    /// Whether we should auto-background (past the assistant blocking budget).
    pub fn should_auto_background(&self) -> bool {
        self.elapsed_ms() >= ASSISTANT_BLOCKING_BUDGET_MS
    }

    /// Elapsed time in milliseconds.
    pub fn elapsed_ms(&self) -> u64 {
        self.start_time.elapsed().as_millis() as u64
    }

    /// Elapsed time in seconds (for progress display).
    pub fn elapsed_seconds(&self) -> f64 {
        self.start_time.elapsed().as_secs_f64()
    }

    /// Build a progress snapshot.
    pub fn snapshot(&mut self) -> BashProgress {
        self.progress_emitted = true;
        BashProgress {
            output: self.last_output.clone(),
            elapsed_time_seconds: self.elapsed_seconds(),
            total_lines: self.total_lines,
            total_bytes: self.total_bytes,
        }
    }

    /// Whether any progress was ever emitted.
    pub fn was_progress_emitted(&self) -> bool {
        self.progress_emitted
    }
}

/// A progress snapshot for a running bash command.
///
/// TS: BashProgress
#[derive(Debug, Clone)]
pub struct BashProgress {
    pub output: String,
    pub elapsed_time_seconds: f64,
    pub total_lines: i64,
    pub total_bytes: i64,
}

// ── CWD tracking ──

/// Check if a command contains any `cd` subcommands.
///
/// TS: commandHasAnyCd()
pub fn command_has_any_cd(command: &str) -> bool {
    split_simple(command).iter().any(|part| {
        let trimmed = part.trim();
        trimmed == "cd" || trimmed.starts_with("cd ")
    })
}

// ── Command type classification (analytics) ──

/// Get the command type for logging/analytics.
///
/// TS: getCommandTypeForLogging()
pub fn get_command_type_for_logging(command: &str) -> &'static str {
    for part in split_simple(command) {
        let base = part.split_whitespace().next().unwrap_or("");
        if let Some(matched) = COMMON_BACKGROUND_COMMANDS.get(base) {
            return matched;
        }
    }
    "other"
}

/// Extract description from the tool input, falling back to command truncation.
///
/// TS: BashTool.description() + getToolUseSummary() + getActivityDescription()
pub fn extract_description(command: &str, description: Option<&str>) -> String {
    if let Some(desc) = description
        && !desc.is_empty()
    {
        return desc.to_string();
    }
    // Truncate command to a reasonable summary length
    const MAX_SUMMARY_LEN: usize = 80;
    if command.len() <= MAX_SUMMARY_LEN {
        command.to_string()
    } else {
        format!("{}...", &command[..MAX_SUMMARY_LEN])
    }
}

// ── Internal helpers ──

/// Simple command splitting on `&&`, `||`, `;` delimiters.
fn split_simple(command: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut chars = command.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            '&' if chars.peek() == Some(&'&') => {
                chars.next();
                if !current.trim().is_empty() {
                    parts.push(current.trim().to_string());
                }
                current.clear();
            }
            '|' if chars.peek() == Some(&'|') => {
                chars.next();
                if !current.trim().is_empty() {
                    parts.push(current.trim().to_string());
                }
                current.clear();
            }
            ';' => {
                if !current.trim().is_empty() {
                    parts.push(current.trim().to_string());
                }
                current.clear();
            }
            _ => current.push(c),
        }
    }

    if !current.trim().is_empty() {
        parts.push(current.trim().to_string());
    }

    parts
}

/// Split command into parts preserving operators as separate tokens.
///
/// TS: splitCommandWithOperators() (simplified)
fn split_command_with_operators(command: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut chars = command.chars().peekable();
    let mut in_single_quote = false;
    let mut in_double_quote = false;

    while let Some(c) = chars.next() {
        if c == '\'' && !in_double_quote {
            in_single_quote = !in_single_quote;
            current.push(c);
            continue;
        }
        if c == '"' && !in_single_quote {
            in_double_quote = !in_double_quote;
            current.push(c);
            continue;
        }

        if in_single_quote || in_double_quote {
            current.push(c);
            continue;
        }

        match c {
            '&' if chars.peek() == Some(&'&') => {
                chars.next();
                if !current.trim().is_empty() {
                    parts.push(current.trim().to_string());
                }
                current.clear();
                parts.push("&&".to_string());
            }
            '|' if chars.peek() == Some(&'|') => {
                chars.next();
                if !current.trim().is_empty() {
                    parts.push(current.trim().to_string());
                }
                current.clear();
                parts.push("||".to_string());
            }
            '|' => {
                if !current.trim().is_empty() {
                    parts.push(current.trim().to_string());
                }
                current.clear();
                parts.push("|".to_string());
            }
            ';' => {
                if !current.trim().is_empty() {
                    parts.push(current.trim().to_string());
                }
                current.clear();
                parts.push(";".to_string());
            }
            '>' if chars.peek() == Some(&'>') => {
                chars.next();
                if !current.trim().is_empty() {
                    parts.push(current.trim().to_string());
                }
                current.clear();
                parts.push(">>".to_string());
            }
            '>' if chars.peek() == Some(&'&') => {
                chars.next();
                if !current.trim().is_empty() {
                    parts.push(current.trim().to_string());
                }
                current.clear();
                parts.push(">&".to_string());
            }
            '>' => {
                if !current.trim().is_empty() {
                    parts.push(current.trim().to_string());
                }
                current.clear();
                parts.push(">".to_string());
            }
            _ => current.push(c),
        }
    }

    if !current.trim().is_empty() {
        parts.push(current.trim().to_string());
    }

    parts
}

#[cfg(test)]
#[path = "bash_advanced.test.rs"]
mod tests;
