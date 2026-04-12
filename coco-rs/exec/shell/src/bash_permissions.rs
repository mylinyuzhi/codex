//! Bash command permission checking pipeline.
//!
//! TS: tools/BashTool/bashPermissions.ts (97K)
//!
//! Implements wrapper stripping, env var filtering, command prefix extraction,
//! and compound command splitting for the permission rule matching system.

use std::collections::HashSet;
use std::sync::LazyLock;

/// Safe environment variables — no code execution or library loading risk.
///
/// TS: SAFE_ENV_VARS constant (~40 entries).
static SAFE_ENV_VARS: LazyLock<HashSet<&str>> = LazyLock::new(|| {
    [
        // Go
        "GOEXPERIMENT",
        "GOOS",
        "GOARCH",
        "CGO_ENABLED",
        "GO111MODULE",
        // Rust
        "RUST_BACKTRACE",
        "RUST_LOG",
        "CARGO_TERM_COLOR",
        // Node
        "NODE_ENV",
        "NODE_OPTIONS",
        // Python
        "PYTHONUNBUFFERED",
        "PYTHONDONTWRITEBYTECODE",
        "VIRTUAL_ENV",
        // Pytest, locale, terminal
        "PYTEST_DISABLE_PLUGIN_AUTOLOAD",
        "LANG",
        "LANGUAGE",
        "LC_ALL",
        "TERM",
        // Colors
        "NO_COLOR",
        "FORCE_COLOR",
        "LS_COLORS",
        "LSCOLORS",
        "GREP_COLORS",
        // Other
        "TZ",
        "CHARSET",
        "COLUMNS",
        "LINES",
        "ANTHROPIC_API_KEY",
        "OPENAI_API_KEY",
    ]
    .into_iter()
    .collect()
});

/// Binary hijack variables — env vars that can redirect binary execution.
const BINARY_HIJACK_PATTERNS: &[&str] = &["LD_", "DYLD_", "PATH"];

/// Dangerous bare shell prefixes — never allow bare-prefix rules for these.
static BARE_SHELL_PREFIXES: LazyLock<HashSet<&str>> = LazyLock::new(|| {
    [
        "sh",
        "bash",
        "zsh",
        "fish",
        "csh",
        "tcsh",
        "ksh",
        "dash",
        "cmd",
        "powershell",
        "pwsh",
        "env",
        "xargs",
        "nice",
        "stdbuf",
        "nohup",
        "timeout",
        "time",
        "sudo",
        "doas",
        "pkexec",
    ]
    .into_iter()
    .collect()
});

/// Strip safe wrapper commands from a command string.
///
/// TS: stripSafeWrappers() — two-phase stripping:
/// Phase 1: Strip leading safe env vars + comments
/// Phase 2: Strip wrapper commands (timeout, time, nice, stdbuf, nohup)
///
/// Returns the inner command after stripping.
pub fn strip_safe_wrappers(command: &str) -> String {
    let mut result = command.trim().to_string();

    // Phase 1: Strip leading safe env vars
    loop {
        let before = result.clone();
        result = strip_one_safe_env_var(&result);
        if result == before {
            break;
        }
    }

    // Phase 2: Strip wrapper commands
    loop {
        let before = result.clone();
        result = strip_one_wrapper(&result);
        if result == before {
            break;
        }
    }

    result.trim().to_string()
}

/// Strip all leading env var assignments (safe or not).
///
/// TS: stripAllLeadingEnvVars() — used for deny rule matching.
/// Optionally checks against a blocklist of hijack variables.
pub fn strip_all_env_vars(command: &str, check_hijack: bool) -> String {
    let mut rest = command.trim();

    loop {
        // Match VAR=value pattern
        if let Some((var_name, after)) = try_parse_env_assignment(rest) {
            if check_hijack && is_binary_hijack_var(var_name) {
                break;
            }
            rest = after.trim_start();
        } else {
            break;
        }
    }

    rest.to_string()
}

