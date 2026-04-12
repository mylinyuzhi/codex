//! Command result interpretation based on exit code semantics.
//!
//! Some commands use non-zero exit codes for expected outcomes
//! (e.g., `grep` returns 1 for "no match"). This module interprets
//! exit codes in the context of the command that produced them.

/// Interpretation of a command's exit code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandResultInterpretation {
    /// Command succeeded (exit code 0).
    Success,
    /// Non-zero exit code, but expected for this command.
    ExpectedFailure { explanation: String },
    /// Genuine error.
    Error,
}

/// Interpret an exit code based on the command that produced it.
///
/// Returns `Success` for exit code 0, checks known command semantics
/// for non-zero codes, and falls back to `Error` otherwise.
pub fn interpret_command_result(command: &str, exit_code: i32) -> CommandResultInterpretation {
    if exit_code == 0 {
        return CommandResultInterpretation::Success;
    }

    let base_command = extract_base_command(command);

    match (base_command, exit_code) {
        // grep: exit 1 = no match found
        ("grep" | "egrep" | "fgrep" | "rg" | "ag", 1) => {
            CommandResultInterpretation::ExpectedFailure {
                explanation: "no matches found".into(),
            }
        }
        // diff: exit 1 = files differ
        ("diff", 1) => CommandResultInterpretation::ExpectedFailure {
            explanation: "files differ".into(),
        },
        // diff: exit 2 = trouble (actual error)
        ("diff", 2) => CommandResultInterpretation::Error,
        // test / [: exit 1 = condition is false
        ("test" | "[", 1) => CommandResultInterpretation::ExpectedFailure {
            explanation: "condition evaluated to false".into(),
        },
        // curl: exit 22 = HTTP error (404 etc.), still can be expected
        ("curl", 22) => CommandResultInterpretation::ExpectedFailure {
            explanation: "HTTP error response (e.g. 404)".into(),
        },
        // git diff: exit 1 when using --exit-code means differences found
        ("git", 1) if command_has_subcommand(command, "diff") => {
            CommandResultInterpretation::ExpectedFailure {
                explanation: "git diff found differences".into(),
            }
        }
        // timeout: exit 124 = command timed out (often expected in CI)
        ("timeout", 124) => CommandResultInterpretation::ExpectedFailure {
            explanation: "command timed out".into(),
        },
        // All other non-zero codes are errors
        _ => CommandResultInterpretation::Error,
    }
}

/// Extract the base command name from a command string.
fn extract_base_command(command: &str) -> &str {
    let trimmed = command.trim();

    // Skip env var assignments (VAR=value)
    let mut rest = trimmed;
    while let Some(eq_pos) = rest.find('=') {
        let before_eq = &rest[..eq_pos];
        if before_eq.contains(' ') {
            break;
        }
        rest = rest[eq_pos + 1..].trim_start();
        if let Some(space_pos) = rest.find(' ') {
            rest = rest[space_pos..].trim_start();
        } else {
            return rest;
        }
    }

    rest.split_whitespace().next().unwrap_or("")
}

/// Check if a command string contains a given subcommand.
fn command_has_subcommand(command: &str, subcommand: &str) -> bool {
    command
        .split_whitespace()
        .skip(1)
        .any(|word| word == subcommand)
}

#[cfg(test)]
#[path = "semantics.test.rs"]
mod tests;
