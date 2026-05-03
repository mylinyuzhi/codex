//! Prompt builders — concatenate verbatim text blocks (`include_str!`)
//! with run-time inputs.

use std::path::Path;

const TYPES_INDIVIDUAL: &str = include_str!("text/types_individual.md");
const TYPES_COMBINED: &str = include_str!("text/types_combined.md");
const WHAT_NOT_TO_SAVE: &str = include_str!("text/what_not_to_save.md");
const HOW_TO_SAVE: &str = include_str!("text/how_to_save.md");
const HOW_TO_SAVE_SKIP_INDEX: &str = include_str!("text/how_to_save_skipindex.md");
const WHEN_TO_ACCESS: &str = include_str!("text/when_to_access.md");
const EXTRACT_GUIDANCE: &str = include_str!("text/extract.md");
const DREAM_GUIDANCE: &str = include_str!("text/dream.md");
const SESSION_TEMPLATE: &str = include_str!("text/session_template.md");

/// Which system-prompt variant to render.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemPromptVariant {
    /// Personal-only memory (no team).
    Auto,
    /// Personal + team memory directories.
    Combined,
    /// KAIROS daily-log mode (assistant-mode append-only).
    Kairos,
}

/// Build the `# auto memory` system-prompt block.
///
/// `index_content` is the truncated `MEMORY.md` body (or `None` when
/// the file is missing / empty). `skip_index` corresponds to TS feature
/// `tengu_moth_copse` — when set, the model is told to write topic
/// files only (no two-step indexing).
pub fn build_system_prompt_section(
    variant: SystemPromptVariant,
    memory_dir: &Path,
    team_dir: Option<&Path>,
    index_content: Option<&str>,
    team_index_content: Option<&str>,
    skip_index: bool,
    extra_guidelines: Option<&str>,
) -> String {
    if matches!(variant, SystemPromptVariant::Kairos) {
        return build_kairos_prompt(memory_dir);
    }

    let mut sections = Vec::new();
    sections.push("# auto memory".to_string());

    let dir_blurb = if let Some(team) = team_dir
        && matches!(variant, SystemPromptVariant::Combined)
    {
        format!(
            "You have a persistent, file-based memory system with two directories:\n- private: `{}`\n- team:    `{}`\nBoth directories already exist — write to them directly with the Write tool (do not run mkdir or check for their existence).",
            memory_dir.display(),
            team.display(),
        )
    } else {
        format!(
            "You have a persistent, file-based memory system at `{}`. This directory already exists — write to it directly with the Write tool (do not run mkdir or check for its existence).",
            memory_dir.display(),
        )
    };
    sections.push(dir_blurb);

    sections.push(BEHAVIOR_GUIDANCE.to_string());

    sections.push(match variant {
        SystemPromptVariant::Combined => TYPES_COMBINED.to_string(),
        _ => TYPES_INDIVIDUAL.to_string(),
    });

    sections.push(WHAT_NOT_TO_SAVE.to_string());
    sections.push(if skip_index {
        HOW_TO_SAVE_SKIP_INDEX.to_string()
    } else {
        HOW_TO_SAVE.to_string()
    });
    sections.push(WHEN_TO_ACCESS.to_string());

    if let Some(guidance) = extra_guidelines
        && !guidance.trim().is_empty()
    {
        sections.push(guidance.to_string());
    }

    if let Some(body) = index_content
        && !body.trim().is_empty()
    {
        sections.push("## MEMORY.md".to_string());
        sections.push(body.to_string());
    } else {
        sections.push(
            "## MEMORY.md\n\nYour MEMORY.md is currently empty. When you save new memories, they will appear here."
                .to_string(),
        );
    }

    if let Some(team_body) = team_index_content
        && !team_body.trim().is_empty()
        && matches!(variant, SystemPromptVariant::Combined)
    {
        sections.push("## Team MEMORY.md".to_string());
        sections.push(team_body.to_string());
    }

    sections.join("\n\n")
}

const BEHAVIOR_GUIDANCE: &str = "You should build up this memory system over time so that future conversations can have a complete picture of who the user is, how they'd like to collaborate with you, what behaviors to avoid or repeat, and the context behind the work the user gives you.\n\nIf the user explicitly asks you to remember something, save it immediately as whichever type fits best. If they ask you to forget something, find and remove the relevant entry.";

