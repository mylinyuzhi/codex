//! Lazy skill prompt rendering with TS-mirroring semantics.
//!
//! TS: `Command.getPromptForCommand(args, ctx) -> Promise<ContentBlockParam[]>`.
//!
//! Each invocation can:
//! - Substitute named arguments (`$1`, `$2`, …, `$ARGUMENTS`).
//! - Execute embedded `$(shell)` commands via `shell_exec`.
//! - Prepend `Base directory for this skill: <dir>` when bundled `files` are
//!   extracted (handled by [`render_with_extraction`] below).
//!
//! Mirrors `skills/loadSkillsDir.ts:substituteArguments` +
//! `utils/promptShellExecution.ts:executeShellCommandsInPrompt` +
//! `skills/bundledSkills.ts:prependBaseDir`.

use std::path::PathBuf;

use crate::SkillDefinition;
use crate::extraction;
use crate::shell_exec;

/// One block of rendered prompt content.
///
/// TS: `ContentBlockParam` from `@anthropic-ai/sdk`. We restrict to the two
/// shapes a skill prompt actually produces today (text + arbitrary doc),
/// mapped 1:1 to vercel-ai `TextPart` / `FilePart` at the message-build site.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PromptPart {
    Text { text: String },
    File { media_type: String, data: Vec<u8> },
}

/// Inputs available to the rendering pipeline.
#[derive(Debug, Clone, Default)]
pub struct RenderContext {
    /// Whether shell expansion is allowed in this context.
    /// TS: gated on `context.options.allowShellInPrompt` (defaults true).
    pub allow_shell: bool,
    /// Process environment passed through to shell commands.
    pub env: Vec<(String, String)>,
}

/// Render a skill's prompt for a given invocation.
///
/// Pipeline (matching TS order):
/// 1. **Argument substitution** — `$1..$N` and `$ARGUMENTS`.
/// 2. **Shell expansion** — only when `ctx.allow_shell` is true.
/// 3. **Base-directory prefix** — for bundled skills with `files`, the
///    extraction directory is prepended to the first text block.
///
/// Returns one `PromptPart::Text` for in-process skills. Bundled skills with
/// extracted files get the prefix injected here.
pub async fn render_skill_prompt(
    skill: &SkillDefinition,
    args: &str,
    ctx: &RenderContext,
) -> Vec<PromptPart> {
    let args_opt = if args.is_empty() { None } else { Some(args) };
    let mut text = substitute_arguments(
        &skill.prompt,
        args_opt,
        &skill.argument_names,
        /* append_if_no_placeholder */ true,
    );

    // shell_exec runs unconditionally and gates internally via the
    // `skip_shell` parameter (matches TS `executeShellCommandsInPrompt`
    // which is always called and uses `allow` flag).
    text = shell_exec::execute_shell_in_prompt(&text, !ctx.allow_shell).await;

    let extracted_dir = if !skill.files.is_empty() {
        extraction::extract_bundled_skill_files(&skill.name, &skill.files).await
    } else {
        // Allow callers to set `skill_root` in advance if they extract via a
        // different path (e.g. a deferred installer). We only touch the value
        // when there are no in-memory `files` to extract.
        skill.skill_root.clone()
    };

    if let Some(dir) = extracted_dir {
        text = extraction::prepend_base_dir(&text, &dir);
    }

    vec![PromptPart::Text { text }]
}

/// Render and update the skill record's `skill_root` field with the extraction
/// path. Useful for callers that want to persist the dir to disk metadata.
pub async fn render_with_extraction(
    skill: &mut SkillDefinition,
    args: &str,
    ctx: &RenderContext,
) -> (Vec<PromptPart>, Option<PathBuf>) {
    let parts = render_skill_prompt(skill, args, ctx).await;
    if !skill.files.is_empty()
        && let Some(p) = skill.skill_root.clone()
    {
        return (parts, Some(p));
    }
    if !skill.files.is_empty() {
        let dir = extraction::extract_dir_for(&skill.name);
        skill.skill_root = Some(dir.clone());
        (parts, Some(dir))
    } else {
        (parts, None)
    }
}

