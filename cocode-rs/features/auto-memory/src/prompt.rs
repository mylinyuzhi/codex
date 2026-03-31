//! Auto memory prompt generation.
//!
//! Builds the system prompt section that instructs the model on how to
//! use the persistent memory system. The prompt format closely aligns
//! with Claude Code's auto memory prompt, using XML-structured type
//! descriptions with detailed `when_to_save`, `how_to_use`, and
//! `examples` for each memory type.

use cocode_protocol::ToolName;

use crate::memory_file::MemoryIndex;

/// Build the full auto memory prompt with MEMORY.md content.
///
/// This is the primary prompt format injected into the system prompt
/// when auto memory is enabled. Includes memory types, save/access
/// guidelines, search context, and the loaded MEMORY.md content.
pub fn build_auto_memory_prompt(
    memory_dir: &str,
    index: Option<&MemoryIndex>,
    max_lines: i32,
) -> String {
    let memory_index_desc = format!(
        "`MEMORY.md` is always loaded into your conversation context — lines after {max_lines} \
         will be truncated, so keep the index concise.\n\n\
         Memory files are re-read from disk each turn, so changes are immediately visible.\n"
    );

    let mut parts = Vec::new();
    parts.push("# auto memory\n".to_string());
    let write_tool = ToolName::Write.as_str();
    parts.push(format!(
        "You have a persistent, file-based memory system at `{memory_dir}/`. \
         This directory already exists — write to it directly with the {write_tool} tool \
         (do not run mkdir or check for its existence).\n"
    ));
    parts.push(
        "You should build up this memory system over time so that future conversations can have \
         a complete picture of who the user is, how they'd like to collaborate with you, what \
         behaviors to avoid or repeat, and the context behind the work the user gives you.\n\n\
         If the user explicitly asks you to remember something, save it immediately as whichever \
         type fits best. If they ask you to forget something, find and remove the relevant entry.\n"
            .to_string(),
    );
    parts.push(memory_index_desc);
    parts.push(MEMORY_TYPES_SECTION.to_string());
    parts.push(WHAT_NOT_TO_SAVE_SECTION.to_string());
    parts.push(HOW_TO_SAVE.to_string());
    parts.push(WHEN_TO_ACCESS.to_string());
    parts.push(BEFORE_RECOMMENDING.to_string());
    parts.push(MEMORY_VS_PLAN_VS_TASK.to_string());
    parts.push(build_search_context_section(memory_dir));

    let index_content = build_memory_index_section(
        "claudeMd", memory_dir, index, max_lines, /*include_preamble*/ true,
    );
    if !index_content.is_empty() {
        parts.push(index_content);
    }

    parts.join("\n")
}

/// Build combined user+team memory prompt with typed frontmatter guidance.
///
/// Includes both user memory and team memory sections with structured
/// memory type descriptions (team-scoped) and team-typed save instructions.
pub fn build_typed_combined_memory_prompt(
    memory_dir: &str,
    team_dir: &str,
    index: Option<&MemoryIndex>,
    team_index: Option<&MemoryIndex>,
    max_lines: i32,
) -> String {
    let write_tool = ToolName::Write.as_str();

    let mut parts = Vec::new();
    parts.push("# auto memory\n".to_string());
    parts.push(format!(
        "You have a persistent, file-based memory system with two directories:\n\
         - **User memory** at `{memory_dir}/` — private to you and this user.\n\
         - **Team memory** at `{team_dir}/` — shared with all team members.\n\n\
         Both directories already exist — write to them directly with the {write_tool} tool \
         (do not run mkdir or check for their existence).\n"
    ));
    parts.push(
        "You should build up this memory system over time so that future conversations can have \
         a complete picture of who the user is, how they'd like to collaborate with you, what \
         behaviors to avoid or repeat, and the context behind the work the user gives you.\n\n\
         If the user explicitly asks you to remember something, save it immediately as whichever \
         type fits best. If they ask you to forget something, find and remove the relevant entry.\n"
            .to_string(),
    );
    parts.push(format!(
        "`MEMORY.md` is always loaded into your conversation context — lines after {max_lines} \
         will be truncated, so keep the index concise.\n\n\
         Memory files are re-read from disk each turn, so changes are immediately visible.\n"
    ));
    parts.push(MEMORY_TYPES_SECTION_WITH_TEAM_SCOPE.to_string());
    parts.push(build_user_vs_team_guidance_section());
    parts.push(WHAT_NOT_TO_SAVE_SECTION.to_string());
    parts.push(HOW_TO_SAVE_TEAM_TYPED.to_string());
    parts.push(WHEN_TO_ACCESS.to_string());
    parts.push(BEFORE_RECOMMENDING.to_string());
    parts.push(MEMORY_VS_PLAN_VS_TASK.to_string());
    parts.push(build_search_context_section(memory_dir));
    parts.push(build_team_search_context_section(team_dir));
    parts.push(build_memory_index_section(
        "claudeMd", memory_dir, index, max_lines, /*include_preamble*/ true,
    ));
    parts.push(build_memory_index_section(
        "Team Memory",
        team_dir,
        team_index,
        max_lines,
        /*include_preamble*/ false,
    ));

    parts.join("\n")
}

