//! Read-only command detection for safe execution without sandbox.
//!
//! This module provides two levels of read-only command detection:
//!
//! 1. **Fast path** (`is_read_only_command`): Simple whitelist-based detection
//!    for known safe commands without shell operators.
//!
//! 2. **Enhanced detection** (`analyze_command_safety`): Deep security analysis
//!    using shell-parser that detects 24 different risk types across two phases.
//!
//! # Security Analysis
//!
//! The enhanced detection leverages `cocode-shell-parser` for comprehensive
//! security analysis including:
//!
//! - Command injection via metacharacters
//! - Privilege escalation (sudo, su, etc.)
//! - File system tampering (rm -rf, chmod, etc.)
//! - Network exfiltration attempts
//! - Code execution risks (eval, exec, etc.)
//! - Obfuscated flags and dangerous substitutions
//!
//! # Example
//!
//! ```
//! use cocode_shell::{is_read_only_command, analyze_command_safety, SafetyResult};
//!
//! // Fast path: simple whitelist check
//! assert!(is_read_only_command("ls -la"));
//! assert!(!is_read_only_command("rm -rf /"));
//!
//! // Enhanced analysis: deep security check
//! let result = analyze_command_safety("ls -la");
//! assert!(matches!(result, SafetyResult::Safe { .. }));
//!
//! // Dangerous commands require approval or are denied
//! let result = analyze_command_safety("sudo rm -rf /");
//! assert!(!result.is_safe());
//! ```

use cocode_shell_parser::ShellParser;
use cocode_shell_parser::security::RiskKind;
use cocode_shell_parser::security::RiskLevel;
use cocode_shell_parser::security::RiskPhase;
use cocode_shell_parser::security::SecurityAnalysis;
use cocode_shell_parser::security::SecurityRisk;

/// Known safe read-only commands that do not modify the system.
///
/// Aligned with Claude Code's SAFE_COMMAND_REGISTRY + SAFE_COMMAND_PATTERNS.
const READ_ONLY_COMMANDS: &[&str] = &[
    // File inspection
    "ls", "cat", "head", "tail", "wc", "grep", "rg", "find", "which", "file", "stat", "diff",
    "strings", "hexdump", "od", "nl", "readlink", // Identity / system info
    "whoami", "pwd", "echo", "date", "env", "printenv", "uname", "hostname", "id", "groups",
    "arch", "nproc", "locale", "uptime", "cal", // Disk / resource queries
    "df", "du", "free", // Text processing (read-only)
    "cut", "paste", "tr", "column", "fold", "expand", "unexpand", "rev", "tac", "uniq", "seq",
    "sort", "expr", // Path utilities
    "basename", "dirname", "realpath", // Type/command lookup
    "type", "command", // Sleep (harmless)
    "sleep",   // Boolean builtins
    "true", "false", // Git (with subcommand checking below)
    "git",   // Docker (with subcommand checking below)
    "docker",
];

/// Shell operators that may cause side effects (piping to commands, chaining, redirects).
const UNSAFE_OPERATORS: &[&str] = &["&&", "||", ";", "|", ">", "<"];

/// Git subcommands that are purely read-only.
///
/// Aligned with Claude Code's SAFE_COMMAND_REGISTRY git entries.
const GIT_READ_ONLY_SUBCOMMANDS: &[&str] = &[
    "status",
    "log",
    "diff",
    "show",
    "branch",
    "tag",
    "remote",
    "blame",
    "grep",
    "shortlog",
    "reflog",
    "ls-remote",
    "ls-files",
    "merge-base",
    "rev-parse",
    "rev-list",
    "describe",
    "cat-file",
    "for-each-ref",
];

/// Git two-word subcommands that are purely read-only (e.g. "stash list").
const GIT_READ_ONLY_TWO_WORD_SUBCOMMANDS: &[(&str, &str)] = &[
    ("stash", "list"),
    ("stash", "show"),
    ("worktree", "list"),
    ("config", "--get"),
];