/// Substitute `$ARGUMENTS`, `$ARGUMENTS[N]`, `$N`, and named `$name` in a prompt.
///
/// TS source: `utils/argumentSubstitution.ts substituteArguments` (verbatim port).
///
/// **Order matters** (matches TS exactly):
/// 1. Named tokens `$<name>` first, with word-boundary lookahead so
///    `$foo` doesn't match `$foobar` and `$foo[0]` is left for step 2.
/// 2. `$ARGUMENTS[N]` (any digits) → parsed-args[N] or empty.
/// 3. `$N` (any digits, zero-indexed, with no trailing word char) →
///    parsed-args[N] or empty.
/// 4. `$ARGUMENTS` → the full args string verbatim.
/// 5. If no placeholder matched and `append_if_no_placeholder` is true and
///    args is non-empty, append `\n\nARGUMENTS: <args>`.
///
/// Argument splitting honors quoted strings (single/double quotes, backslash
/// escapes) so `/foo "hello world"` parses as one arg, mirroring TS
/// `parseArguments` which uses shell-quote.
pub fn substitute_arguments(
    prompt: &str,
    args: Option<&str>,
    argument_names: &[String],
    append_if_no_placeholder: bool,
) -> String {
    // TS: `args === undefined || args === null` → return content unchanged.
    let Some(args) = args else {
        return prompt.to_string();
    };

    let parsed = parse_arguments(args);
    let original = prompt.to_string();
    let mut out = original.clone();

    // The `regex` crate does not support lookahead, so word-boundary checks
    // are done manually after the candidate match: `(?![\[\w])` for named
    // tokens, `(?!\w)` for the `$N` shorthand. `replace_all` would over-
    // match (e.g., `$foo` inside `$foobar`), so we walk matches and rebuild
    // the string with explicit boundary checks.

    // 1. Named tokens — `$name` followed by neither `[` nor a word char.
    for (i, name) in argument_names.iter().enumerate() {
        if !is_valid_arg_name(name) {
            continue;
        }
        let pat = format!(r"\${}", regex::escape(name));
        let Ok(re) = regex::Regex::new(&pat) else {
            continue;
        };
        let value = parsed.get(i).map(String::as_str).unwrap_or("");
        out = replace_with_negative_lookahead(&out, &re, value, |next| match next {
            Some(c) => c != '[' && !is_word_char(c),
            None => true,
        });
    }

    // 2. `$ARGUMENTS[N]` — bracketed index always wins.
    if let Ok(bracket_re) = regex::Regex::new(r"\$ARGUMENTS\[(\d+)\]") {
        out = bracket_re
            .replace_all(&out, |caps: &regex::Captures<'_>| {
                let idx: usize = caps[1].parse().unwrap_or(usize::MAX);
                parsed.get(idx).cloned().unwrap_or_default()
            })
            .into_owned();
    }

    // 3. `$N` (zero-indexed, must not be followed by another word char).
    if let Ok(dollar_n) = regex::Regex::new(r"\$(\d+)") {
        out = replace_dollar_n(&out, &dollar_n, &parsed);
    }

    // 4. `$ARGUMENTS` → full args verbatim.
    out = out.replace("$ARGUMENTS", args);

    // 5. Fallback append.
    if out == original && append_if_no_placeholder && !args.is_empty() {
        out.push_str("\n\nARGUMENTS: ");
        out.push_str(args);
    }

    out
}

fn is_word_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// Walk regex matches and rebuild the string, only replacing when
/// `boundary_ok` of the next character is true. The match is dropped when the
/// boundary check fails — the original literal is left in place.
fn replace_with_negative_lookahead<F>(
    haystack: &str,
    re: &regex::Regex,
    replacement: &str,
    boundary_ok: F,
) -> String
where
    F: Fn(Option<char>) -> bool,
{
    let mut out = String::with_capacity(haystack.len());
    let mut last = 0;
    for m in re.find_iter(haystack) {
        out.push_str(&haystack[last..m.start()]);
        let next = haystack[m.end()..].chars().next();
        if boundary_ok(next) {
            out.push_str(replacement);
        } else {
            out.push_str(m.as_str());
        }
        last = m.end();
    }
    out.push_str(&haystack[last..]);
    out
}

/// Replace `$N` only when the digits are NOT followed by another word char.
/// `$10` works; `$1abc` does not.
fn replace_dollar_n(haystack: &str, re: &regex::Regex, parsed: &[String]) -> String {
    let mut out = String::with_capacity(haystack.len());
    let mut last = 0;
    for caps in re.captures_iter(haystack) {
        let m = match caps.get(0) {
            Some(m) => m,
            None => continue,
        };
        out.push_str(&haystack[last..m.start()]);
        let next = haystack[m.end()..].chars().next();
        let is_followed_by_word = matches!(next, Some(c) if is_word_char(c));
        if is_followed_by_word {
            out.push_str(m.as_str());
        } else {
            let idx: usize = caps
                .get(1)
                .and_then(|m| m.as_str().parse().ok())
                .unwrap_or(usize::MAX);
            out.push_str(parsed.get(idx).map(String::as_str).unwrap_or(""));
        }
        last = m.end();
    }
    out.push_str(&haystack[last..]);
    out
}

/// Validate an argument name: non-empty, non-numeric (numbers conflict with
/// `$N` shorthand). Matches TS `isValidName`.
fn is_valid_arg_name(name: &str) -> bool {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return false;
    }
    !trimmed.chars().all(|c| c.is_ascii_digit())
}

/// Parse an arguments string honoring single/double quotes and backslash
/// escapes. Matches TS `parseArguments` semantics (which uses shell-quote).
///
/// Examples:
/// - `foo bar baz` → `["foo", "bar", "baz"]`
/// - `foo "hello world" baz` → `["foo", "hello world", "baz"]`
/// - `foo 'a b' c` → `["foo", "a b", "c"]`
/// - `foo\ bar` → `["foo bar"]`
pub fn parse_arguments(args: &str) -> Vec<String> {
    let s = args.trim();
    if s.is_empty() {
        return Vec::new();
    }

    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_single = false;
    let mut in_double = false;
    let mut chars = s.chars().peekable();

    while let Some(c) = chars.next() {
        if in_single {
            if c == '\'' {
                in_single = false;
            } else {
                current.push(c);
            }
        } else if in_double {
            if c == '"' {
                in_double = false;
            } else if c == '\\' {
                if let Some(&next) = chars.peek() {
                    // Only `\"` and `\\` are escaped inside double quotes.
                    if next == '"' || next == '\\' {
                        chars.next();
                        current.push(next);
                    } else {
                        current.push(c);
                    }
                } else {
                    current.push(c);
                }
            } else {
                current.push(c);
            }
        } else if c == '\'' {
            in_single = true;
        } else if c == '"' {
            in_double = true;
        } else if c == '\\' {
            if let Some(next) = chars.next() {
                current.push(next);
            }
        } else if c.is_whitespace() {
            if !current.is_empty() {
                tokens.push(std::mem::take(&mut current));
            }
        } else {
            current.push(c);
        }
    }

    if !current.is_empty() {
        tokens.push(current);
    }

    tokens
}

#[cfg(test)]
#[path = "prompt_render.test.rs"]
mod tests;
