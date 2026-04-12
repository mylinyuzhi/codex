//! Mode-based command validation.
//!
//! TS: modeValidation.ts — in acceptEdits mode, auto-allow certain file
//! manipulation commands (mkdir, touch, rm, rmdir, mv, cp, sed) without
//! requiring user approval.

/// Commands that are auto-allowed in acceptEdits mode.
///
/// These are file-manipulation commands that are expected side effects
/// when the model is editing files. No flag restriction — any usage is allowed.
const ACCEPT_EDITS_COMMANDS: &[&str] = &["mkdir", "touch", "rm", "rmdir", "mv", "cp", "sed"];

/// Check if a command should be auto-allowed in acceptEdits mode.
///
/// Returns true if the command's base executable is in the auto-allow list.
pub fn is_auto_allowed_in_accept_edits(command: &str) -> bool {
    let trimmed = command.trim();
    if trimmed.is_empty() {
        return false;
    }

    let base = extract_base_executable(trimmed);
    ACCEPT_EDITS_COMMANDS.contains(&base)
}

/// Extract the base executable name from a command string.
/// Strips path prefixes and env var assignments.
fn extract_base_executable(command: &str) -> &str {
    let mut rest = command;

    // Skip env var assignments (VAR=value ...)
    loop {
        let first_word = rest.split_whitespace().next().unwrap_or("");
        if first_word.contains('=') && !first_word.starts_with('=') {
            // Skip past this env assignment
            rest = rest[first_word.len()..].trim_start();
        } else {
            break;
        }
    }

    // Get the first word and strip path
    let cmd = rest.split_whitespace().next().unwrap_or("");
    cmd.rsplit('/').next().unwrap_or(cmd)
}

#[cfg(test)]
#[path = "mode_validation.test.rs"]
mod tests;
