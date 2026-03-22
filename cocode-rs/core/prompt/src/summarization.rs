//! Summarization prompts for context compaction.
//!
//! Provides prompt construction for full compaction and micro-compaction,
//! and parsing of structured summary responses.

use crate::templates;

/// Parsed summary response with extracted sections.
#[derive(Debug, Clone, Default)]
pub struct ParsedSummary {
    /// The main summary content.
    pub summary: String,
    /// Optional analysis section.
    pub analysis: Option<String>,
}

/// Build a summarization prompt for full context compaction.
///
/// Returns `(system_prompt, user_prompt)` for the summarization request.
pub fn build_summarization_prompt(
    conversation_summary: &str,
    custom_instructions: Option<&str>,
) -> (String, String) {
    let mut system = templates::SUMMARIZATION.to_string();

    if let Some(instructions) = custom_instructions {
        system.push_str("\n\n## Additional Instructions\n\n");
        system.push_str(instructions);
    }

    let user = format!(
        "Please summarize the following conversation:\n\n---\n\n{conversation_summary}\n\n---\n\nProvide your summary using the required section format."
    );

    (system, user)
}

/// Build a brief summarization prompt for micro-compaction.
///
/// Returns `(system_prompt, user_prompt)` for a shorter summary.
pub fn build_brief_summary_prompt(conversation_text: &str) -> (String, String) {
    let system = "You are a conversation summarizer. Provide a brief, actionable summary \
                  of the conversation so far. Focus on: what was done, what files were \
                  changed, and what remains to be done. Be concise (2-4 sentences)."
        .to_string();

    let user = format!("Briefly summarize this conversation:\n\n---\n\n{conversation_text}\n\n---");

    (system, user)
}

/// Parse a summary response, extracting `<summary>` and `<analysis>` tags.
pub fn parse_summary_response(response: &str) -> ParsedSummary {
    let summary = extract_tag(response, "summary").unwrap_or_else(|| response.to_string());
    let analysis = extract_tag(response, "analysis");

    ParsedSummary { summary, analysis }
}

/// Extract content between `<tag>` and `</tag>`.
fn extract_tag(text: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");

    let start = text.find(&open)?;
    let end = text.find(&close)?;

    if end <= start {
        return None;
    }

    let content = &text[start + open.len()..end];
    Some(content.trim().to_string())
}

#[cfg(test)]
#[path = "summarization.test.rs"]
mod tests;