/// Build extraction-mode combined prompt (read-only with background extraction).
///
/// The main agent gets a read-only view of both user and team memory,
/// with extraction handled by a background subagent.
pub fn build_extract_mode_typed_combined_prompt(
    memory_dir: &str,
    team_dir: &str,
    max_lines: i32,
) -> String {
    format!(
        "# auto memory\n\n\
         You have a persistent, file-based memory system with two directories:\n\
         - **User memory** at `{memory_dir}/` — private to you and this user.\n\
         - **Team memory** at `{team_dir}/` — shared with all team members.\n\n\
         `MEMORY.md` in each directory is an index of memory files, loaded into your \
         conversation context (first {max_lines} lines). Use them to find relevant notes \
         from prior sessions.\n\n\
         A background agent automatically extracts and saves memories from this conversation.\n\
         If the user asks you to remember or forget something, acknowledge it — the save happens automatically.\n\
         You should not write to memory files yourself.\n\n\
         ## When to access memories\n\
         - When specific known memories seem relevant to the task at hand.\n\
         - When the user seems to be referring to work you may have done in a prior conversation.\n\
         - You MUST access memory when the user explicitly asks you to check your memory, recall, or remember.\n\n\
         {}\n\
         {}\n",
        build_search_context_section(memory_dir),
        build_team_search_context_section(team_dir),
    )
}

/// Build a read-only memory prompt for background agents.
///
/// Background agents should not write memory files — a separate
/// extraction subagent handles that automatically.
pub fn build_background_agent_memory_prompt(memory_dir: &str, max_lines: i32) -> String {
    format!(
        "# auto memory\n\n\
         You have a persistent, file-based memory system at `{memory_dir}`.\n\n\
         `MEMORY.md` is an index of memory files, loaded into your conversation context \
         (first {max_lines} lines). Use it to find relevant notes from prior sessions.\n\n\
         A background agent automatically extracts and saves memories from this conversation.\n\
         If the user asks you to remember or forget something, acknowledge it — the save happens automatically.\n\
         You should not write to memory files yourself.\n\n\
         ## When to access memories\n\
         - When specific known memories seem relevant to the task at hand.\n\
         - When the user seems to be referring to work you may have done in a prior conversation.\n\
         - You MUST access memory when the user explicitly asks you to check your memory, recall, or remember.\n\n\
         {}\n",
        build_search_context_section(memory_dir)
    )
}

