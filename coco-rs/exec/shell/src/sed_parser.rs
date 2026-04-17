//! Sed in-place edit command parser.
//!
//! TS: tools/BashTool/sedEditParser.ts (9.4K LOC)
//! Parses `sed -i 's/pattern/replacement/flags' file` commands.

/// Parsed sed edit information.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SedEditInfo {
    /// The file being edited.
    pub file_path: String,
    /// Search pattern (regex).
    pub pattern: String,
    /// Replacement string.
    pub replacement: String,
    /// Substitution flags (g, i, etc.).
    pub flags: String,
    /// Whether -E or -r flag was used (extended regex).
    pub extended_regex: bool,
}

/// Check if a command is a sed in-place edit.
pub fn is_sed_in_place_edit(command: &str) -> bool {
    let trimmed = command.trim();
    trimmed.starts_with("sed ") && (trimmed.contains(" -i") || trimmed.contains(" --in-place"))
}

/// Parse a sed in-place edit command.
///
/// Returns `None` if the command is not a valid simple sed -i edit.
pub fn parse_sed_edit_command(command: &str) -> Option<SedEditInfo> {
    let trimmed = command.trim();
    if !trimmed.starts_with("sed ") {
        return None;
    }

    let args: Vec<String> = split_shell_args(&trimmed["sed ".len()..]);
    if args.is_empty() {
        return None;
    }

    let mut has_in_place = false;
    let mut extended_regex = false;
    let mut expression: Option<String> = None;
    let mut file_path: Option<String> = None;
    let mut i = 0;

    while i < args.len() {
        let arg = args[i].as_str();
        match arg {
            "-i" | "--in-place" => {
                has_in_place = true;
                if i + 1 < args.len() && args[i + 1].starts_with('.') {
                    i += 1; // skip backup suffix
                }
            }
            "-E" | "-r" => {
                extended_regex = true;
            }
            "-e" | "--expression" => {
                if i + 1 < args.len() {
                    i += 1;
                    expression = Some(args[i].clone());
                }
            }
            _ if arg.starts_with('-') => {
                // Unknown flag — skip
            }
            _ => {
                if expression.is_none() {
                    expression = Some(args[i].clone());
                } else if file_path.is_none() {
                    file_path = Some(args[i].clone());
                }
            }
        }
        i += 1;
    }

    if !has_in_place {
        return None;
    }

    let expr = expression?;
    let file = file_path?;

    // Parse the substitution expression: s/pattern/replacement/flags
    let sub = parse_substitution(&expr)?;

    Some(SedEditInfo {
        file_path: file,
        pattern: sub.0,
        replacement: sub.1,
        flags: sub.2,
        extended_regex,
    })
}

/// Parse a sed substitution expression: s/pattern/replacement/flags
fn parse_substitution(expr: &str) -> Option<(String, String, String)> {
    let bytes = expr.as_bytes();
    if bytes.is_empty() || bytes[0] != b's' {
        return None;
    }
    if bytes.len() < 2 {
        return None;
    }

    let delimiter = bytes[1] as char;
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut i = 2;

    while i < bytes.len() {
        if bytes[i] == b'\\' && i + 1 < bytes.len() {
            // Escaped character
            current.push(bytes[i] as char);
            current.push(bytes[i + 1] as char);
            i += 2;
        } else if bytes[i] as char == delimiter {
            parts.push(std::mem::take(&mut current));
            i += 1;
        } else {
            current.push(bytes[i] as char);
            i += 1;
        }
    }

    // Remaining text after last delimiter is flags
    if parts.len() < 2 {
        return None;
    }

    let pattern = parts[0].clone();
    let replacement = parts[1].clone();
    let flags = if !current.is_empty() {
        current
    } else if parts.len() > 2 {
        parts[2].clone()
    } else {
        String::new()
    };

    // Validate flags
    if !flags
        .chars()
        .all(|c| matches!(c, 'g' | 'p' | 'i' | 'I' | 'm' | 'M' | '1'..='9'))
    {
        return None;
    }

    Some((pattern, replacement, flags))
}

/// Shell argument splitting with basic quote stripping.
fn split_shell_args(command: &str) -> Vec<String> {
    command.split_whitespace().map(strip_quotes).collect()
}

/// Strip surrounding single or double quotes from an argument.
fn strip_quotes(s: &str) -> String {
    if s.len() >= 2
        && ((s.starts_with('\'') && s.ends_with('\'')) || (s.starts_with('"') && s.ends_with('"')))
    {
        s[1..s.len() - 1].to_string()
    } else {
        s.to_string()
    }
}

#[cfg(test)]
#[path = "sed_parser.test.rs"]
mod tests;
