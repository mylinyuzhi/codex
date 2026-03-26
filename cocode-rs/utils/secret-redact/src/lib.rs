//! Best-effort secret redaction for tool output.
//!
//! Applies well-known regex patterns to strip API keys, bearer tokens,
//! AWS access keys, and generic secret assignments from strings before
//! they enter model context or logs.

use std::borrow::Cow;
use std::sync::LazyLock;

use regex::Regex;

// Provider-specific key patterns.
static OPENAI_KEY_REGEX: LazyLock<Regex> = LazyLock::new(|| compile_regex(r"sk-[A-Za-z0-9]{20,}"));

// Must be applied BEFORE OPENAI_KEY_REGEX because `sk-ant-...` would
// otherwise be partially matched by the `sk-...` pattern.
static ANTHROPIC_KEY_REGEX: LazyLock<Regex> =
    LazyLock::new(|| compile_regex(r"sk-ant-[A-Za-z0-9\-]{20,}"));

static GITHUB_TOKEN_REGEX: LazyLock<Regex> =
    LazyLock::new(|| compile_regex(r"\b(ghp|ghs|gho|ghu|ghr|github_pat)_[A-Za-z0-9]{20,}\b"));

static SLACK_TOKEN_REGEX: LazyLock<Regex> =
    LazyLock::new(|| compile_regex(r"\bxox[bpras]-[A-Za-z0-9\-]{10,}\b"));

static AWS_ACCESS_KEY_ID_REGEX: LazyLock<Regex> =
    LazyLock::new(|| compile_regex(r"\bAKIA[0-9A-Z]{16}\b"));

// Generic patterns.
static BEARER_TOKEN_REGEX: LazyLock<Regex> =
    LazyLock::new(|| compile_regex(r"(?i)\bBearer\s+[A-Za-z0-9._\-]{16,}\b"));

static SECRET_ASSIGNMENT_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    compile_regex(r#"(?i)\b(api[_-]?key|token|secret|password)\b(\s*[:=]\s*)(["']?)[^\s"']{8,}"#)
});

/// Placeholder inserted in place of detected secrets.
const REDACTED: &str = "[REDACTED_SECRET]";
const BEARER_REPLACEMENT: &str = "Bearer [REDACTED_SECRET]";
const ASSIGNMENT_REPLACEMENT: &str = "$1$2$3[REDACTED_SECRET]";

/// Apply a regex replacement, only allocating when there is a match.
fn apply_regex<'a>(input: Cow<'a, str>, regex: &Regex, replacement: &str) -> Cow<'a, str> {
    match regex.replace_all(&input, replacement) {
        Cow::Borrowed(_) => input,
        Cow::Owned(new) => Cow::Owned(new),
    }
}

/// Remove secrets and keys from a string on a best-effort basis.
///
/// Returns the input unchanged (zero-copy) when no secrets are found.
///
/// Matches Anthropic keys (`sk-ant-...`), OpenAI keys (`sk-...`),
/// GitHub tokens (`ghp_...`, `github_pat_...`), Slack tokens (`xoxb-...`),
/// AWS access key IDs (`AKIA...`), Bearer tokens, and generic
/// `key=value` / `secret: value` assignments.
pub fn redact_secrets(input: &str) -> Cow<'_, str> {
    let s = Cow::Borrowed(input);
    let s = apply_regex(s, &ANTHROPIC_KEY_REGEX, REDACTED);
    let s = apply_regex(s, &OPENAI_KEY_REGEX, REDACTED);
    let s = apply_regex(s, &GITHUB_TOKEN_REGEX, REDACTED);
    let s = apply_regex(s, &SLACK_TOKEN_REGEX, REDACTED);
    let s = apply_regex(s, &AWS_ACCESS_KEY_ID_REGEX, REDACTED);
    let s = apply_regex(s, &BEARER_TOKEN_REGEX, BEARER_REPLACEMENT);
    apply_regex(s, &SECRET_ASSIGNMENT_REGEX, ASSIGNMENT_REPLACEMENT)
}

fn compile_regex(pattern: &str) -> Regex {
    match Regex::new(pattern) {
        Ok(regex) => regex,
        // Panic is acceptable: `load_regex` test catches this at build time.
        Err(err) => panic!("invalid regex pattern `{pattern}`: {err}"),
    }
}

#[cfg(test)]
#[path = "lib.test.rs"]
mod tests;