/// Docker subcommands that are purely read-only.
const DOCKER_READ_ONLY_SUBCOMMANDS: &[&str] = &[
    "ps", "images", "stats", "diff", "port", "logs", "inspect", "info", "version",
];

/// Docker two-word subcommands that are purely read-only (e.g. "compose ps").
const DOCKER_READ_ONLY_TWO_WORD_SUBCOMMANDS: &[(&str, &str)] = &[
    ("compose", "ps"),
    ("compose", "top"),
    ("compose", "config"),
    ("compose", "logs"),
];

/// Git flags that enable arbitrary code execution and must be rejected.
const GIT_DANGEROUS_FLAGS: &[&str] = &["-c", "--exec-path", "--config-env"];

/// Result of command safety analysis.
#[derive(Debug, Clone)]
pub enum SafetyResult {
    /// Command is safe to execute without approval.
    Safe {
        /// Whether detected via fast whitelist path.
        via_whitelist: bool,
    },
    /// Command requires user approval before execution.
    RequiresApproval {
        /// Security risks that were detected.
        risks: Vec<SecurityRisk>,
        /// The highest risk level detected.
        max_level: RiskLevel,
    },
    /// Command is denied (critical risk detected).
    Denied {
        /// The reason for denial.
        reason: String,
        /// The critical risks detected.
        risks: Vec<SecurityRisk>,
    },
}

impl SafetyResult {
    /// Returns true if the command is safe to execute without approval.
    pub fn is_safe(&self) -> bool {
        matches!(self, SafetyResult::Safe { .. })
    }

    /// Returns true if the command requires user approval.
    pub fn requires_approval(&self) -> bool {
        matches!(self, SafetyResult::RequiresApproval { .. })
    }

    /// Returns true if the command should be denied.
    pub fn is_denied(&self) -> bool {
        matches!(self, SafetyResult::Denied { .. })
    }

    /// Returns the security risks if any were detected.
    pub fn risks(&self) -> &[SecurityRisk] {
        match self {
            SafetyResult::Safe { .. } => &[],
            SafetyResult::RequiresApproval { risks, .. } => risks,
            SafetyResult::Denied { risks, .. } => risks,
        }
    }
}

/// Analyzes a command for safety using a hybrid approach.
///
/// This function combines fast whitelist-based detection with comprehensive
/// security analysis for the best balance of speed and security:
///
/// 1. **Fast path**: If the command matches a known read-only pattern
///    (simple command without shell operators), it's immediately approved.
///
/// 2. **Deep analysis**: For complex commands, the shell-parser performs
///    comprehensive security analysis detecting 14 risk types.
///
/// # Returns
///
/// - `SafetyResult::Safe` - Command is safe to execute without approval
/// - `SafetyResult::RequiresApproval` - Command has risks that need user review
/// - `SafetyResult::Denied` - Command has critical risks and should be blocked
///
/// # Example
///
/// ```
/// use cocode_shell::{analyze_command_safety, SafetyResult};
///
/// // Simple read-only command (fast path)
/// let result = analyze_command_safety("ls -la");
/// assert!(result.is_safe());
///
/// // Complex but safe pipeline
/// let result = analyze_command_safety("cat file.txt | grep pattern");
/// assert!(result.is_safe());
///
/// // Dangerous command
/// let result = analyze_command_safety("sudo rm -rf /");
/// assert!(result.requires_approval() || result.is_denied());
/// ```
pub fn analyze_command_safety(command: &str) -> SafetyResult {
    // Step 1: Fast path - simple whitelist check
    if is_simple_read_only(command) {
        return SafetyResult::Safe {
            via_whitelist: true,
        };
    }

    // Step 1.5: Compound command security checks
    if let Some(result) = check_compound_command_safety(command) {
        return result;
    }

    // Step 2: Deep security analysis via shell-parser
    let mut parser = ShellParser::new();
    let cmd = parser.parse(command);
    let analysis = cocode_shell_parser::security::analyze(&cmd);

    // Convert analysis to SafetyResult
    analyze_security_result(&cmd, analysis)
}

