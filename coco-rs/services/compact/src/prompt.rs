//! Compaction prompt templates.
//!
//! TS: services/compact/prompt.ts — detailed analysis + summary structure.

/// No-tools preamble to prevent LLM from calling tools during compaction.
const NO_TOOLS_PREAMBLE: &str = "CRITICAL: Respond with TEXT ONLY. Do NOT call any tools.

- Do NOT use Read, Bash, Grep, Glob, Edit, Write, or ANY other tool.
- You already have all the context you need in the conversation above.
- Tool calls will be REJECTED and will waste your only turn — you will fail the task.
- Your entire response must be plain text: an <analysis> block followed by a <summary> block.

";

const NO_TOOLS_TRAILER: &str = "\n\nREMINDER: Do NOT call any tools. Respond with plain text only — \
an <analysis> block followed by a <summary> block. \
Tool calls will be rejected and you will fail the task.";

const DETAILED_ANALYSIS_INSTRUCTION: &str = "Before providing your final summary, wrap your analysis in <analysis> tags to organize your thoughts and ensure you've covered all necessary points. In your analysis process:

1. Chronologically analyze each message and section of the conversation. For each section thoroughly identify:
   - The user's explicit requests and intents
   - Your approach to addressing the user's requests
   - Key decisions, technical concepts and code patterns
   - Specific details like:
     - file names
     - full code snippets
     - function signatures
     - file edits
   - Errors that you ran into and how you fixed them
   - Pay special attention to specific user feedback that you received, especially if the user told you to do something differently.
2. Double-check for technical accuracy and completeness, addressing each required element thoroughly.";

const BASE_COMPACT_TEMPLATE: &str = r#"Your task is to create a detailed summary of the conversation so far, paying close attention to the user's explicit requests and your previous actions.
This summary should be thorough in capturing technical details, code patterns, and architectural decisions that would be essential for continuing development work without losing context.

{ANALYSIS_INSTRUCTION}

Your summary should include the following sections:

1. Primary Request and Intent: Capture all of the user's explicit requests and intents in detail
2. Key Technical Concepts: List all important technical concepts, technologies, and frameworks discussed.
3. Files and Code Sections: Enumerate specific files and code sections examined, modified, or created. Pay special attention to the most recent messages and include full code snippets where applicable and include a summary of why this file read or edit is important.
4. Errors and fixes: List all errors that you ran into, and how you fixed them. Pay special attention to specific user feedback that you received, especially if the user told you to do something differently.
5. Problem Solving: Document problems solved and any ongoing troubleshooting efforts.
6. All user messages: List ALL user messages that are not tool results. These are critical for understanding the users' feedback and changing intent.
7. Pending Tasks: Outline any pending tasks that you have explicitly been asked to work on.
8. Current Work: Describe in detail precisely what was being worked on immediately before this summary request, paying special attention to the most recent messages from both user and assistant. Include file names and code snippets where applicable.
9. Optional Next Step: List the next step that you will take that is related to the most recent work you were doing. IMPORTANT: ensure that this step is DIRECTLY in line with the user's most recent explicit requests, and the task you were working on immediately before this summary request.

Please provide your summary based on the conversation so far, following this structure and ensuring precision and thoroughness in your response."#;

/// Build the full compaction prompt.
pub fn get_compact_prompt(custom_instructions: Option<&str>) -> String {
    let template =
        BASE_COMPACT_TEMPLATE.replace("{ANALYSIS_INSTRUCTION}", DETAILED_ANALYSIS_INSTRUCTION);
    let mut prompt = format!("{NO_TOOLS_PREAMBLE}{template}");

    if let Some(instructions) = custom_instructions
        && !instructions.trim().is_empty()
    {
        prompt.push_str(&format!("\n\nAdditional Instructions:\n{instructions}"));
    }

    prompt.push_str(NO_TOOLS_TRAILER);
    prompt
}

/// Build the partial compaction prompt (for compacting only recent messages).
pub fn get_partial_compact_prompt(custom_instructions: Option<&str>) -> String {
    let template = "Your task is to create a detailed summary of the RECENT portion of the conversation — the messages that follow earlier retained context. The earlier messages are being kept intact and do NOT need to be summarized. Focus your summary on what was discussed, learned, and accomplished in the recent messages only.

Before providing your final summary, wrap your analysis in <analysis> tags. Analyze the recent messages chronologically, identifying requests, approaches, decisions, file changes, errors, and user feedback.

Your summary should include these sections:
1. Primary Request and Intent
2. Key Technical Concepts
3. Files and Code Sections (with snippets)
4. Errors and fixes
5. Problem Solving
6. All user messages (non-tool-result)
7. Pending Tasks
8. Current Work
9. Optional Next Step

Please provide your summary based on the RECENT messages only.";

    let mut prompt = format!("{NO_TOOLS_PREAMBLE}{template}");

    if let Some(instructions) = custom_instructions
        && !instructions.trim().is_empty()
    {
        prompt.push_str(&format!("\n\nAdditional Instructions:\n{instructions}"));
    }

    prompt.push_str(NO_TOOLS_TRAILER);
    prompt
}

/// Format the compact summary by stripping <analysis> scratchpad and extracting <summary>.
///
/// TS: formatCompactSummary() — strips analysis block, extracts summary content.
pub fn format_compact_summary(summary: &str) -> String {
    let mut result = summary.to_string();

    // Strip analysis section (drafting scratchpad)
    if let Some(start) = result.find("<analysis>")
        && let Some(end) = result.find("</analysis>")
    {
        result = format!(
            "{}{}",
            &result[..start],
            &result[end + "</analysis>".len()..]
        );
    }

    // Extract and format summary section
    if let Some(start) = result.find("<summary>")
        && let Some(end) = result.find("</summary>")
    {
        let content = &result[start + "<summary>".len()..end];
        result = format!("Summary:\n{}", content.trim());
    }

    // Clean up runs of 3+ newlines → 2 (matches TS regex /\n\n+/g → \n\n)
    let mut cleaned = String::with_capacity(result.len());
    let mut consecutive_newlines = 0u32;
    for ch in result.chars() {
        if ch == '\n' {
            consecutive_newlines += 1;
            if consecutive_newlines <= 2 {
                cleaned.push(ch);
            }
        } else {
            consecutive_newlines = 0;
            cleaned.push(ch);
        }
    }

    cleaned.trim().to_string()
}

/// Build the user-facing summary message shown after compaction.
///
/// TS: `getCompactUserSummaryMessage()` — expects pre-formatted summary
/// (caller must call `format_compact_summary` first).
pub fn get_compact_user_summary_message(
    summary: &str,
    suppress_follow_up: bool,
    transcript_path: Option<&str>,
) -> String {
    let mut msg = format!(
        "This session is being continued from a previous conversation that ran out of context. \
         The summary below covers the earlier portion of the conversation.\n\n{summary}"
    );

    if let Some(path) = transcript_path {
        msg.push_str(&format!(
            "\n\nIf you need specific details from before compaction \
             (like exact code snippets, error messages, or content you generated), \
             read the full transcript at: {path}"
        ));
    }

    if suppress_follow_up {
        msg.push_str(
            "\nContinue the conversation from where it left off without asking the user \
             any further questions. Resume directly — do not acknowledge the summary, \
             do not recap what was happening, do not preface with \"I'll continue\" or similar. \
             Pick up the last task as if the break never happened.",
        );
    }

    msg
}

#[cfg(test)]
#[path = "prompt.test.rs"]
mod tests;
