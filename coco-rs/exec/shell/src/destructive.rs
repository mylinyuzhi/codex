//! Destructive command warning patterns.
//!
//! TS: destructiveCommandWarning.ts — ~20 patterns that trigger warnings.
//! These patterns are checked before tool execution to warn the user.

/// Check if a command is destructive and return a warning message.
pub fn get_destructive_warning(command: &str) -> Option<String> {
    let trimmed = command.trim();

    for &(pattern, warning) in DESTRUCTIVE_PATTERNS {
        if matches_pattern(trimmed, pattern) {
            return Some(warning.to_string());
        }
    }

    // Check case-insensitive SQL patterns
    let upper = trimmed.to_uppercase();
    for &(pattern, warning) in SQL_PATTERNS {
        if upper.contains(pattern) {
            return Some(warning.to_string());
        }
    }

    None
}

/// Destructive command patterns and their warnings.
/// Matched via substring contains (case-sensitive).
const DESTRUCTIVE_PATTERNS: &[(&str, &str)] = &[
    // Filesystem destruction
    ("rm -rf /", "This will delete the entire filesystem"),
    ("rm -rf ~", "This will delete your home directory"),
    (
        "rm -rf .",
        "This will delete the current directory and all contents",
    ),
    (
        "rm -rf *",
        "This will delete all files in the current directory",
    ),
    (":(){:|:&};:", "This is a fork bomb"),
    ("mkfs", "This will format a filesystem"),
    ("dd if=", "dd can overwrite disk data irreversibly"),
    ("> /dev/sd", "This will overwrite a disk device"),
    ("chmod -R 777 /", "This will make all files world-writable"),
    (
        "chown -R",
        "Recursive ownership change can break system files",
    ),
    // Git destructive operations
    (
        "git push --force",
        "Force pushing can overwrite remote history and lose commits",
    ),
    (
        "git push -f",
        "Force pushing can overwrite remote history and lose commits",
    ),
    (
        "git reset --hard",
        "This will discard all uncommitted changes permanently",
    ),
    (
        "git clean -f",
        "This will permanently delete untracked files",
    ),
    (
        "git clean -fd",
        "This will permanently delete untracked files and directories",
    ),
    (
        "git checkout -- .",
        "This will discard all unstaged changes in the working tree",
    ),
    (
        "git restore .",
        "This will discard all unstaged changes in the working tree",
    ),
    ("--no-verify", "This bypasses pre-commit and pre-push hooks"),
    // Infrastructure / container destruction
    (
        "kubectl delete",
        "This will delete Kubernetes resources (potentially in production)",
    ),
    (
        "terraform destroy",
        "This will destroy infrastructure resources",
    ),
    ("docker rm", "This will remove Docker containers"),
    ("docker rmi", "This will remove Docker images"),
    (
        "docker system prune",
        "This will remove all unused Docker data",
    ),
    // System commands
    ("shutdown", "This will shut down the system"),
    ("reboot", "This will reboot the system"),
    ("systemctl stop", "This will stop a system service"),
    ("kill -9", "This will forcefully kill a process"),
    ("killall", "This will kill processes by name"),
    ("pkill", "This will signal processes by pattern"),
];

/// SQL destructive patterns (checked case-insensitively).
const SQL_PATTERNS: &[(&str, &str)] = &[
    (
        "DROP TABLE",
        "This will permanently delete a database table",
    ),
    (
        "DROP DATABASE",
        "This will permanently delete an entire database",
    ),
    (
        "TRUNCATE TABLE",
        "This will delete all rows in a table irreversibly",
    ),
    ("DELETE FROM", "This will delete rows from a database table"),
    (
        "ALTER TABLE",
        "This will modify the structure of a database table",
    ),
];

fn matches_pattern(command: &str, pattern: &str) -> bool {
    command.contains(pattern)
}

#[cfg(test)]
#[path = "destructive.test.rs"]
mod tests;