/// Maximum number of subcommands before requiring approval (prevents DoS).
/// Matches Claude Code's Rfq=50 cap.
const MAX_SUBCOMMAND_COUNT: usize = 50;

/// Check compound commands for cd+git and multiple-cd patterns.
///
/// Uses `ShellParser` for proper quote-aware subcommand extraction
/// (unlike raw string splitting which breaks on `git commit -m "fix; refactor"`).
///
/// Returns `Some(SafetyResult)` if the command should be flagged,
/// `None` if compound checks pass and normal analysis should continue.
fn check_compound_command_safety(command: &str) -> Option<SafetyResult> {
    let mut parser = ShellParser::new();
    let parsed = parser.parse(command);
    let commands = parsed.extract_commands();

    // If there's only one subcommand, no compound checks needed
    if commands.len() <= 1 {
        return None;
    }

    // Subcommand count cap — prevent analysis DoS
    if commands.len() > MAX_SUBCOMMAND_COUNT {
        return Some(SafetyResult::RequiresApproval {
            risks: Vec::new(),
            max_level: RiskLevel::Medium,
        });
    }

    // Count cd commands — multiple directory changes require approval
    // (prevents bare repository attacks)
    let cd_count = commands
        .iter()
        .filter(|args| args.first().is_some_and(|s| s == "cd"))
        .count();

    if cd_count > 1 {
        return Some(SafetyResult::RequiresApproval {
            risks: vec![SecurityRisk::new(
                RiskKind::CodeExecution,
                "Multiple directory changes in compound command".to_string(),
            )],
            max_level: RiskLevel::Medium,
        });
    }

    // cd+git compound check — cd followed by non-read-only git requires approval
    if cd_count > 0 {
        let has_unsafe_git = commands.iter().any(|args| {
            args.first().is_some_and(|s| s == "git") && !is_git_read_only_internal(&args.join(" "))
        });
        if has_unsafe_git {
            return Some(SafetyResult::RequiresApproval {
                risks: vec![SecurityRisk::new(
                    RiskKind::CodeExecution,
                    "Directory change combined with git write operation".to_string(),
                )],
                max_level: RiskLevel::Medium,
            });
        }
    }

    None
}

/// Converts shell-parser security analysis to SafetyResult.
fn analyze_security_result(
    cmd: &cocode_shell_parser::ParsedShell,
    analysis: SecurityAnalysis,
) -> SafetyResult {
    // No risks detected - check if command is word-only (safe structure)
    if !analysis.has_risks() {
        // Additional check: can we extract safe commands?
        if cmd.try_extract_safe_commands().is_some() {
            return SafetyResult::Safe {
                via_whitelist: false,
            };
        }
        // Even without explicit risks, non-word-only commands need review
        return SafetyResult::RequiresApproval {
            risks: Vec::new(),
            max_level: RiskLevel::Low,
        };
    }

    // Check for critical risks that should be denied
    let critical_risks: Vec<SecurityRisk> = analysis
        .risks
        .iter()
        .filter(|r| r.level == RiskLevel::Critical)
        .cloned()
        .collect();

    if !critical_risks.is_empty() {
        let reasons: Vec<String> = critical_risks.iter().map(|r| r.message.clone()).collect();
        return SafetyResult::Denied {
            reason: reasons.join("; "),
            risks: critical_risks,
        };
    }

    // Deny-phase risks → always denied (injection vectors)
    if analysis.is_auto_denied() {
        let deny_risks: Vec<SecurityRisk> = analysis
            .risks
            .into_iter()
            .filter(|r| r.phase == RiskPhase::Deny)
            .collect();
        let reasons: Vec<String> = deny_risks.iter().map(|r| r.message.clone()).collect();
        return SafetyResult::Denied {
            reason: reasons.join("; "),
            risks: deny_risks,
        };
    }

    // Check if approval is required (Ask phase risks)
    if analysis.requires_approval() {
        return SafetyResult::RequiresApproval {
            risks: analysis.risks,
            max_level: analysis.max_level.unwrap_or(RiskLevel::Low),
        };
    }

    // Remaining risks without Deny or Ask phase classification
    if cmd.try_extract_safe_commands().is_some() {
        SafetyResult::Safe {
            via_whitelist: false,
        }
    } else {
        SafetyResult::RequiresApproval {
            risks: analysis.risks,
            max_level: analysis.max_level.unwrap_or(RiskLevel::Low),
        }
    }
}

