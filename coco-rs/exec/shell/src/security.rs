//! Shell command security checks.
//!
//! Runs a battery of checks against command strings to detect injection,
//! obfuscation, and dangerous patterns before execution.

use crate::safety::SecurityCheckId;

/// Result of a single security check.
#[derive(Debug, Clone)]
pub struct SecurityCheck {
    pub id: SecurityCheckId,
    pub severity: SecuritySeverity,
    pub message: String,
}

/// Severity level for a security check result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecuritySeverity {
    /// Command must be denied outright.
    Deny,
    /// Command needs user confirmation.
    Ask,
}

/// Run all security checks on a command string.
pub fn check_security(command: &str) -> Vec<SecurityCheck> {
    let checks: [fn(&str) -> Option<SecurityCheck>; 5] = [
        check_ifs_injection,
        check_dangerous_patterns,
        check_shell_metacharacters,
        check_control_characters,
        check_proc_environ,
    ];

    checks.iter().filter_map(|check| check(command)).collect()
}

/// Check for IFS variable manipulation.
fn check_ifs_injection(command: &str) -> Option<SecurityCheck> {
    if command.contains("IFS=") {
        return Some(SecurityCheck {
            id: SecurityCheckId::IFS_INJECTION,
            severity: SecuritySeverity::Deny,
            message: "IFS variable manipulation detected".into(),
        });
    }
    None
}

/// Check for dangerous command patterns: eval, exec, source /dev/, backticks.
fn check_dangerous_patterns(command: &str) -> Option<SecurityCheck> {
    // Check for backtick command substitution
    if command.contains('`') {
        return Some(SecurityCheck {
            id: SecurityCheckId::DANGEROUS_PATTERNS_GENERAL,
            severity: SecuritySeverity::Ask,
            message: "backtick command substitution detected".into(),
        });
    }

    let words: Vec<&str> = command.split_whitespace().collect();
    for (i, word) in words.iter().enumerate() {
        match *word {
            "eval" => {
                return Some(SecurityCheck {
                    id: SecurityCheckId::DANGEROUS_PATTERNS_GENERAL,
                    severity: SecuritySeverity::Deny,
                    message: "eval command detected".into(),
                });
            }
            "exec" => {
                return Some(SecurityCheck {
                    id: SecurityCheckId::DANGEROUS_PATTERNS_GENERAL,
                    severity: SecuritySeverity::Ask,
                    message: "exec command detected".into(),
                });
            }
            "source" | "." => {
                if let Some(next) = words.get(i + 1)
                    && next.starts_with("/dev/")
                {
                    return Some(SecurityCheck {
                        id: SecurityCheckId::DANGEROUS_PATTERNS_GENERAL,
                        severity: SecuritySeverity::Deny,
                        message: "source from /dev/ detected".into(),
                    });
                }
            }
            _ => {}
        }
    }

    None
}

/// Check for shell metacharacters: $(...) substitution, unescaped pipes
/// chained with dangerous commands.
fn check_shell_metacharacters(command: &str) -> Option<SecurityCheck> {
    // $(...) command substitution
    if command.contains("$(") {
        return Some(SecurityCheck {
            id: SecurityCheckId::SHELL_METACHARACTERS,
            severity: SecuritySeverity::Ask,
            message: "command substitution $(...) detected".into(),
        });
    }

    // Piped dangerous commands: pipe into sh/bash/eval
    if command.contains('|') {
        let parts: Vec<&str> = command.split('|').collect();
        for part in parts.iter().skip(1) {
            let first_word = part.split_whitespace().next().unwrap_or("");
            if matches!(first_word, "sh" | "bash" | "eval" | "exec") {
                return Some(SecurityCheck {
                    id: SecurityCheckId::SHELL_METACHARACTERS,
                    severity: SecuritySeverity::Deny,
                    message: format!("pipe into {first_word} detected"),
                });
            }
        }
    }

    // && chaining with dangerous commands
    if command.contains("&&") {
        let parts: Vec<&str> = command.split("&&").collect();
        for part in &parts {
            let first_word = part.split_whitespace().next().unwrap_or("");
            if matches!(first_word, "eval" | "exec") {
                return Some(SecurityCheck {
                    id: SecurityCheckId::SHELL_METACHARACTERS,
                    severity: SecuritySeverity::Deny,
                    message: format!("{first_word} in chained command detected"),
                });
            }
        }
    }

    None
}

/// Check for Unicode control characters and zero-width characters.
fn check_control_characters(command: &str) -> Option<SecurityCheck> {
    for ch in command.chars() {
        // Zero-width chars
        if matches!(ch, '\u{200B}' | '\u{200C}' | '\u{200D}' | '\u{FEFF}') {
            return Some(SecurityCheck {
                id: SecurityCheckId::CONTROL_CHARACTERS,
                severity: SecuritySeverity::Deny,
                message: format!("zero-width character U+{:04X} detected", ch as u32),
            });
        }
        // Unicode control characters (excluding common whitespace)
        if ch.is_control() && !matches!(ch, '\n' | '\r' | '\t') {
            return Some(SecurityCheck {
                id: SecurityCheckId::CONTROL_CHARACTERS,
                severity: SecuritySeverity::Deny,
                message: format!("control character U+{:04X} detected", ch as u32),
            });
        }
    }
    None
}

/// Check for /proc/*/environ access.
fn check_proc_environ(command: &str) -> Option<SecurityCheck> {
    if command.contains("/proc/") && command.contains("/environ") {
        return Some(SecurityCheck {
            id: SecurityCheckId::PROC_ENVIRON_ACCESS,
            severity: SecuritySeverity::Deny,
            message: "/proc/*/environ access detected".into(),
        });
    }
    None
}

#[cfg(test)]
#[path = "security.test.rs"]
mod tests;
