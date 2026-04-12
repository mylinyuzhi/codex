//! Team memory prompt building.
//!
//! TS: memdir/teamMemPrompts.ts — builds combined memory prompts when
//! both personal and team memory are enabled.

use std::path::Path;

use crate::prompt::truncate_entrypoint_content;
use crate::team_paths;

/// Build a combined memory prompt that includes both personal and team memories.
///
/// When team memory is enabled, the system prompt includes both MEMORY.md files
/// with clear scope labels.
pub fn build_combined_memory_content(personal_content: &str, memory_dir: &Path) -> String {
    let team_content = team_paths::read_team_index(memory_dir);

    let mut sections = Vec::new();

    // Personal memory
    sections.push("## Personal Memory".to_string());
    sections.push(String::new());
    let truncated_personal = truncate_entrypoint_content(personal_content);
    sections.push(truncated_personal);

    // Team memory
    if let Some(team) = team_content
        && !team.trim().is_empty()
    {
        sections.push(String::new());
        sections.push("## Team Memory".to_string());
        sections.push(String::new());
        let truncated_team = truncate_entrypoint_content(&team);
        sections.push(truncated_team);
    }

    sections.join("\n")
}

/// Build the team memory scope guidance for the system prompt.
///
/// Injected when team memory is enabled to help the model decide
/// which scope to use when saving memories.
pub fn team_scope_guidance() -> &'static str {
    "### Team vs Personal scope\n\n\
     When saving memories, decide whether they are:\n\
     - **personal** (default): Only relevant to you and this user. \
     Save to the main memory directory.\n\
     - **team**: Relevant to all team members working on this project. \
     Save to the `team/` subdirectory.\n\n\
     Guidelines for team memories:\n\
     - Project conventions, agreed-upon patterns, and team decisions → team\n\
     - External resource pointers (dashboards, ticket boards, runbooks) → team\n\
     - Individual preferences, role details, personal feedback → personal\n\
     - NEVER put API keys, tokens, credentials, or personal data in team memories"
}

/// Check if memory content should be stored in team scope.
///
/// Heuristic: project and reference types default to team scope,
/// user and feedback types default to personal scope.
pub fn suggest_scope(memory_type: &str) -> &'static str {
    match memory_type {
        "project" | "reference" => "team",
        "user" | "feedback" => "personal",
        _ => "personal",
    }
}