/// Checks if a command is a simple read-only command (fast path).
///
/// This is the original whitelist-based check that's very fast but limited.
/// A command is considered simple read-only if:
/// 1. Its first word is in the safe command whitelist
/// 2. It does not contain shell operators (&&, ||, ;, |, >, <)
fn is_simple_read_only(command: &str) -> bool {
    // Strip trailing stderr redirect before analysis (matches CC's isReadOnlyCommand)
    let trimmed = command
        .trim()
        .strip_suffix("2>&1")
        .unwrap_or(command.trim())
        .trim();
    if trimmed.is_empty() {
        return false;
    }

    // Reject commands containing unquoted glob patterns — can't safely validate.
    // Skip this check if globs are inside quotes (single or double).
    if contains_unquoted_glob(trimmed) {
        return false;
    }

    // Reject commands containing unsafe shell operators
    for op in UNSAFE_OPERATORS {
        if trimmed.contains(op) {
            return false;
        }
    }

    // Extract the first word (the command name)
    let first_word = match trimmed.split_whitespace().next() {
        Some(word) => word,
        None => return false,
    };

    // Version check patterns — always safe regardless of the command being in the whitelist
    if is_version_check(first_word, trimmed) {
        return true;
    }

    // Check if it is a known safe command
    if !READ_ONLY_COMMANDS.contains(&first_word) {
        return false;
    }

    // For git commands, additionally verify the subcommand
    if first_word == "git" {
        return is_git_read_only_internal(trimmed);
    }

    // For docker commands, verify the subcommand
    if first_word == "docker" {
        return is_docker_read_only_internal(trimmed);
    }

    true
}

/// Check if the command contains unquoted glob characters (`*`, `?`).
/// Globs inside single or double quotes are not expanded by the shell.
fn contains_unquoted_glob(command: &str) -> bool {
    let mut in_single = false;
    let mut in_double = false;
    let mut chars = command.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '\\' => {
                chars.next();
            } // skip escaped char
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            '*' | '?' if !in_single && !in_double => return true,
            _ => {}
        }
    }
    false
}

/// Check if the command is a version/help check (always safe).
fn is_version_check(first_word: &str, command: &str) -> bool {
    let version_commands = [
        "node", "npm", "npx", "python", "python3", "ruby", "go", "rustc", "cargo", "java", "javac",
        "dotnet", "php", "perl", "swift", "kotlin",
    ];
    if !version_commands.contains(&first_word) {
        return false;
    }
    let mut words = command.split_whitespace();
    words.next(); // skip command name
    matches!(words.next(), Some("-v" | "-V" | "--version")) && words.next().is_none()
}

/// Returns true if the command is a known read-only command.
///
/// A command is considered read-only if:
/// 1. Its first word is in the safe command list
/// 2. It does not contain shell operators (&&, ||, ;, |, >, <)
///
/// For `git` commands, further checks are applied via [`is_git_read_only`].
///
/// **Note**: This is the fast-path check only. For comprehensive security
/// analysis, use [`analyze_command_safety`] instead.
pub fn is_read_only_command(command: &str) -> bool {
    is_simple_read_only(command)
}

