use pretty_assertions::assert_eq;

use super::*;

#[test]
fn test_exit_code_zero_is_success() {
    assert_eq!(
        interpret_command_result("ls", 0),
        CommandResultInterpretation::Success
    );
    assert_eq!(
        interpret_command_result("grep foo bar.txt", 0),
        CommandResultInterpretation::Success
    );
}

// ── grep ──

#[test]
fn test_grep_no_match_is_expected() {
    assert_eq!(
        interpret_command_result("grep pattern file.txt", 1),
        CommandResultInterpretation::ExpectedFailure {
            explanation: "no matches found".into()
        }
    );
}

#[test]
fn test_grep_error_is_error() {
    assert_eq!(
        interpret_command_result("grep pattern file.txt", 2),
        CommandResultInterpretation::Error
    );
}

#[test]
fn test_rg_no_match_is_expected() {
    assert_eq!(
        interpret_command_result("rg pattern", 1),
        CommandResultInterpretation::ExpectedFailure {
            explanation: "no matches found".into()
        }
    );
}

// ── diff ──

#[test]
fn test_diff_files_differ_is_expected() {
    assert_eq!(
        interpret_command_result("diff file1 file2", 1),
        CommandResultInterpretation::ExpectedFailure {
            explanation: "files differ".into()
        }
    );
}

#[test]
fn test_diff_error_is_error() {
    assert_eq!(
        interpret_command_result("diff file1 file2", 2),
        CommandResultInterpretation::Error
    );
}

// ── test / [ ──

#[test]
fn test_test_false_is_expected() {
    assert_eq!(
        interpret_command_result("test -f missing.txt", 1),
        CommandResultInterpretation::ExpectedFailure {
            explanation: "condition evaluated to false".into()
        }
    );
}

#[test]
fn test_bracket_false_is_expected() {
    assert_eq!(
        interpret_command_result("[ -d /nonexistent ]", 1),
        CommandResultInterpretation::ExpectedFailure {
            explanation: "condition evaluated to false".into()
        }
    );
}

// ── git diff ──

#[test]
fn test_git_diff_differences_is_expected() {
    assert_eq!(
        interpret_command_result("git diff --exit-code", 1),
        CommandResultInterpretation::ExpectedFailure {
            explanation: "git diff found differences".into()
        }
    );
}

#[test]
fn test_git_push_error_is_error() {
    assert_eq!(
        interpret_command_result("git push origin main", 1),
        CommandResultInterpretation::Error
    );
}

// ── curl ──

#[test]
fn test_curl_http_error_is_expected() {
    assert_eq!(
        interpret_command_result("curl -f http://example.com/404", 22),
        CommandResultInterpretation::ExpectedFailure {
            explanation: "HTTP error response (e.g. 404)".into()
        }
    );
}

// ── timeout ──

#[test]
fn test_timeout_is_expected() {
    assert_eq!(
        interpret_command_result("timeout 5 sleep 10", 124),
        CommandResultInterpretation::ExpectedFailure {
            explanation: "command timed out".into()
        }
    );
}

// ── unknown commands ──

#[test]
fn test_unknown_command_nonzero_is_error() {
    assert_eq!(
        interpret_command_result("mycommand --flag", 1),
        CommandResultInterpretation::Error
    );
    assert_eq!(
        interpret_command_result("ls /nonexistent", 2),
        CommandResultInterpretation::Error
    );
}

// ── helpers ──

#[test]
fn test_extract_base_command() {
    assert_eq!(extract_base_command("grep foo"), "grep");
    assert_eq!(extract_base_command("  git status"), "git");
    assert_eq!(extract_base_command("VAR=val command arg"), "command");
}

#[test]
fn test_command_has_subcommand() {
    assert!(command_has_subcommand("git diff --exit-code", "diff"));
    assert!(command_has_subcommand("git diff file1 file2", "diff"));
    assert!(!command_has_subcommand("git push origin", "diff"));
    assert!(!command_has_subcommand("diff file1 file2", "diff"));
}
