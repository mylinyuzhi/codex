//! Stall detection for background shell tasks.
//!
//! Constants and prompt-detection logic. The watchdog *loop* lives
//! in the app-layer crate that drives the shell child process
//! (`coco_cli::task_runtime::stall::watchdog`); this module owns
//! the pure-logic detection bits so the loop's two ingredients —
//! "is output frozen?" and "does the tail look like a prompt?" —
//! can be unit-tested without spinning a tokio runtime.
//!
//! TS source:
//! - `tasks/LocalShellTask/LocalShellTask.tsx:24-26` — constants.
//! - `tasks/LocalShellTask/LocalShellTask.tsx:32-42` —
//!   `PROMPT_PATTERNS` + `looksLikePrompt`.

/// Interval between stall checks. TS:
/// `LocalShellTask.tsx:24` `STALL_CHECK_INTERVAL_MS = 5_000`.
pub const STALL_CHECK_INTERVAL_MS: u64 = 5_000;

/// Output must be frozen for this long before a stall fires. TS:
/// `LocalShellTask.tsx:25` `STALL_THRESHOLD_MS = 45_000`.
pub const STALL_THRESHOLD_MS: u64 = 45_000;

/// Tail bytes to sample for prompt detection. TS:
/// `LocalShellTask.tsx:26` `STALL_TAIL_BYTES = 1024`.
pub const STALL_TAIL_BYTES: usize = 1024;

/// Returns `true` if the last non-empty line of `tail` matches one
/// of the interactive-prompt heuristics. Mirrors TS
/// `LocalShellTask.tsx:32-42`'s regex set:
///
/// ```text
/// /\(y\/n\)/i, /\[y\/n\]/i, /\(yes\/no\)/i,
/// /\b(?:Do you|Would you|Shall I|Are you sure|Ready to)\b.*\? *$/i,
/// /Press (any key|Enter)/i,
/// /Continue\?/i, /Overwrite\?/i
/// ```
///
/// Only the **last** non-empty line is checked — anything earlier
/// might just be transcript text that mentions a prompt-shaped
/// string (matches TS comment "to avoid false positives").
pub fn matches_interactive_prompt(tail: &str) -> bool {
    let last_line = tail.trim_end().rsplit('\n').next().unwrap_or("").trim();
    if last_line.is_empty() {
        return false;
    }
    let lower = last_line.to_lowercase();

    // Literal substrings: yes/no + password prompts. TS encodes
    // these as regex but they're all literal — match plain
    // substring on the lowercased tail.
    let string_patterns = [
        "(y/n)",
        "[y/n]",
        "y/n",
        "(yes/no)",
        "[yes/no]",
        "yes/no",
        "password:",
        "passphrase:",
        "[sudo]",
        "enter passphrase",
    ];
    if string_patterns.iter().any(|p| lower.contains(p)) {
        return true;
    }

    // Directed questions: must contain a directive AND end with `?`.
    let directives = ["do you", "would you", "shall i", "are you sure", "ready to"];
    if (lower.ends_with('?') || lower.ends_with("? "))
        && directives.iter().any(|d| lower.contains(d))
    {
        return true;
    }

    // Standalone action prompts.
    if ["continue?", "overwrite?", "proceed?"]
        .iter()
        .any(|p| lower.contains(p))
    {
        return true;
    }

    // "Press any key" / "Press Enter".
    if lower.contains("press any key") || lower.contains("press enter") {
        return true;
    }

    false
}

#[cfg(test)]
#[path = "stall.test.rs"]
mod tests;