/// Build the search context section that teaches the agent how to
/// search memory files using Read and Grep tools.
fn build_search_context_section(memory_dir: &str) -> String {
    let read_tool = ToolName::Read.as_str();
    let grep_tool = ToolName::Grep.as_str();
    format!(
        "## Searching memory files\n\
         - Check `MEMORY.md` first for an overview, then read specific topic files as needed.\n\
         - Use the {read_tool} tool to read individual memory files referenced in MEMORY.md.\n\
         - Use the {grep_tool} tool to search across all memory files in `{memory_dir}`.\n\
         - Memory file names indicate their topic (e.g., `feedback_testing.md`, `project_auth.md`).\n"
    )
}

// ========================================================================
// Extraction Prompt Variants (2x2 matrix: team x typed)
// ========================================================================

/// Build extraction subagent prompt - standard single memory.
///
/// The default extraction prompt for a single user memory directory.
/// Includes save triggers, content guidelines, and exclusions.
pub fn build_extraction_prompt_standard(message_count: i32) -> String {
    let parts = [
        extraction_preamble(message_count),
        EXTRACTION_WHEN_TO_SAVE.to_string(),
        EXTRACTION_WHAT_TO_SAVE.to_string(),
        WHAT_NOT_TO_SAVE_SECTION.to_string(),
        EXTRACTION_EXPLICIT_REQUESTS.to_string(),
        HOW_TO_SAVE.to_string(),
    ];
    parts.join("\n")
}

/// Build extraction subagent prompt - typed single memory (with frontmatter format).
///
/// Uses structured memory types with YAML frontmatter for organized storage.
pub fn build_extraction_prompt_typed(message_count: i32) -> String {
    let parts = [
        extraction_preamble(message_count),
        MEMORY_TYPES_SECTION.to_string(),
        WHAT_NOT_TO_SAVE_SECTION.to_string(),
        HOW_TO_SAVE.to_string(),
    ];
    parts.join("\n")
}

/// Build extraction subagent prompt - team mode (user + team distinction).
///
/// Guides the extraction agent on routing memories to the correct
/// directory: user (private) vs team (shared).
pub fn build_extraction_prompt_team(message_count: i32) -> String {
    let parts = [
        extraction_preamble(message_count),
        EXTRACTION_WHEN_TO_SAVE.to_string(),
        EXTRACTION_WHAT_TO_SAVE_USER.to_string(),
        EXTRACTION_WHAT_TO_SAVE_TEAM.to_string(),
        EXTRACTION_CHOOSING_USER_VS_TEAM.to_string(),
        WHAT_NOT_TO_SAVE_SECTION.to_string(),
        EXTRACTION_EXPLICIT_REQUESTS.to_string(),
        HOW_TO_SAVE.to_string(),
    ];
    parts.join("\n")
}

/// Build extraction subagent prompt - typed team mode (both typed format and user/team).
///
/// Combines structured memory types with user/team routing guidance.
pub fn build_extraction_prompt_typed_team(message_count: i32) -> String {
    let parts = [
        extraction_preamble(message_count),
        MEMORY_TYPES_SECTION_WITH_TEAM_SCOPE.to_string(),
        EXTRACTION_CHOOSING_USER_VS_TEAM.to_string(),
        WHAT_NOT_TO_SAVE_SECTION.to_string(),
        HOW_TO_SAVE_TEAM_TYPED.to_string(),
    ];
    parts.join("\n")
}

/// Shared preamble for all extraction prompt variants.
fn extraction_preamble(message_count: i32) -> String {
    format!(
        "You are now acting as the memory extraction subagent. \
         Any prior instruction to not write memory files applies to the main conversation — \
         in this role, writing is your job. Analyze the most recent ~{message_count} messages above \
         and use them to update your persistent memory systems."
    )
}

// ========================================================================
// Helper functions for prompt composition
// ========================================================================

/// Build the user-vs-team guidance section for combined prompts.
fn build_user_vs_team_guidance_section() -> String {
    "\
## Choosing between user memory and team memory

- **User memory** (private): personal preferences, role details, feedback on your behavior, \
individual workflow habits. Only you and this user will see these.
- **Team memory** (shared): project decisions, architecture context, deployment procedures, \
shared conventions, team agreements. All team members benefit from these.

When in doubt, prefer team memory for project-related facts and user memory for personal \
preferences. If a memory is both personal and project-relevant, save the project parts to \
team memory and the personal parts to user memory.\n"
        .to_string()
}

