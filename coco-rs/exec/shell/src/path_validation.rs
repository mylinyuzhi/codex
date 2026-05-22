//! Path validation for shell commands.
//!
//! TS: tools/BashTool/pathValidation.ts (2049 LOC)
//! Checks file paths in commands for dangerous targets (/, /etc, /usr, etc.)

/// Dangerous filesystem paths that always require approval for rm/rmdir.
const DANGEROUS_REMOVAL_PATHS: &[&str] = &[
    "/",
    "/etc",
    "/usr",
    "/lib",
    "/lib64",
    "/bin",
    "/sbin",
    "/boot",
    "/dev",
    "/proc",
    "/sys",
    "/var",
    "/tmp",
    "/root",
    "/home",
    "~",
    "~/.claude",
    "~/.coco",
];

/// Check if a file path is dangerous for a destructive operation.
///
/// Returns Some(reason) if the path is dangerous, None if safe.
pub fn check_dangerous_path(_command: &str, path: &str, cwd: &str) -> Option<String> {
    let resolved = resolve_path(path, cwd);

    for &dangerous in DANGEROUS_REMOVAL_PATHS {
        let expanded = expand_home(dangerous);
        // Only flag if the resolved path IS the dangerous directory itself
        // (not a deep descendant within it, which is usually safe)
        if resolved == expanded {
            return Some(format!("dangerous target path: {path}"));
        }
        // Flag direct children of root-level dangerous paths (e.g., /usr/lib)
        if dangerous == "/" && resolved.matches('/').count() == 1 {
            return Some(format!("dangerous target path: {path}"));
        }
    }

    None
}

/// Extract file paths from command arguments based on command type.
///
/// TS: PATH_EXTRACTORS — per-command path extraction logic.
pub fn extract_paths_from_command(command_name: &str, args: &[&str]) -> Vec<String> {
    match command_name {
        "cd" => {
            if args.is_empty() {
                vec!["~".to_string()]
            } else {
                vec![args.join(" ")]
            }
        }
        "ls" | "mkdir" | "touch" | "rm" | "rmdir" | "cat" | "head" | "tail" | "wc" | "file"
        | "stat" | "diff" | "strings" | "hexdump" | "nl" => filter_flags(args),
        "find" => extract_find_paths(args),
        "grep" | "rg" => extract_pattern_command_paths(args),
        "mv" | "cp" => filter_flags(args),
        "sed" => extract_sed_paths(args),
        "git" => Vec::new(), // Git manages its own paths
        _ => Vec::new(),
    }
}

/// Validate all output redirections in a command.
///
/// Returns Some(reason) if a redirection is to a dangerous path.
pub fn check_redirect_paths(redirects: &[(String, String)], cwd: &str) -> Option<String> {
    for (operator, target) in redirects {
        if matches!(operator.as_str(), ">" | ">>" | "&>")
            && let Some(reason) = check_dangerous_path("redirect", target, cwd)
        {
            return Some(reason);
        }
    }
    None
}

/// Filter out flags from args, returning only positional arguments.
/// Handles the `--` end-of-options delimiter.
fn filter_flags(args: &[&str]) -> Vec<String> {
    let mut result = Vec::new();
    let mut after_double_dash = false;

    for &arg in args {
        if arg == "--" {
            after_double_dash = true;
            continue;
        }
        if after_double_dash || !arg.starts_with('-') {
            result.push(arg.to_string());
        }
    }
    result
}

/// Extract paths from `find` command arguments.
fn extract_find_paths(args: &[&str]) -> Vec<String> {
    let mut paths = Vec::new();
    for &arg in args {
        if arg.starts_with('-') || arg.starts_with('(') || arg.starts_with('!') {
            break;
        }
        paths.push(arg.to_string());
    }
    if paths.is_empty() {
        paths.push(".".to_string());
    }
    paths
}

/// Extract paths from pattern commands (grep, rg) — skip the pattern.
fn extract_pattern_command_paths(args: &[&str]) -> Vec<String> {
    if args.len() <= 1 {
        return Vec::new();
    }
    // First non-flag arg is the pattern, rest are paths
    let mut found_pattern = false;
    let mut paths = Vec::new();
    for &arg in args {
        if arg.starts_with('-') {
            continue;
        }
        if !found_pattern {
            found_pattern = true;
            continue;
        }
        paths.push(arg.to_string());
    }
    paths
}

/// Extract file paths from sed command args.
fn extract_sed_paths(args: &[&str]) -> Vec<String> {
    let mut paths = Vec::new();
    let mut skip_next = false;
    let mut found_expression = false;

    for &arg in args {
        if skip_next {
            skip_next = false;
            continue;
        }
        if matches!(arg, "-e" | "--expression" | "-f" | "--file") {
            skip_next = true;
            continue;
        }
        if arg.starts_with('-') {
            continue;
        }
        if !found_expression {
            found_expression = true; // First positional is the expression
            continue;
        }
        paths.push(arg.to_string());
    }
    paths
}

/// Resolve a relative path against the working directory.
fn resolve_path(path: &str, cwd: &str) -> String {
    let expanded = expand_home(path);
    if expanded.starts_with('/') {
        expanded
    } else {
        format!("{cwd}/{expanded}")
    }
}

/// Expand ~ to home directory.
fn expand_home(path: &str) -> String {
    if path.starts_with("~/") || path == "~" {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
        path.replacen('~', &home, 1)
    } else {
        path.to_string()
    }
}

#[cfg(test)]
#[path = "path_validation.test.rs"]
mod tests;
