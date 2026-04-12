//! System prompt memory section and prompt variant builders.
//!
//! TS: memdir/memdir.ts (loadMemoryPrompt, buildMemoryLines, buildMemoryPrompt,
//!     truncateEntrypointContent) + memdir/memoryTypes.ts (type taxonomy).

use std::path::Path;

use crate::config::MemoryConfig;
use crate::staleness::MEMORY_DRIFT_CAVEAT;

/// Maximum lines in MEMORY.md before truncation.
const MAX_ENTRYPOINT_LINES: usize = 200;

/// Maximum bytes in MEMORY.md before truncation.
const MAX_ENTRYPOINT_BYTES: usize = 25_000;

/// Guidance that the memory directory already exists.
const DIR_EXISTS_GUIDANCE: &str = "This directory already exists — write to it directly with the Write tool \
     (do not run mkdir or check for its existence).";

// ── System prompt section ──────────────────────────────────────────────

/// Load the memory section for the system prompt.
///
/// Reads MEMORY.md, truncates to limits, wraps with behavior instructions.
/// Returns `None` if auto-memory is disabled or no MEMORY.md exists.
pub fn load_memory_prompt(config: &MemoryConfig, memory_dir: &Path) -> Option<String> {
    if !config.enabled {
        return None;
    }

    let index_path = memory_dir.join("MEMORY.md");
    let memory_content = std::fs::read_to_string(&index_path).ok()?;

    if memory_content.trim().is_empty() {
        return None;
    }

    let truncated = truncate_entrypoint_content(&memory_content);
    Some(build_memory_prompt(&truncated, config, memory_dir))
}

/// Build the full memory prompt from MEMORY.md content.
fn build_memory_prompt(memory_content: &str, config: &MemoryConfig, memory_dir: &Path) -> String {
    let mut sections = Vec::new();

    // Header
    sections.push("# auto memory".to_string());
    sections.push(format!(
        "You have a persistent, file-based memory system at `{}`. {DIR_EXISTS_GUIDANCE}",
        memory_dir.display()
    ));
    sections.push(build_memory_behavior_guidance());

    // Type taxonomy (XML-tagged, matching TS memoryTypes.ts)
    if config.team_memory_enabled {
        sections.push(build_combined_types_section());
    } else {
        sections.push(build_individual_types_section());
    }

    // What NOT to save
    sections.push(build_what_not_to_save_section());

    // How to save
    sections.push(build_save_instructions(config));

    // When to access
    sections.push(build_when_to_access_section());

    // Before recommending from memory (TRUSTING_RECALL_SECTION)
    sections.push(build_trusting_recall_section());

    // Memory and other persistence
    sections.push(build_persistence_section());

    // Actual MEMORY.md content
    sections.push(memory_content.to_string());

    sections.join("\n\n")
}

fn build_memory_behavior_guidance() -> String {
    "You should build up this memory system over time so that future conversations \
     can have a complete picture of who the user is, how they'd like to collaborate \
     with you, what behaviors to avoid or repeat, and the context behind the work \
     the user gives you.\n\
     \n\
     If the user explicitly asks you to remember something, save it immediately as \
     whichever type fits best. If they ask you to forget something, find and remove \
     the relevant entry."
        .to_string()
}

// ── Type taxonomy (XML-tagged, matching TS memoryTypes.ts) ─────────────