/// Build the team search context section.
fn build_team_search_context_section(team_dir: &str) -> String {
    let read_tool = ToolName::Read.as_str();
    let grep_tool = ToolName::Grep.as_str();
    format!(
        "## Searching team memory files\n\
         - Check `MEMORY.md` in `{team_dir}` for the team memory overview.\n\
         - Use the {read_tool} tool to read individual team memory files.\n\
         - Use the {grep_tool} tool to search across all team memory files in `{team_dir}`.\n"
    )
}

/// Build a MEMORY.md index content section.
///
/// Unified builder for both user and team memory index sections.
/// `heading` is the section heading (e.g. "claudeMd" or "Team Memory"),
/// `dir` is the memory directory path, `max_lines` controls the
/// truncation threshold, and `include_preamble` adds the CLAUDE.md-style
/// override instructions when true.
fn build_memory_index_section(
    heading: &str,
    dir: &str,
    index: Option<&MemoryIndex>,
    max_lines: i32,
    include_preamble: bool,
) -> String {
    match index {
        Some(idx) if !idx.raw_content.trim().is_empty() => {
            let mut s = String::new();
            s.push_str(&format!("# {heading}\n"));
            if include_preamble {
                s.push_str(
                    "Codebase and user instructions are shown below. Be sure to adhere to these \
                     instructions. IMPORTANT: These instructions OVERRIDE any default behavior \
                     and you MUST follow them exactly as written.\n\n",
                );
            } else {
                s.push('\n');
            }
            s.push_str(&format!("Contents of {dir}/MEMORY.md:\n\n"));
            s.push_str(&idx.raw_content);
            if idx.was_truncated {
                s.push_str(&format!(
                    "\n\nIMPORTANT: MEMORY.md has {line_count} lines but only the first \
                     {max_lines} are shown. Keep it concise to stay under the limit.",
                    line_count = idx.line_count,
                ));
            }
            s
        }
        Some(_) => {
            format!(
                "Your `MEMORY.md` at `{dir}/MEMORY.md` is currently empty. \
                 When you save new memories, they will appear here.\n"
            )
        }
        None => String::new(),
    }
}

// ========================================================================
// Extraction-specific constants
// ========================================================================

const EXTRACTION_WHEN_TO_SAVE: &str = "\
## You MUST save memories when:

- The user shares information about themselves (role, preferences, expertise)
- The user gives feedback on your behavior (corrections or confirmations)
- You learn project context not derivable from code (decisions, deadlines, stakeholders)
- The user mentions external resources or references
- The user explicitly asks you to remember something\n";

const EXTRACTION_WHAT_TO_SAVE: &str = "\
## What to save in memories:

- User profile details (role, expertise, preferences)
- Behavioral feedback (what to do, what to avoid, confirmed approaches)
- Project context (decisions, timelines, stakeholder constraints)
- External references (tools, dashboards, documentation links)
- Non-obvious conventions or agreements\n";

const EXTRACTION_EXPLICIT_REQUESTS: &str = "\
## Explicit user requests:

If the user explicitly asks to remember something, save it immediately using the most \
appropriate memory type. If they ask to forget something, find and remove the relevant \
entry from both the memory file and the MEMORY.md index.\n";

const EXTRACTION_WHAT_TO_SAVE_USER: &str = "\
## What to save in user memory (private):

- Personal preferences and workflow habits
- Role, expertise, and knowledge level
- Feedback on assistant behavior (corrections and confirmations)
- Individual goals and responsibilities
- Communication style preferences\n";

const EXTRACTION_WHAT_TO_SAVE_TEAM: &str = "\
## What to save in team memory (shared):

- Project decisions and architecture context
- Deployment procedures and operational knowledge
- Shared conventions and coding standards
- Team agreements and process decisions
- Cross-cutting concerns (compliance, security policies)
- External system references used by the team\n";