/// Extract a 2-word command prefix (e.g., "git commit" from "git commit -m msg").
///
/// TS: getSimpleCommandPrefix() — returns null if unsafe env vars found.
pub fn get_command_prefix(command: &str) -> Option<String> {
    let stripped = strip_safe_wrappers(command);
    let mut words = stripped.split_whitespace();

    let first = words.next()?;
    let base = first.rsplit('/').next().unwrap_or(first);

    // Don't allow bare shell prefixes as command prefix
    if BARE_SHELL_PREFIXES.contains(base) {
        return None;
    }

    if let Some(second) = words.next() {
        // Skip if second word looks like a flag
        if second.starts_with('-') {
            return Some(base.to_string());
        }
        Some(format!("{base} {second}"))
    } else {
        Some(base.to_string())
    }
}

/// Split a compound command into subcommands.
///
/// Splits on &&, ||, ;, | operators (basic — does not handle quoting fully).
pub fn split_compound_command(command: &str) -> Vec<String> {
    let mut subcommands = Vec::new();
    let mut current = String::new();
    let mut chars = command.chars().peekable();
    let mut in_single_quote = false;
    let mut in_double_quote = false;

    while let Some(c) = chars.next() {
        match c {
            '\'' if !in_double_quote => {
                in_single_quote = !in_single_quote;
                current.push(c);
            }
            '"' if !in_single_quote => {
                in_double_quote = !in_double_quote;
                current.push(c);
            }
            '&' if !in_single_quote && !in_double_quote => {
                if chars.peek() == Some(&'&') {
                    chars.next();
                    let trimmed = current.trim().to_string();
                    if !trimmed.is_empty() {
                        subcommands.push(trimmed);
                    }
                    current.clear();
                } else {
                    current.push(c);
                }
            }
            '|' if !in_single_quote && !in_double_quote => {
                if chars.peek() == Some(&'|') {
                    chars.next();
                }
                let trimmed = current.trim().to_string();
                if !trimmed.is_empty() {
                    subcommands.push(trimmed);
                }
                current.clear();
            }
            ';' if !in_single_quote && !in_double_quote => {
                let trimmed = current.trim().to_string();
                if !trimmed.is_empty() {
                    subcommands.push(trimmed);
                }
                current.clear();
            }
            _ => current.push(c),
        }
    }

    let trimmed = current.trim().to_string();
    if !trimmed.is_empty() {
        subcommands.push(trimmed);
    }

    subcommands
}

/// Check if a command prefix is a dangerous bare shell prefix.
pub fn is_dangerous_bare_prefix(prefix: &str) -> bool {
    BARE_SHELL_PREFIXES.contains(prefix)
}

/// Check if an env var name is a binary hijack variable.
fn is_binary_hijack_var(name: &str) -> bool {
    BINARY_HIJACK_PATTERNS.iter().any(|&pat| {
        if pat.ends_with('_') {
            name.starts_with(pat)
        } else {
            name == pat
        }
    })
}

/// Try to strip one safe env var assignment from the front of the command.
fn strip_one_safe_env_var(command: &str) -> String {
    let trimmed = command.trim_start();
    if let Some((var_name, after)) = try_parse_env_assignment(trimmed) {
        if SAFE_ENV_VARS.contains(var_name) {
            return after.trim_start().to_string();
        }
    }
    command.to_string()
}

/// Try to strip one wrapper command from the front.
fn strip_one_wrapper(command: &str) -> String {
    let trimmed = command.trim_start();

    let wrappers: &[(&str, fn(&str) -> Option<&str>)] = &[
        ("timeout ", strip_timeout_args),
        ("time ", strip_simple_wrapper),
        ("nice ", strip_nice_args),
        ("stdbuf ", strip_stdbuf_args),
        ("nohup ", strip_simple_wrapper),
    ];

    for &(prefix, stripper) in wrappers {
        if trimmed.starts_with(prefix) {
            if let Some(rest) = stripper(&trimmed[prefix.len()..]) {
                return rest.to_string();
            }
        }
    }

    command.to_string()
}

