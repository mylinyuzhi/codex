//! Shared argument substitution for skills.
//!
//! Provides argument parsing and prompt variable substitution used by both
//! user-invoked skills ([`execute_skill`](crate::manager::execute_skill)) and
//! LLM-invoked skills (`SkillTool`).

use std::path::Path;

use crate::interface::ArgumentDef;

/// Parse an argument string into individual arguments, respecting quoted strings
/// and backslash escapes.
///
/// # Escaping Rules
///
/// - Outside quotes: `\` escapes the next character
/// - In double quotes: `\` escapes the next character (e.g., `\"` → `"`)
/// - In single quotes: all characters are literal (no escape processing)
///
/// # Examples
///
/// ```
/// use cocode_skill::parse_skill_args;
///
/// assert_eq!(parse_skill_args("foo bar"), vec!["foo", "bar"]);
/// assert_eq!(parse_skill_args(r#"say \"hello\""#), vec!["say", r#""hello""#]);
/// ```
pub fn parse_skill_args(args: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut current = String::new();
    let mut in_double_quote = false;
    let mut in_single_quote = false;
    let mut chars = args.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            '\\' if !in_single_quote => {
                // Consume next character literally
                if let Some(next) = chars.next() {
                    current.push(next);
                }
            }
            '"' if !in_single_quote => {
                in_double_quote = !in_double_quote;
            }
            '\'' if !in_double_quote => {
                in_single_quote = !in_single_quote;
            }
            c if c.is_whitespace() && !in_double_quote && !in_single_quote => {
                if !current.is_empty() {
                    result.push(std::mem::take(&mut current));
                }
            }
            _ => {
                current.push(c);
            }
        }
    }
    if !current.is_empty() {
        result.push(current);
    }
    result
}

/// Substitute skill arguments into a prompt template.
///
/// Handles the following substitutions (in order):
/// 1. `$ARGUMENTS` → raw argument string
/// 2. `$1`, `$2`, etc. → positional arguments (from `arg_defs` order)
/// 3. `${args.name}` → named arguments (matched by `arg_defs[i].name`)
/// 4. `${COCODE_SKILL_DIR}` → skill base directory path
/// 5. Prepend `Base directory for this skill: {base_dir}` if provided
///
/// If the prompt contains no `$ARGUMENTS` placeholder and args are non-empty,
/// appends `\n\nArguments: {args}` to the prompt.
pub fn substitute_skill_args(
    prompt: &str,
    args: &str,
    arg_defs: Option<&[ArgumentDef]>,
    base_dir: Option<&Path>,
) -> String {
    // Step 1: $ARGUMENTS substitution
    // If the skill defines structured arguments (arg_defs), they are consumed
    // by positional/named substitution in step 2 — don't also append raw args.
    let mut result = if prompt.contains("$ARGUMENTS") {
        prompt.replace("$ARGUMENTS", args)
    } else if args.is_empty() {
        prompt.to_string()
    } else if arg_defs.is_some_and(|d| !d.is_empty()) {
        // Args will be consumed by positional/named substitution below
        prompt.to_string()
    } else {
        format!("{prompt}\n\nArguments: {args}")
    };

    // Step 2: Named and positional argument substitution
    if let Some(defs) = arg_defs {
        let parsed = parse_skill_args(args);
        for (i, def) in defs.iter().enumerate() {
            let value = parsed.get(i).map(String::as_str).unwrap_or("");
            // Positional: $1, $2, etc.
            result = result.replace(&format!("${}", i + 1), value);
            // Named: ${args.name}
            result = result.replace(&format!("${{args.{}}}", def.name), value);
        }
    }

    // Step 3: ${COCODE_SKILL_DIR} substitution
    if let Some(dir) = base_dir {
        result = result.replace("${COCODE_SKILL_DIR}", &dir.display().to_string());
    }

    // Step 4: Base directory prefix
    if let Some(dir) = base_dir {
        result = format!(
            "Base directory for this skill: {}\n\n{result}",
            dir.display()
        );
    }

    result
}

#[cfg(test)]
#[path = "substitution.test.rs"]
mod tests;
