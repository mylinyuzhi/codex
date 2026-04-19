//! Session title auto-generation from an approved plan.
//!
//! TS: `src/utils/sessionTitle.ts` (`generateSessionTitle`) — called
//! post-plan-approval with the plan text + first user message,
//! returning a concise 3-7 word sentence-case title via a lightweight
//! LLM call. We target `ModelRole::Fast`; if the user hasn't configured
//! a Fast role, the feature silently stays off (caller checks).
//!
//! This module is prompt/parse-only — it does not own the LLM
//! invocation. Callers (engine post-ExitPlanMode path) resolve the
//! `ModelSpec` via `ModelRoles::get(Fast)`, run the generated prompt
//! through their inference layer, and pass the raw response here for
//! parsing.

use serde::Deserialize;

/// Minimum title length we'll accept from the model (guards against
/// empty / 1-char junk output).
const MIN_TITLE_LEN: usize = 3;
/// Maximum length in chars before we truncate — typical good titles are
/// 3-7 words, ~50 chars; 100 is a generous upper bound.
const MAX_TITLE_LEN: usize = 100;

/// How much of the plan body to include in the title-generation prompt.
/// TS: sessionTitle.ts passes up to 1000 chars. Same bound.
const PLAN_CONTEXT_CHARS: usize = 1000;

/// Build the system + user prompt pair for the title-generation call.
///
/// Returns `(system_prompt, user_prompt)`. Callers pass these to the
/// Fast-role model and parse the response via [`parse_title_response`].
pub fn build_title_prompt(plan_text: &str) -> (String, String) {
    let system = "You generate concise, human-friendly session titles \
        for software-engineering work. Output ONLY a JSON object like \
        {\"title\":\"Fix login button on mobile\"}. The title should be \
        3-7 words, sentence case, no trailing punctuation, no quotes, \
        describing the goal of the plan — not its structure. Never \
        include phrases like 'Plan for' or 'Add feature'; lead with the \
        verb or change."
        .to_string();

    let mut context = plan_text;
    if context.len() > PLAN_CONTEXT_CHARS {
        // Truncate at char boundary to avoid splitting UTF-8 sequences.
        let mut end = PLAN_CONTEXT_CHARS;
        while !context.is_char_boundary(end) && end > 0 {
            end -= 1;
        }
        context = &context[..end];
    }
    let user = format!(
        "Generate a title for this plan. Respond with JSON only.\n\n\
         --- PLAN ---\n{context}"
    );
    (system, user)
}

/// Parse the model's JSON response into a title string.
///
/// Accepts either a JSON object `{"title": "..."}` (the strict schema
/// the prompt asks for) or a raw JSON string. Returns `None` if the
/// response doesn't parse or the extracted title is outside length
/// bounds — callers should fall back gracefully (leave the session
/// untitled).
pub fn parse_title_response(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    // Primary path: strict JSON object.
    #[derive(Deserialize)]
    struct TitleJson {
        title: String,
    }
    if let Ok(parsed) = serde_json::from_str::<TitleJson>(trimmed) {
        return normalize_title(&parsed.title);
    }

    // Fallback: bare JSON string ("Fix login button").
    if let Ok(s) = serde_json::from_str::<String>(trimmed) {
        return normalize_title(&s);
    }

    // Last resort: model ignored the schema and returned plain text.
    // Take the first line, hope for the best.
    let first_line = trimmed.lines().next().unwrap_or("").trim();
    // Strip common wrapping: outer quotes, `title:` labels.
    let cleaned = first_line
        .trim_start_matches("title:")
        .trim_start_matches("Title:")
        .trim_matches(|c: char| c == '"' || c == '\'' || c.is_whitespace());
    normalize_title(cleaned)
}

fn normalize_title(raw: &str) -> Option<String> {
    let s = raw.trim().trim_end_matches(|c: char| c == '.' || c == '!');
    if s.len() < MIN_TITLE_LEN {
        return None;
    }
    let truncated = if s.len() > MAX_TITLE_LEN {
        // Char-boundary safe truncation.
        let mut end = MAX_TITLE_LEN;
        while !s.is_char_boundary(end) && end > 0 {
            end -= 1;
        }
        s[..end].to_string()
    } else {
        s.to_string()
    };
    Some(truncated)
}

/// Apply a generated title to a session record, persisting it via the
/// session manager. Respects an existing user-set title (won't
/// overwrite — matches TS behavior where `/rename` wins).
///
/// Returns `true` if the title was applied, `false` if the session
/// already had one.
pub fn apply_title(
    manager: &crate::SessionManager,
    session_id: &str,
    title: String,
) -> anyhow::Result<bool> {
    let mut session = manager.load(session_id)?;
    if session.title.as_deref().is_some_and(|t| !t.is_empty()) {
        return Ok(false);
    }
    session.title = Some(title);
    manager.save(&session)?;
    Ok(true)
}

#[cfg(test)]
#[path = "title_generator.test.rs"]
mod tests;
