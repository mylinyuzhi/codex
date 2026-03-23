//! Command normalization for policy rule matching.
//!
//! Strips environment variable prefixes and wrapper commands from command
//! strings so that `"LANG=C git push"` matches a `"git *"` rule.
//!
//! Aligned with the JS original's `Ac()` (stripWrapperCommands) and `bn8()`
//! (stripEnvVars) in chunks.172.mjs.

/// Wrapper commands that are transparent for permission purposes.
///
/// These commands execute another command and don't change its
/// observable security profile for permission decisions.
const WRAPPERS: &[&str] = &["timeout", "nice", "nohup", "time", "env", "command"];

/// Normalize a command by stripping env var prefixes and wrapper commands.
///
/// Iteratively removes leading `KEY=VALUE` pairs and transparent wrapper
/// commands until the core command is exposed.
///
/// ```text
/// "LANG=C git push"            → "git push"
/// "timeout 30 git push"        → "git push"
/// "nice -n 10 npm run test"    → "npm run test"
/// "env FOO=bar git push"       → "git push"
/// "LANG=C timeout 5 git push"  → "git push"
/// ```
pub fn normalize_command(command: &str) -> &str {
    let mut result = command.trim();
    loop {
        let prev = result;
        result = strip_env_vars(result);
        result = strip_wrapper(result);
        if result == prev {
            break;
        }
    }
    result
}

/// Strip leading `KEY=VALUE` pairs from a command string.
///
/// Matches patterns like `FOO=bar`, `LANG=C`, `CC=/usr/bin/gcc`.
/// Stops at the first token that isn't a `KEY=VALUE` pair.
fn strip_env_vars(command: &str) -> &str {
    let mut rest = command;
    loop {
        let trimmed = rest.trim_start();
        if let Some(after) = try_strip_env_var(trimmed) {
            rest = after;
        } else {
            return trimmed;
        }
    }
}

/// Try to strip a single leading `KEY=VALUE` token.
///
/// Returns `Some(rest)` if a `KEY=VALUE` was found and stripped.
fn try_strip_env_var(s: &str) -> Option<&str> {
    let bytes = s.as_bytes();
    if bytes.is_empty() {
        return None;
    }
    // First char must be letter or underscore.
    let first = bytes[0];
    if !first.is_ascii_alphabetic() && first != b'_' {
        return None;
    }
    // Find the '=' sign.
    let eq_pos = bytes.iter().position(|&b| b == b'=')?;
    // Everything before '=' must be [A-Za-z0-9_].
    if !bytes[1..eq_pos]
        .iter()
        .all(|b| b.is_ascii_alphanumeric() || *b == b'_')
    {
        return None;
    }
    // Skip the value (non-whitespace after '=').
    let after_eq = &s[eq_pos + 1..];
    let value_end = after_eq
        .find(|c: char| c.is_ascii_whitespace())
        .unwrap_or(after_eq.len());
    let rest = &after_eq[value_end..];
    if rest.is_empty() {
        // Only env var, no command after — don't strip.
        return None;
    }
    Some(rest)
}

/// Strip a leading wrapper command with its arguments.
///
/// Handles: `timeout [-k N] N cmd`, `nice [-n N] cmd`, `nohup cmd`,
/// `time [-p] cmd`, `env [-u VAR] [KEY=VAL...] cmd`, `command [-pvV] cmd`.
fn strip_wrapper(command: &str) -> &str {
    let trimmed = command.trim_start();
    let (first_word, rest) = split_first_word(trimmed);

    if !WRAPPERS.contains(&first_word) {
        return command;
    }

    match first_word {
        "nohup" => rest.trim_start(),
        "timeout" => skip_timeout_args(rest),
        "nice" => skip_nice_args(rest),
        "time" => skip_time_args(rest),
        "env" => skip_env_args(rest),
        "command" => skip_flag_args(rest),
        _ => command,
    }
}

/// Split a string into the first whitespace-delimited word and the rest.
fn split_first_word(s: &str) -> (&str, &str) {
    match s.find(|c: char| c.is_ascii_whitespace()) {
        Some(i) => (&s[..i], &s[i..]),
        None => (s, ""),
    }
}

/// Skip `timeout` arguments: `[-k DURATION] DURATION`.
fn skip_timeout_args(rest: &str) -> &str {
    let mut r = rest.trim_start();
    // Skip optional -k flag + its argument
    if r.starts_with("-k") {
        let (_, after_k) = split_first_word(r);
        r = after_k.trim_start();
        // Skip the kill-after duration
        let (_, after_dur) = split_first_word(r);
        r = after_dur.trim_start();
    }
    // Skip optional flags like --signal, --preserve-status
    while r.starts_with('-') {
        let (_, after_flag) = split_first_word(r);
        r = after_flag.trim_start();
    }
    // Skip the duration argument
    let (word, after) = split_first_word(r);
    if word.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        after.trim_start()
    } else {
        r
    }
}

/// Skip `nice` arguments: `[-n ADJUSTMENT]`.
fn skip_nice_args(rest: &str) -> &str {
    let r = rest.trim_start();
    if r.starts_with("-n") {
        let (_, after_n) = split_first_word(r);
        let trimmed = after_n.trim_start();
        let (_, after_adj) = split_first_word(trimmed);
        after_adj.trim_start()
    } else if r.starts_with('-') {
        // nice -10 cmd
        let (_, after) = split_first_word(r);
        after.trim_start()
    } else {
        r
    }
}

/// Skip `time` arguments: `[-p]`.
fn skip_time_args(rest: &str) -> &str {
    let r = rest.trim_start();
    if r.starts_with('-') {
        let (_, after) = split_first_word(r);
        after.trim_start()
    } else {
        r
    }
}

/// Skip `env` arguments: `[-u VAR] [KEY=VAL...]`.
fn skip_env_args(rest: &str) -> &str {
    let mut r = rest.trim_start();
    loop {
        if r.starts_with("-u") {
            let (_, after_u) = split_first_word(r);
            r = after_u.trim_start();
            // Skip the VAR argument
            let (_, after_var) = split_first_word(r);
            r = after_var.trim_start();
        } else if r.starts_with('-') {
            // Other flags like -i, -0
            let (_, after) = split_first_word(r);
            r = after.trim_start();
        } else if try_strip_env_var(r).is_some() {
            // Skip KEY=VAL pairs after env
            r = strip_env_vars(r);
        } else {
            break;
        }
    }
    r
}

/// Skip leading flag arguments (starting with `-`).
fn skip_flag_args(rest: &str) -> &str {
    let mut r = rest.trim_start();
    while r.starts_with('-') {
        let (_, after) = split_first_word(r);
        r = after.trim_start();
    }
    r
}

#[cfg(test)]
#[path = "normalize.test.rs"]
mod tests;