/// Build the KAIROS daily-log prompt — used when `kairos_mode` is set.
pub fn build_kairos_prompt(memory_dir: &Path) -> String {
    let log_pattern = memory_dir
        .join("logs")
        .join("YYYY")
        .join("MM")
        .join("YYYY-MM-DD.md");
    let mem = memory_dir.display();
    let log = log_pattern.display();
    let not_to_save = WHAT_NOT_TO_SAVE;
    format!(
        "# auto memory\n\nYou have a persistent, file-based memory system at: `{mem}`\n\nThis session is long-lived. Record anything worth remembering by **appending** to today's daily log file:\n\n`{log}`\n\nSubstitute today's date for `YYYY-MM-DD`. When the date rolls over mid-session, start appending to the new day's file.\n\nWrite each entry as a short timestamped bullet. Create the file (and parent directories) on first write if it does not exist. Do not rewrite or reorganize the log — it is append-only. A separate nightly process distills these logs into MEMORY.md and topic files.\n\n## What to log\n- User corrections and preferences (\"use bun, not npm\"; \"stop summarizing diffs\")\n- Facts about the user, their role, or their goals\n- Project context that is not derivable from the code (deadlines, incidents, decisions and their rationale)\n- Pointers to external systems (dashboards, Linear projects, Slack channels)\n- Anything the user explicitly asks you to remember\n\n{not_to_save}\n\n## MEMORY.md\n`MEMORY.md` is the distilled index (maintained nightly) and is loaded into your context automatically. Read it for orientation, but do not edit it directly — record new information in today's log instead."
    )
}

/// Build the extraction-agent system prompt.
pub fn build_extract_prompt(message_count: i32, manifest: &str, skip_index: bool) -> String {
    let how_to = if skip_index {
        HOW_TO_SAVE_SKIP_INDEX
    } else {
        HOW_TO_SAVE
    };
    let guidance = EXTRACT_GUIDANCE;
    let count = message_count;
    let types = TYPES_INDIVIDUAL;
    let not_to_save = WHAT_NOT_TO_SAVE;
    format!(
        "{guidance}\n\nAnalyze the last ~{count} messages from the conversation.\n\n{manifest}\n\n{types}\n\n{not_to_save}\n\n{how_to}"
    )
}

/// Build the auto-dream consolidation agent prompt.
pub fn build_dream_prompt(
    memory_dir: &Path,
    transcript_dir: &Path,
    sessions_since_last: &[String],
) -> String {
    let sessions = if sessions_since_last.is_empty() {
        String::new()
    } else {
        let count = sessions_since_last.len();
        let body = sessions_since_last
            .iter()
            .map(|s| format!("- {s}"))
            .collect::<Vec<_>>()
            .join("\n");
        format!("\n\nSessions since last consolidation ({count}):\n{body}")
    };
    let guidance = DREAM_GUIDANCE;
    let mem = memory_dir.display();
    let trans = transcript_dir.display();
    format!(
        "{guidance}\n\nMemory directory: `{mem}` — this directory already exists.\nTranscript directory: `{trans}` — contains large JSONL files, grep narrowly.{sessions}"
    )
}

/// The verbatim 9-section session-memory template (TS:
/// `services/SessionMemory/prompts.ts:DEFAULT_SESSION_MEMORY_TEMPLATE`).
pub fn build_session_memory_template() -> &'static str {
    SESSION_TEMPLATE
}

/// Build the user-facing prompt for an in-place session-memory update.
///
/// `current_notes_path` is the absolute path the model edits via Edit.
pub fn build_session_memory_update_prompt(current_notes: &str, notes_path: &Path) -> String {
    let path = notes_path.display();
    let notes = current_notes;
    format!(
        "Update the session memory at `{path}`.\n\nCRITICAL EDIT RULES:\n- Preserve every section header (lines starting with `#`).\n- Preserve every italic _section description_ — these are the template instructions.\n- Only update content AFTER each header + italic description.\n- Do NOT add new sections.\n- Keep each section under its token budget. If approaching the limit, condense by removing less important details.\n\nUse the `Edit` tool exactly once per change. The current contents are:\n\n```\n{notes}\n```"
    )
}

#[cfg(test)]
#[path = "builders.test.rs"]
mod tests;