fn build_individual_types_section() -> String {
    r#"## Types of memory

There are several discrete types of memory that you can store in your memory system:

<types>
<type>
    <name>user</name>
    <description>Contain information about the user's role, goals, responsibilities, and knowledge. Great user memories help you tailor your future behavior to the user's preferences and perspective. Your goal in reading and writing these memories is to build up an understanding of who the user is and how you can be most helpful to them specifically. For example, you should collaborate with a senior software engineer differently than a student who is coding for the very first time. Keep in mind, that the aim here is to be helpful to the user. Avoid writing memories about the user that could be viewed as a negative judgement or that are not relevant to the work you're trying to accomplish together.</description>
    <when_to_save>When you learn any details about the user's role, preferences, responsibilities, or knowledge</when_to_save>
    <how_to_use>When your work should be informed by the user's profile or perspective. For example, if the user is asking you to explain a part of the code, you should answer that question in a way that is tailored to the specific details that they will find most valuable or that helps them build their mental model in relation to domain knowledge they already have.</how_to_use>
    <examples>
    user: I'm a data scientist investigating what logging we have in place
    assistant: [saves user memory: user is a data scientist, currently focused on observability/logging]

    user: I've been writing Go for ten years but this is my first time touching the React side of this repo
    assistant: [saves user memory: deep Go expertise, new to React and this project's frontend — frame frontend explanations in terms of backend analogues]
    </examples>
</type>
<type>
    <name>feedback</name>
    <description>Guidance the user has given you about how to approach work — both what to avoid and what to keep doing. These are a very important type of memory to read and write as they allow you to remain coherent and responsive to the way you should approach work in the project. Record from failure AND success: if you only save corrections, you will avoid past mistakes but drift away from approaches the user has already validated, and may grow overly cautious.</description>
    <when_to_save>Any time the user corrects your approach ("no not that", "don't", "stop doing X") OR confirms a non-obvious approach worked ("yes exactly", "perfect, keep doing that", accepting an unusual choice without pushback). Corrections are easy to notice; confirmations are quieter — watch for them. In both cases, save what is applicable to future conversations, especially if surprising or not obvious from the code. Include *why* so you can judge edge cases later.</when_to_save>
    <how_to_use>Let these memories guide your behavior so that the user does not need to offer the same guidance twice.</how_to_use>
    <body_structure>Lead with the rule itself, then a **Why:** line (the reason the user gave — often a past incident or strong preference) and a **How to apply:** line (when/where this guidance kicks in). Knowing *why* lets you judge edge cases instead of blindly following the rule.</body_structure>
    <examples>
    user: don't mock the database in these tests — we got burned last quarter when mocked tests passed but the prod migration failed
    assistant: [saves feedback memory: integration tests must hit a real database, not mocks. Reason: prior incident where mock/prod divergence masked a broken migration]

    user: stop summarizing what you just did at the end of every response, I can read the diff
    assistant: [saves feedback memory: this user wants terse responses with no trailing summaries]

    user: yeah the single bundled PR was the right call here, splitting this one would've just been churn
    assistant: [saves feedback memory: for refactors in this area, user prefers one bundled PR over many small ones. Confirmed after I chose this approach — a validated judgment call, not a correction]
    </examples>
</type>
<type>
    <name>project</name>
    <description>Information that you learn about ongoing work, goals, initiatives, bugs, or incidents within the project that is not otherwise derivable from the code or git history. Project memories help you understand the broader context and motivation behind the work the user is doing within this working directory.</description>
    <when_to_save>When you learn who is doing what, why, or by when. These states change relatively quickly so try to keep your understanding of this up to date. Always convert relative dates in user messages to absolute dates when saving (e.g., "Thursday" → "2026-03-05"), so the memory remains interpretable after time passes.</when_to_save>
    <how_to_use>Use these memories to more fully understand the details and nuance behind the user's request and make better informed suggestions.</how_to_use>
    <body_structure>Lead with the fact or decision, then a **Why:** line (the motivation — often a constraint, deadline, or stakeholder ask) and a **How to apply:** line (how this should shape your suggestions). Project memories decay fast, so the why helps future-you judge whether the memory is still load-bearing.</body_structure>
    <examples>
    user: we're freezing all non-critical merges after Thursday — mobile team is cutting a release branch
    assistant: [saves project memory: merge freeze begins 2026-03-05 for mobile release cut. Flag any non-critical PR work scheduled after that date]

    user: the reason we're ripping out the old auth middleware is that legal flagged it for storing session tokens in a way that doesn't meet the new compliance requirements
    assistant: [saves project memory: auth middleware rewrite is driven by legal/compliance requirements around session token storage, not tech-debt cleanup — scope decisions should favor compliance over ergonomics]
    </examples>
</type>
<type>
    <name>reference</name>
    <description>Stores pointers to where information can be found in external systems. These memories allow you to remember where to look to find up-to-date information outside of the project directory.</description>
    <when_to_save>When you learn about resources in external systems and their purpose. For example, that bugs are tracked in a specific project in Linear or that feedback can be found in a specific Slack channel.</when_to_save>
    <how_to_use>When the user references an external system or information that may be in an external system.</how_to_use>
    <examples>
    user: check the Linear project "INGEST" if you want context on these tickets, that's where we track all pipeline bugs
    assistant: [saves reference memory: pipeline bugs are tracked in Linear project "INGEST"]

    user: the Grafana board at grafana.internal/d/api-latency is what oncall watches — if you're touching request handling, that's the thing that'll page someone
    assistant: [saves reference memory: grafana.internal/d/api-latency is the oncall latency dashboard — check it when editing request-path code]
    </examples>
</type>
</types>"#
        .to_string()
}

fn build_combined_types_section() -> String {
    r#"## Types of memory

There are several discrete types of memory that you can store in your memory system. Each type below declares a <scope> of `private`, `team`, or guidance for choosing between the two.

<types>
<type>
    <name>user</name>
    <scope>always private</scope>
    <description>Contain information about the user's role, goals, responsibilities, and knowledge. Great user memories help you tailor your future behavior to the user's preferences and perspective. Your goal in reading and writing these memories is to build up an understanding of who the user is and how you can be most helpful to them specifically. For example, you should collaborate with a senior software engineer differently than a student who is coding for the very first time. Keep in mind, that the aim here is to be helpful to the user. Avoid writing memories about the user that could be viewed as a negative judgement or that are not relevant to the work you're trying to accomplish together.</description>
    <when_to_save>When you learn any details about the user's role, preferences, responsibilities, or knowledge</when_to_save>
    <how_to_use>When your work should be informed by the user's profile or perspective.</how_to_use>
    <examples>
    user: I'm a data scientist investigating what logging we have in place
    assistant: [saves private user memory: user is a data scientist, currently focused on observability/logging]

    user: I've been writing Go for ten years but this is my first time touching the React side of this repo
    assistant: [saves private user memory: deep Go expertise, new to React and this project's frontend — frame frontend explanations in terms of backend analogues]
    </examples>
</type>
<type>
    <name>feedback</name>
    <scope>default to private. Save as team only when the guidance is clearly a project-wide convention that every contributor should follow (e.g., a testing policy, a build invariant), not a personal style preference.</scope>
    <description>Guidance the user has given you about how to approach work — both what to avoid and what to keep doing. These are a very important type of memory to read and write as they allow you to remain coherent and responsive to the way you should approach work in the project. Record from failure AND success: if you only save corrections, you will avoid past mistakes but drift away from approaches the user has already validated, and may grow overly cautious. Before saving a private feedback memory, check that it doesn't contradict a team feedback memory — if it does, either don't save it or note the override explicitly.</description>
    <when_to_save>Any time the user corrects your approach OR confirms a non-obvious approach worked. Include *why* so you can judge edge cases later.</when_to_save>
    <how_to_use>Let these memories guide your behavior so that the user and other users in the project do not need to offer the same guidance twice.</how_to_use>
    <body_structure>Lead with the rule itself, then a **Why:** line and a **How to apply:** line.</body_structure>
    <examples>
    user: don't mock the database in these tests — we got burned last quarter when mocked tests passed but the prod migration failed
    assistant: [saves team feedback memory: integration tests must hit a real database, not mocks. Team scope: this is a project testing policy, not a personal preference]

    user: stop summarizing what you just did at the end of every response, I can read the diff
    assistant: [saves private feedback memory: this user wants terse responses with no trailing summaries. Private because it's a communication preference, not a project convention]
    </examples>
</type>
<type>
    <name>project</name>
    <scope>private or team, but strongly bias toward team</scope>
    <description>Information that you learn about ongoing work, goals, initiatives, bugs, or incidents within the project that is not otherwise derivable from the code or git history.</description>
    <when_to_save>When you learn who is doing what, why, or by when. Always convert relative dates to absolute dates when saving.</when_to_save>
    <how_to_use>Use these memories to more fully understand the details and nuance behind the user's request, anticipate coordination issues across users, make better informed suggestions.</how_to_use>
    <body_structure>Lead with the fact or decision, then a **Why:** line and a **How to apply:** line.</body_structure>
    <examples>
    user: we're freezing all non-critical merges after Thursday — mobile team is cutting a release branch
    assistant: [saves team project memory: merge freeze begins 2026-03-05 for mobile release cut. Flag any non-critical PR work scheduled after that date]

    user: the reason we're ripping out the old auth middleware is that legal flagged it for storing session tokens in a way that doesn't meet the new compliance requirements
    assistant: [saves team project memory: auth middleware rewrite is driven by legal/compliance requirements around session token storage, not tech-debt cleanup]
    </examples>
</type>
<type>
    <name>reference</name>
    <scope>usually team</scope>
    <description>Stores pointers to where information can be found in external systems.</description>
    <when_to_save>When you learn about resources in external systems and their purpose.</when_to_save>
    <how_to_use>When the user references an external system or information that may be in an external system.</how_to_use>
    <examples>
    user: check the Linear project "INGEST" if you want context on these tickets, that's where we track all pipeline bugs
    assistant: [saves team reference memory: pipeline bugs are tracked in Linear project "INGEST"]

    user: the Grafana board at grafana.internal/d/api-latency is what oncall watches — if you're touching request handling, that's the thing that'll page someone
    assistant: [saves team reference memory: grafana.internal/d/api-latency is the oncall latency dashboard — check it when editing request-path code]
    </examples>
</type>
</types>"#
        .to_string()
}

// ── Behavioral sections ────────────────────────────────────────────────

fn build_what_not_to_save_section() -> String {
    "## What NOT to save in memory\n\n\
     - Code patterns, conventions, architecture, file paths, or project structure — \
     these can be derived by reading the current project state.\n\
     - Git history, recent changes, or who-changed-what — `git log` / `git blame` are authoritative.\n\
     - Debugging solutions or fix recipes — the fix is in the code; the commit message has the context.\n\
     - Anything already documented in CLAUDE.md files.\n\
     - Ephemeral task details: in-progress work, temporary state, current conversation context.\n\
     \n\
     These exclusions apply even when the user explicitly asks you to save. If they ask you to \
     save a PR list or activity summary, ask what was *surprising* or *non-obvious* about it — \
     that is the part worth keeping."
        .to_string()
}

fn build_save_instructions(config: &MemoryConfig) -> String {
    if config.skip_index {
        return "## How to save memories\n\n\
                Write the memory to its own file (e.g., `user_role.md`, `feedback_testing.md`) \
                using markdown with YAML frontmatter:\n\n\
                ```markdown\n\
                ---\n\
                name: {{memory name}}\n\
                description: {{one-line description — used to decide relevance in future conversations, so be specific}}\n\
                type: {{user, feedback, project, reference}}\n\
                ---\n\n\
                {{memory content — for feedback/project types, structure as: rule/fact, then **Why:** and **How to apply:** lines}}\n\
                ```"
        .to_string();
    }

    "## How to save memories\n\n\
     Saving a memory is a two-step process:\n\n\
     **Step 1** — write the memory to its own file (e.g., `user_role.md`, `feedback_testing.md`) \
     using this frontmatter format:\n\n\
     ```markdown\n\
     ---\n\
     name: {{memory name}}\n\
     description: {{one-line description — used to decide relevance in future conversations, so be specific}}\n\
     type: {{user, feedback, project, reference}}\n\
     ---\n\n\
     {{memory content — for feedback/project types, structure as: rule/fact, then **Why:** and **How to apply:** lines}}\n\
     ```\n\n\
     **Step 2** — add a pointer to that file in `MEMORY.md`. `MEMORY.md` is an index, not a \
     memory — each entry should be one line, under ~150 characters: \
     `- [Title](file.md) — one-line hook`. It has no frontmatter.\n\n\
     - `MEMORY.md` is always loaded into your conversation context — lines after 200 \
     will be truncated, so keep the index concise\n\
     - Keep the name, description, and type fields in memory files up-to-date with the content\n\
     - Organize memory semantically by topic, not chronologically\n\
     - Update or remove memories that turn out to be wrong or outdated\n\
     - Do not write duplicate memories. First check if there is an existing memory \
     you can update before writing a new one."
        .to_string()
}

fn build_when_to_access_section() -> String {
    format!(
        "## When to access memories\n\
         - When memories seem relevant, or the user references prior-conversation work.\n\
         - You MUST access memory when the user explicitly asks you to check, recall, or remember.\n\
         - If the user says to *ignore* or *not use* memory: proceed as if MEMORY.md were empty. \
         Do not apply remembered facts, cite, compare against, or mention memory content.\n\
         - {MEMORY_DRIFT_CAVEAT}"
    )
}

/// The TRUSTING_RECALL_SECTION — verify before recommending from memory.
///
/// TS: memoryTypes.ts TRUSTING_RECALL_SECTION (lines 240-256).
fn build_trusting_recall_section() -> String {
    "## Before recommending from memory\n\n\
     A memory that names a specific function, file, or flag is a claim that it existed \
     *when the memory was written*. It may have been renamed, removed, or never merged. \
     Before recommending it:\n\n\
     - If the memory names a file path: check the file exists.\n\
     - If the memory names a function or flag: grep for it.\n\
     - If the user is about to act on your recommendation (not just asking about history), \
     verify first.\n\n\
     \"The memory says X exists\" is not the same as \"X exists now.\"\n\n\
     A memory that summarizes repo state (activity logs, architecture snapshots) is frozen \
     in time. If the user asks about *recent* or *current* state, prefer `git log` or \
     reading the code over recalling the snapshot."
        .to_string()
}

fn build_persistence_section() -> String {
    "## Memory and other forms of persistence\n\
     Memory is one of several persistence mechanisms available to you as you assist the user \
     in a given conversation. The distinction is often that memory can be recalled in future \
     conversations and should not be used for persisting information that is only useful within \
     the scope of the current conversation.\n\
     - When to use or update a plan instead of memory: If you are about to start a non-trivial \
     implementation task and would like to reach alignment with the user on your approach you \
     should use a Plan rather than saving this information to memory. Similarly, if you already \
     have a plan within the conversation and you have changed your approach persist that change \
     by updating the plan rather than saving a memory.\n\
     - When to use or update tasks instead of memory: When you need to break your work in \
     current conversation into discrete steps or keep track of your progress use tasks instead \
     of saving to memory. Tasks are great for persisting information about the work that needs \
     to be done in the current conversation, but memory should be reserved for information that \
     will be useful in future conversations."
        .to_string()
}

// ── Truncation ─────────────────────────────────────────────────────────

/// Truncate MEMORY.md content to fit within line and byte limits.
pub fn truncate_entrypoint_content(content: &str) -> String {
    let lines: Vec<&str> = content.lines().collect();
    let byte_len = content.len();

    if lines.len() <= MAX_ENTRYPOINT_LINES && byte_len <= MAX_ENTRYPOINT_BYTES {
        return content.to_string();
    }

    let mut result = String::with_capacity(MAX_ENTRYPOINT_BYTES);
    let mut line_count = 0;

    for line in &lines {
        if line_count >= MAX_ENTRYPOINT_LINES || result.len() + line.len() > MAX_ENTRYPOINT_BYTES {
            break;
        }
        if line_count > 0 {
            result.push('\n');
        }
        result.push_str(line);
        line_count += 1;
    }

    result.push_str("\n\n<!-- Memory index truncated. ");
    if lines.len() > MAX_ENTRYPOINT_LINES {
        result.push_str(&format!(
            "{} lines omitted (max {MAX_ENTRYPOINT_LINES}). ",
            lines.len() - line_count
        ));
    }
    result.push_str("Keep entries concise to stay within limits. -->");

    result
}

// ── Extraction prompt variants ─────────────────────────────────────────

/// Build the extraction agent prompt for auto-only mode.
///
/// TS: services/extractMemories/prompts.ts — buildExtractAutoOnlyPrompt.
pub fn build_extract_auto_only_prompt(
    message_count: i32,
    manifest: &str,
    skip_index: bool,
) -> String {
    let save_step = if skip_index {
        "Write memory files with YAML frontmatter (no MEMORY.md indexing needed)."
    } else {
        "Step 1: Write memory file with frontmatter. Step 2: Add pointer to MEMORY.md \
         (one-line index entry, <150 chars, no frontmatter in MEMORY.md)."
    };

    format!(
        "You are a memory extraction subagent. Analyze the last ~{message_count} messages \
         from the conversation and extract information worth remembering across sessions.\n\n\
         {manifest}\n\n\
         Check this list before writing — update existing files rather than creating duplicates.\n\n\
         Tools available: Read, Grep, Glob (unrestricted), Bash (read-only: ls, find, grep, cat, \
         stat, wc, head, tail), Edit/Write (memory directory only).\n\n\
         Strategy: issue all Read calls in parallel (turn 1), all writes in parallel (turn 2). \
         WARNING: Do not waste turns investigating — use only the last ~{message_count} messages.\n\n\
         {save_step}"
    )
}

/// Build the extraction agent prompt for combined (auto + team) mode.
pub fn build_extract_combined_prompt(
    message_count: i32,
    personal_manifest: &str,
    team_manifest: &str,
    skip_index: bool,
) -> String {
    let save_step = if skip_index {
        "Write memory files with frontmatter."
    } else {
        "Step 1: Write file. Step 2: Add pointer to the same directory's MEMORY.md."
    };

    format!(
        "You are a memory extraction subagent. Analyze the last ~{message_count} messages.\n\n\
         ## Personal Memories\n{personal_manifest}\n\n\
         ## Team Memories\n{team_manifest}\n\n\
         Check these lists before writing — update existing files rather than creating duplicates.\n\n\
         When saving, choose directory per type's <scope> guidance:\n\
         - **personal**: preferences, role, individual feedback → personal directory\n\
         - **team**: project decisions, conventions, references → team/ subdirectory\n\n\
         Warning: avoid saving sensitive data (API keys, credentials) in team memories.\n\n\
         {save_step}\n\n\
         Strategy: Read turn 1, Write turn 2."
    )
}

/// Build the KAIROS daily log prompt (assistant mode).
///
/// TS: memdir/memdir.ts — buildAssistantDailyLogPrompt.
pub fn build_daily_log_prompt(date: &str, memory_dir: &Path) -> String {
    let parts: Vec<&str> = date.split('-').collect();
    let (year, month) = match parts.as_slice() {
        [y, m, ..] => (*y, *m),
        _ => ("unknown", "00"),
    };

    format!(
        "You are in assistant mode. Instead of managing MEMORY.md directly, \
         append observations to today's daily log.\n\n\
         Log path: `{}/logs/{year}/{month}/{date}.md`\n\n\
         Format: short timestamped bullets, append-only.\n\
         The nightly consolidation process will distill logs into the memory index.\n\n\
         Only log information that would be useful for future context:\n\
         - Decisions made and their rationale\n\
         - User corrections or preferences discovered\n\
         - Project context that isn't in the codebase\n\
         - Pointers to external resources",
        memory_dir.display(),
    )
}

#[cfg(test)]
#[path = "prompt.test.rs"]
mod tests;