/// Internal helper to check git read-only status.
fn is_git_read_only_internal(command: &str) -> bool {
    let trimmed = command.trim();
    let mut words = trimmed.split_whitespace();

    // Skip "git"
    match words.next() {
        Some("git") => {}
        _ => return false,
    }

    // Reject dangerous git flags that enable code execution
    let remaining: Vec<&str> = words.collect();
    for flag in GIT_DANGEROUS_FLAGS {
        let flag_eq = format!("{flag}=");
        if remaining
            .iter()
            .any(|w| *w == *flag || w.starts_with(&flag_eq))
        {
            return false;
        }
    }

    let subcommand = match remaining.first() {
        Some(s) => *s,
        None => return false,
    };

    // Check single-word subcommands
    if GIT_READ_ONLY_SUBCOMMANDS.contains(&subcommand) {
        return true;
    }

    // Check two-word subcommands (e.g. "stash list", "worktree list")
    if let Some(second) = remaining.get(1) {
        for (first, expected_second) in GIT_READ_ONLY_TWO_WORD_SUBCOMMANDS {
            if subcommand == *first && *second == *expected_second {
                return true;
            }
        }
    }

    false
}

/// Internal helper to check docker read-only status.
fn is_docker_read_only_internal(command: &str) -> bool {
    let trimmed = command.trim();
    let mut words = trimmed.split_whitespace();

    // Skip "docker"
    match words.next() {
        Some("docker") => {}
        _ => return false,
    }

    let subcommand = match words.next() {
        Some(s) => s,
        None => return false,
    };

    // Check single-word subcommands
    if DOCKER_READ_ONLY_SUBCOMMANDS.contains(&subcommand) {
        return true;
    }

    // Check two-word subcommands (e.g. "compose ps")
    if let Some(second) = words.next() {
        for (first, expected_second) in DOCKER_READ_ONLY_TWO_WORD_SUBCOMMANDS {
            if subcommand == *first && second == *expected_second {
                return true;
            }
        }
    }

    false
}

/// Returns true if the git command is a read-only subcommand.
///
/// Checks the second word of the command against the known read-only
/// git subcommands (status, log, diff, show, branch, tag, remote).
pub fn is_git_read_only(command: &str) -> bool {
    is_git_read_only_internal(command)
}

/// Returns safety analysis summary for a command.
///
/// This provides a quick overview of the command's safety status
/// suitable for logging or display.
pub fn safety_summary(command: &str) -> String {
    let result = analyze_command_safety(command);
    match result {
        SafetyResult::Safe { via_whitelist } => {
            if via_whitelist {
                "Safe (whitelist)".to_string()
            } else {
                "Safe (analyzed)".to_string()
            }
        }
        SafetyResult::RequiresApproval { risks, max_level } => {
            format!(
                "Requires approval: {} risk(s), max level: {}",
                risks.len(),
                max_level
            )
        }
        SafetyResult::Denied { reason, .. } => {
            format!("Denied: {reason}")
        }
    }
}

/// Returns detailed risk information for a command.
///
/// This extracts all security risks detected in a command, suitable
/// for detailed reporting.
pub fn get_command_risks(command: &str) -> Vec<SecurityRisk> {
    let mut parser = ShellParser::new();
    let cmd = parser.parse(command);
    let analysis = cocode_shell_parser::security::analyze(&cmd);
    analysis.risks
}

/// Filters risks by phase.
pub fn filter_risks_by_phase(risks: &[SecurityRisk], phase: RiskPhase) -> Vec<&SecurityRisk> {
    risks.iter().filter(|r| r.phase == phase).collect()
}

/// Filters risks by minimum level.
pub fn filter_risks_by_level(risks: &[SecurityRisk], min_level: RiskLevel) -> Vec<&SecurityRisk> {
    risks.iter().filter(|r| r.level >= min_level).collect()
}

#[cfg(test)]
#[path = "readonly.test.rs"]
mod tests;