const EXTRACTION_CHOOSING_USER_VS_TEAM: &str = "\
## Choosing between user memory and team memory:

- **User memory**: anything personal — preferences, role details, feedback on your behavior, \
individual workflow habits. Only this user benefits from these.
- **Team memory**: anything project-related — decisions, architecture, conventions, procedures. \
All team members benefit from these.

When in doubt, prefer team memory for project facts and user memory for personal preferences. \
Never store sensitive personal information in team memory (API keys, personal contacts, \
private feedback about specific team members).\n";

const MEMORY_TYPES_SECTION_WITH_TEAM_SCOPE: &str = "\
## Types of memory

There are several discrete types of memory. Each type can be stored in either the \
user directory (private) or the team directory (shared), depending on the content.

<types>
<type>
    <name>user</name>
    <scope>Usually private (user directory)</scope>
    <description>Information about the user's role, goals, responsibilities, and knowledge.</description>
    <when_to_save>When you learn details about the user's role, preferences, or knowledge</when_to_save>
</type>
<type>
    <name>feedback</name>
    <scope>Usually private (user directory)</scope>
    <description>Guidance the user has given about how to approach work — corrections and confirmations.</description>
    <when_to_save>When the user corrects your approach or confirms a non-obvious approach worked</when_to_save>
</type>
<type>
    <name>project</name>
    <scope>Usually shared (team directory)</scope>
    <description>Information about ongoing work, goals, decisions, or incidents not derivable from code.</description>
    <when_to_save>When you learn who is doing what, why, or by when</when_to_save>
</type>
<type>
    <name>reference</name>
    <scope>Shared (team directory) unless personal</scope>
    <description>Pointers to where information can be found in external systems.</description>
    <when_to_save>When you learn about resources in external systems</when_to_save>
</type>
</types>\n";

const HOW_TO_SAVE_TEAM_TYPED: &str = "\
## How to save memories

Saving a memory is a two-step process:

**Step 1** — write the memory to its own file in the chosen directory \
(user directory for private memories, team directory for shared memories). \
Use this frontmatter format:

```markdown
---
name: {{memory name}}
description: {{one-line description — used to decide relevance in future conversations, so be specific}}
type: {{user, feedback, project, reference}}
---

{{memory content}}
```

**Step 2** — add a pointer to that file in the same directory's `MEMORY.md`. \
`MEMORY.md` is an index, not a memory — it should contain only links to memory files \
with brief descriptions. It has no frontmatter. Never write memory content directly \
into `MEMORY.md`.

- Keep the name, description, and type fields in memory files up-to-date with the content
- Organize memory semantically by topic, not chronologically
- Update or remove memories that turn out to be wrong or outdated
- Do not write duplicate memories. First check if there is an existing memory you can update.\n";

// ========================================================================
// Existing constants
// ========================================================================

const MEMORY_TYPES_SECTION: &str = "\
## Types of memory

There are several discrete types of memory that you can store in your memory system:

<types>
<type>
    <name>user</name>
    <description>Contain information about the user's role, goals, responsibilities, and knowledge. \
Great user memories help you tailor your future behavior to the user's preferences and perspective. \
Your goal in reading and writing these memories is to build up an understanding of who the user is and \
how you can be most helpful to them specifically. For example, you should collaborate with a senior \
software engineer differently than a student who is coding for the very first time. Keep in mind, that \
the aim here is to be helpful to the user. Avoid writing memories about the user that could be viewed \
as a negative judgement or that are not relevant to the work you're trying to accomplish together.</description>
    <when_to_save>When you learn any details about the user's role, preferences, responsibilities, or knowledge</when_to_save>
    <how_to_use>When your work should be informed by the user's profile or perspective. For example, if \
the user is asking you to explain a part of the code, you should answer that question in a way that is \
tailored to the specific details that they will find most valuable or that helps them build their mental \
model in relation to domain knowledge they already have.</how_to_use>
    <examples>
    user: I'm a data scientist investigating what logging we have in place
    assistant: [saves user memory: user is a data scientist, currently focused on observability/logging]

    user: I've been writing Go for ten years but this is my first time touching the React side of this repo
    assistant: [saves user memory: deep Go expertise, new to React and this project's frontend — frame frontend explanations in terms of backend analogues]
    </examples>
</type>
<type>
    <name>feedback</name>
    <description>Guidance the user has given you about how to approach work — both what to avoid and what \
to keep doing. These are a very important type of memory to read and write as they allow you to remain \
coherent and responsive to the way you should approach work in the project. Record from failure AND success: \
if you only save corrections, you will avoid past mistakes but drift away from approaches the user has already \
validated, and may grow overly cautious.</description>
    <when_to_save>Any time the user corrects your approach (\"no not that\", \"don't\", \"stop doing X\") OR \
confirms a non-obvious approach worked (\"yes exactly\", \"perfect, keep doing that\", accepting an unusual \
choice without pushback). Corrections are easy to notice; confirmations are quieter — watch for them. In both \
cases, save what is applicable to future conversations, especially if surprising or not obvious from the code. \
Include *why* so you can judge edge cases later.</when_to_save>
    <how_to_use>Let these memories guide your behavior so that the user does not need to offer the same guidance twice.</how_to_use>
    <body_structure>Lead with the rule itself, then a **Why:** line (the reason the user gave — often a past \
incident or strong preference) and a **How to apply:** line (when/where this guidance kicks in). Knowing \
*why* lets you judge edge cases instead of blindly following the rule.</body_structure>
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
    <description>Information that you learn about ongoing work, goals, initiatives, bugs, or incidents \
within the project that is not otherwise derivable from the code or git history. Project memories help you \
understand the broader context and motivation behind the work the user is doing within this working directory.</description>
    <when_to_save>When you learn who is doing what, why, or by when. These states change relatively quickly \
so try to keep your understanding of this up to date. Always convert relative dates in user messages to \
absolute dates when saving (e.g., \"Thursday\" → \"2026-03-05\"), so the memory remains interpretable after \
time passes.</when_to_save>
    <how_to_use>Use these memories to more fully understand the details and nuance behind the user's request and make better informed suggestions.</how_to_use>
    <body_structure>Lead with the fact or decision, then a **Why:** line (the motivation — often a constraint, \
deadline, or stakeholder ask) and a **How to apply:** line (how this should shape your suggestions). Project \
memories decay fast, so the why helps future-you judge whether the memory is still load-bearing.</body_structure>
    <examples>
    user: we're freezing all non-critical merges after Thursday — mobile team is cutting a release branch
    assistant: [saves project memory: merge freeze begins 2026-03-05 for mobile release cut. Flag any non-critical PR work scheduled after that date]

    user: the reason we're ripping out the old auth middleware is that legal flagged it for storing session tokens in a way that doesn't meet the new compliance requirements
    assistant: [saves project memory: auth middleware rewrite is driven by legal/compliance requirements around session token storage, not tech-debt cleanup — scope decisions should favor compliance over ergonomics]
    </examples>
</type>
<type>
    <name>reference</name>
    <description>Stores pointers to where information can be found in external systems. These memories \
allow you to remember where to look to find up-to-date information outside of the project directory.</description>
    <when_to_save>When you learn about resources in external systems and their purpose. For example, that \
bugs are tracked in a specific project in Linear or that feedback can be found in a specific Slack channel.</when_to_save>
    <how_to_use>When the user references an external system or information that may be in an external system.</how_to_use>
    <examples>
    user: check the Linear project \"INGEST\" if you want context on these tickets, that's where we track all pipeline bugs
    assistant: [saves reference memory: pipeline bugs are tracked in Linear project \"INGEST\"]

    user: the Grafana board at grafana.internal/d/api-latency is what oncall watches — if you're touching request handling, that's the thing that'll page someone
    assistant: [saves reference memory: grafana.internal/d/api-latency is the oncall latency dashboard — check it when editing request-path code]
    </examples>
</type>
</types>\n";

const WHAT_NOT_TO_SAVE_SECTION: &str = "\
## What NOT to save in memory

- Code patterns, conventions, architecture, file paths, or project structure — these can be derived by reading the current project state.
- Git history, recent changes, or who-changed-what — `git log` / `git blame` are authoritative.
- Debugging solutions or fix recipes — the fix is in the code; the commit message has the context.
- Anything already documented in CLAUDE.md files.
- Ephemeral task details: in-progress work, temporary state, current conversation context.

These exclusions apply even when the user explicitly asks you to save. If they ask you to save a PR list \
or activity summary, ask what was *surprising* or *non-obvious* about it — that is the part worth keeping.\n";

const HOW_TO_SAVE: &str = "\
## How to save memories

Saving a memory is a two-step process:

**Step 1** — write the memory to its own file (e.g., `user_role.md`, `feedback_testing.md`) using this frontmatter format:

```markdown
---
name: {{memory name}}
description: {{one-line description — used to decide relevance in future conversations, so be specific}}
type: {{user, feedback, project, reference}}
---

{{memory content — for feedback/project types, structure as: rule/fact, then **Why:** and **How to apply:** lines}}
```

**Step 2** — add a pointer to that file in `MEMORY.md`. `MEMORY.md` is an index, not a memory — \
it should contain only links to memory files with brief descriptions. It has no frontmatter. \
Never write memory content directly into `MEMORY.md`.

- Keep the name, description, and type fields in memory files up-to-date with the content
- Organize memory semantically by topic, not chronologically
- Update or remove memories that turn out to be wrong or outdated
- Do not write duplicate memories. First check if there is an existing memory you can update before writing a new one.\n";

const WHEN_TO_ACCESS: &str = "\
## When to access memories
- When memories seem relevant, or the user references prior-conversation work.
- You MUST access memory when the user explicitly asks you to check, recall, or remember.
- If the user asks you to *ignore* memory: don't cite, compare against, or mention it — answer as if absent.
- Memory records can become stale over time. Use memory as context for what was true at a given point in time. \
Before answering the user or building assumptions based solely on information in memory records, verify that \
the memory is still correct and up-to-date by reading the current state of the files or resources. If a \
recalled memory conflicts with current information, trust what you observe now — and update or remove the stale memory.\n";

const BEFORE_RECOMMENDING: &str = "\
## Before recommending from memory

A memory that names a specific function, file, or flag is a claim that it existed *when the memory was written*. \
It may have been renamed, removed, or never merged. Before recommending it:

- If the memory names a file path: check the file exists.
- If the memory names a function or flag: grep for it.
- If the user is about to act on your recommendation (not just asking about history), verify first.

\"The memory says X exists\" is not the same as \"X exists now.\"

A memory that summarizes repo state (activity logs, architecture snapshots) is frozen in time. \
If the user asks about *recent* or *current* state, prefer `git log` or reading the code over recalling the snapshot.\n";

const MEMORY_VS_PLAN_VS_TASK: &str = "\
## Memory and other forms of persistence
Memory is one of several persistence mechanisms available to you as you assist the user in a given conversation. \
The distinction is often that memory can be recalled in future conversations and should not be used for persisting \
information that is only useful within the scope of the current conversation.
- When to use or update a plan instead of memory: If you are about to start a non-trivial implementation task \
and would like to reach alignment with the user on your approach you should use a Plan rather than saving this \
information to memory. Similarly, if you already have a plan within the conversation and you have changed your \
approach persist that change by updating the plan rather than saving a memory.
- When to use or update tasks instead of memory: When you need to break your work in current conversation into \
discrete steps or keep track of your progress use tasks instead of saving to memory. Tasks are great for persisting \
information about the work that needs to be done in the current conversation, but memory should be reserved for \
information that will be useful in future conversations.\n";

#[cfg(test)]
#[path = "prompt.test.rs"]
mod tests;