/// Try to parse "VAR=value " at the start, returning (var_name, rest).
fn try_parse_env_assignment(s: &str) -> Option<(&str, &str)> {
    let eq_pos = s.find('=')?;
    let name = &s[..eq_pos];

    // Name must be valid identifier
    if name.is_empty()
        || !name.chars().next().unwrap_or('0').is_ascii_alphabetic()
        || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        return None;
    }

    // Find end of value (next unquoted whitespace)
    let rest = &s[eq_pos + 1..];
    let value_end = find_value_end(rest);
    let after = &rest[value_end..];

    if after.is_empty() || after.starts_with(char::is_whitespace) {
        Some((name, after.trim_start()))
    } else {
        None
    }
}

fn find_value_end(s: &str) -> usize {
    let mut i = 0;
    let bytes = s.as_bytes();

    if i < bytes.len() && bytes[i] == b'\'' {
        // Single-quoted value
        i += 1;
        while i < bytes.len() && bytes[i] != b'\'' {
            i += 1;
        }
        if i < bytes.len() {
            i += 1;
        }
    } else if i < bytes.len() && bytes[i] == b'"' {
        // Double-quoted value
        i += 1;
        while i < bytes.len() && bytes[i] != b'"' {
            if bytes[i] == b'\\' {
                i += 1;
            }
            i += 1;
        }
        if i < bytes.len() {
            i += 1;
        }
    } else {
        // Unquoted value
        while i < bytes.len() && !bytes[i].is_ascii_whitespace() {
            i += 1;
        }
    }
    i
}

fn strip_timeout_args(rest: &str) -> Option<&str> {
    // Skip timeout flags until we hit the actual command
    let mut remaining = rest.trim_start();
    // Skip optional flags like --signal=KILL, -k 5, etc.
    while remaining.starts_with('-') || remaining.starts_with("--") {
        let next_space = remaining.find(' ')?;
        remaining = remaining[next_space..].trim_start();
        // If the flag takes an argument (e.g., -k 5), skip that too
        if !remaining.starts_with('-')
            && remaining
                .chars()
                .next()
                .map_or(false, |c| c.is_ascii_digit())
        {
            if let Some(ns) = remaining.find(' ') {
                remaining = remaining[ns..].trim_start();
            }
        }
    }
    // Skip the duration argument
    if let Some(space) = remaining.find(' ') {
        Some(remaining[space..].trim_start())
    } else {
        None
    }
}

fn strip_simple_wrapper(rest: &str) -> Option<&str> {
    // Just skip "-- " if present, then return the rest
    let trimmed = rest.trim_start();
    if let Some(stripped) = trimmed.strip_prefix("-- ") {
        Some(stripped)
    } else {
        Some(trimmed)
    }
}

fn strip_nice_args(rest: &str) -> Option<&str> {
    let mut remaining = rest.trim_start();
    // Skip -n <priority> or -<digit>
    if remaining.starts_with("-n") {
        remaining = remaining[2..].trim_start();
        if let Some(space) = remaining.find(' ') {
            remaining = remaining[space..].trim_start();
        }
    } else if remaining.starts_with('-')
        && remaining
            .as_bytes()
            .get(1)
            .map_or(false, u8::is_ascii_digit)
    {
        if let Some(space) = remaining.find(' ') {
            remaining = remaining[space..].trim_start();
        }
    }
    if let Some(stripped) = remaining.strip_prefix("-- ") {
        Some(stripped)
    } else {
        Some(remaining)
    }
}

fn strip_stdbuf_args(rest: &str) -> Option<&str> {
    let mut remaining = rest.trim_start();
    // Skip -i/-o/-e flags with values
    while remaining.starts_with('-') && remaining.len() > 1 {
        let flag_char = remaining.as_bytes()[1];
        if matches!(flag_char, b'i' | b'o' | b'e') {
            // Skip flag + value (e.g., -iL or -i0)
            let end = remaining[2..]
                .find(' ')
                .map(|p| p + 2)
                .unwrap_or(remaining.len());
            remaining = remaining[end..].trim_start();
        } else {
            break;
        }
    }
    if let Some(stripped) = remaining.strip_prefix("-- ") {
        Some(stripped)
    } else {
        Some(remaining)
    }
}

#[cfg(test)]
#[path = "bash_permissions.test.rs"]
mod tests;
