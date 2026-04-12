# coco-memory — Crate Plan

TS source: `src/memdir/` (507 LOC), `src/services/extractMemories/`, `src/services/SessionMemory/`, `src/services/autoDream/`

## Dependencies

```
coco-memory depends on:
  - coco-types (Message), coco-inference (ApiClient — LLM for auto-extraction + session memory + auto-dream)
  - coco-config (Settings — autoDreamEnabled, session memory config)
  - utils/frontmatter (YAML frontmatter parsing)
  - utils/git (canonical git-root resolution for memory path)

coco-memory does NOT depend on:
  - coco-tools, coco-query, any app/ crate
```

## Data Definitions

```rust
/// 4-type taxonomy with scope rules (from memdir/memoryTypes.ts 271 LOC):
///   User: role, preferences, knowledge → tailor behavior
///   Feedback: corrections AND confirmations → avoid repeating mistakes
///   Project: goals, decisions, deadlines → understand context (convert relative dates)
///   Reference: pointers to external systems → know where to look
pub enum MemoryEntryType { User, Feedback, Project, Reference }

pub struct MemoryEntry {
    pub name: String,
    pub description: String,        // One-line, used for relevance matching
    pub entry_type: MemoryEntryType,
    pub content: String,
    pub file_path: PathBuf,
}

/// MEMORY.md index: one-line pointers, max 200 lines.
/// Lines after 200 truncated with warning.
/// Max 25KB total size for MEMORY.md.
pub struct MemoryIndex {
    pub entries: Vec<MemoryIndexEntry>,
}

pub struct MemoryIndexEntry {
    pub title: String,
    pub file: String,     // Relative path to .md file
    pub hook: String,     // One-line description (<150 chars)
}

/// Session memory configuration thresholds.
pub struct SessionMemoryConfig {
    /// Minimum accumulated message tokens before first extraction (default 10000).
    pub minimum_message_tokens_to_init: i64,
    /// Minimum new tokens since last update before re-extraction allowed (default 5000).
    pub minimum_tokens_between_update: i64,
    /// Minimum tool calls since last update before re-extraction allowed (default 3).
    pub tool_calls_between_updates: i64,
}

/// Per-section budget for session memory template.
pub struct SessionMemorySectionBudget {
    /// Max tokens per section (default 2000).
    pub per_section: i64,
    /// Max total tokens across all 9 sections (default 12000).
    pub total: i64,
}
```

## Core Logic

```rust
pub struct MemoryManager {
    pub memory_dir: PathBuf,  // ~/.coco/projects/<hash>/memory/
    pub index: MemoryIndex,
}

impl MemoryManager {
    /// Load MEMORY.md index + all referenced memory files.
    /// Truncation: If MEMORY.md exceeds 200 lines or 25KB, truncate with warning.
    pub fn load(project_dir: &Path) -> Self;

    /// Two-step save process:
    /// 1. Write memory content to its own file (e.g., feedback_testing.md)
    /// 2. Add one-line pointer to MEMORY.md index
    /// Dedup: check existing memories before creating new one.
    pub fn save(&mut self, entry: MemoryEntry) -> Result<(), MemoryError>;
    pub fn delete(&mut self, name: &str) -> Result<(), MemoryError>;

    /// Auto-extract memories from conversation (LLM call via ApiClient).
    pub async fn auto_extract(&mut self, messages: &[Message], api: &ApiClient) -> Vec<MemoryEntry>;

    /// Staleness detection (from memdir/memoryAge.ts 53 LOC):
    /// Human-readable age ("47 days ago").
    /// Caveat text for memories >1 day old: "this memory may be stale".
    pub fn memory_age(entry: &MemoryEntry) -> String;
    pub fn memory_freshness_text(entry: &MemoryEntry) -> Option<String>;

    /// Sonnet-based recall selector (from memdir/findRelevantMemories.ts 141 LOC):
    /// Filters 5 most relevant memory files based on current context.
    pub async fn find_relevant_memories(
        &self,
        context: &str,
        api: &ApiClient,
    ) -> Vec<MemoryEntry>;
}
```

## Session Memory

TS source: `src/services/SessionMemory/` (~600 LOC)

Session memory persists a structured summary of the current conversation to disk,
enabling context recovery after compaction or session resume.

### Trigger Logic

Extraction fires when BOTH conditions are met:
1. Token gate: accumulated message tokens >= `minimum_tokens_between_update` (5000) since last update
   (or >= `minimum_message_tokens_to_init` (10000) for first extraction).
2. Activity gate: one of:
   - Tool call gate: tool calls since last update >= `tool_calls_between_updates` (3), OR
   - Natural break: token gate met AND zero tool calls in the interval (conversation pause).

Manual trigger: `/summary` command bypasses all gates and runs extraction immediately.

### Storage

- Directory: `~/.coco/session-memory/` (TS: `~/.claude/session-memory/`)
- File permissions: `0o600` (owner read/write only)
- Directory permissions: `0o700` (owner only)
- One file per session, keyed by session ID.

### 9-Section Template

The extraction prompt produces a structured document with these sections:

1. **Session Title** -- concise title for the session
2. **Current State** -- what the agent is currently doing or last completed
3. **Task Specification** -- the user's original request and refined understanding
4. **Files and Functions** -- key files/functions touched or referenced
5. **Workflow** -- steps taken, sequence of operations
6. **Errors & Corrections** -- mistakes made, how they were fixed
7. **Codebase Documentation** -- discovered architecture, patterns, conventions
8. **Learnings** -- insights about the codebase or user preferences
9. **Key Results & Worklog** -- concrete outputs, changes made, remaining work

Section-size budget: 2000 tokens per section, 12000 tokens total across all sections.

### Custom Template/Prompt Override

Users can override the default template and extraction prompt:
- `~/.coco/session-memory/config/template.md` -- custom section template
- `~/.coco/session-memory/config/prompt.md` -- custom extraction system prompt

If present, these replace the built-in defaults entirely.

### Compaction Integration

`truncate_session_memory_for_compact()`: when the query loop triggers compaction,
session memory is truncated to fit within the post-compact token budget. The
session memory content is injected as context for the compaction summary so the
model retains awareness of earlier work.

### Async Extraction

`wait_for_session_memory_extraction()`: blocks up to 15 seconds for an in-progress
extraction to complete. Called during session save and shutdown to avoid data loss.

```rust
pub struct SessionMemoryManager {
    config: SessionMemoryConfig,
    session_dir: PathBuf,
    last_extraction_tokens: i64,
    last_extraction_tool_calls: i64,
}

impl SessionMemoryManager {
    /// Check trigger conditions and run extraction if met.
    pub async fn maybe_extract(
        &mut self,
        messages: &[Message],
        current_tokens: i64,
        current_tool_calls: i64,
        api: &ApiClient,
    ) -> Option<String>;

    /// Force extraction regardless of gates (for /summary command).
    pub async fn force_extract(
        &mut self,
        messages: &[Message],
        api: &ApiClient,
    ) -> String;

    /// Truncate session memory to fit compact budget.
    pub fn truncate_for_compact(&self, content: &str, budget_tokens: i64) -> String;

    /// Block until pending extraction completes (15s timeout).
    pub async fn wait_for_extraction(&self) -> Result<(), TimeoutError>;
}
```

## Auto-Dream (Consolidation)

TS source: `src/services/autoDream/` (~400 LOC)

Auto-dream is a background consolidation process that periodically merges and prunes
accumulated memory files across sessions. It runs as a forked background agent.

### Three-Gate Scheduling

All three gates must pass before consolidation starts:

1. **Time gate**: at least `min_hours` (default 24h) since last consolidation.
   `lastConsolidatedAt` is read from the lock file's mtime.
2. **Scan throttle**: at most one scan attempt per 10 minutes (prevents busy-loop
   on rapid session starts).
3. **Session gate**: at least `min_sessions` (default 5) sessions since last
   consolidation.

### Lock File Protocol

- Location: `.consolidate-lock` inside the memory directory.
- The lock file's **mtime** serves as `lastConsolidatedAt` timestamp.
- The lock file's **body** contains the holder's PID.
- Dead-PID reclaim: if the lock is held but the PID is not running AND the lock
  is older than 1 hour, it is considered stale and can be reclaimed.
- `rollback_consolidation_lock()`: if the forked consolidation agent fails,
  the lock file mtime is rolled back to its previous value so the next attempt
  does not wait another full `min_hours` interval.

### Four-Phase Prompt

The consolidation agent executes a structured four-phase workflow:

1. **Orient** -- read MEMORY.md index, understand the memory landscape.
2. **Gather** -- read all referenced memory files, identify overlaps and conflicts.
3. **Consolidate** -- merge related entries, resolve contradictions, update content.
4. **Prune and index** -- delete redundant files, rewrite MEMORY.md index.

### Exclusions

- KAIROS mode: auto-dream is disabled when running in KAIROS mode.
- Remote mode: auto-dream is disabled for remote sessions.

### Configuration

User-configurable via `settings.json`:
- `auto_dream_enabled: bool` (default true) -- set to false to disable entirely.

### UI Integration

`DreamTask`: a cancelable background task that surfaces consolidation progress in the
TUI. The user can cancel an in-progress consolidation without corrupting state (the
lock file is rolled back).

## Auto-Extraction Forked Agent

TS source: `src/services/extractMemories/` (~500 LOC)

The auto-extraction agent runs as a forked background process after each turn,
analyzing the conversation for extractable memories.

### Mutual Exclusion

`has_memory_writes_since()`: before starting extraction, check whether any memory
writes have occurred since the last extraction. If another agent or the user has
written to memory files, skip this extraction cycle to avoid conflicts.

### Cursor Tracking

- Each extraction run tracks its position via a UUID-based cursor.
- Per-turn incremental: only messages since the last cursor are analyzed.
- The cursor is persisted so extraction resumes correctly after interruption.

### Throttle Gate

`turns_since_last_extraction` counter: extraction does not run on every turn.
The counter must exceed the threshold before the agent is spawned.

### Stash-and-Trailing-Run Pattern

When extraction starts, pending messages are "stashed" (snapshot of conversation
state at extraction start). If new messages arrive during extraction, a trailing
run is scheduled to process the gap between the stash point and current state.

### Shutdown Protocol

`drain_pending_extraction()`: on session shutdown, wait up to 60 seconds for
any in-progress extraction to complete. This prevents data loss when the user
exits mid-extraction.

### Tool Sandbox

The forked extraction agent has a restricted tool set:
- **Allowed**: Read, Grep, Glob, Bash (read-only), Write and Edit (within memdir only)
- Write/Edit operations are path-restricted to the memory directory.
- **Hard limit**: maximum 5 turns before the agent is forcibly terminated.

## Memory Path Resolution and Security

### Canonical Git-Root Resolution

Memory directories are anchored to the project's git root. For git worktrees,
the resolution follows symlinks to the shared `.git` directory so that all
worktrees of the same repo share the same memory directory.

### Path Security

Memory path construction rejects the following:
- **Null bytes**: any path containing `\0` is rejected.
- **UNC paths**: Windows UNC paths (`\\server\share`) are rejected.
- **Drive-root paths**: bare drive roots (`C:\`) are rejected to prevent writing
  to filesystem root.
- **Tilde expansion**: `~` is expanded to the actual home directory; unexpanded
  tildes in stored paths are rejected.

### Enable/Disable Priority

`is_auto_memory_enabled()` checks in priority order (first match wins):
1. Environment variable override (e.g., `COCO_AUTO_MEMORY=0`)
2. `--bare` CLI flag (disables memory)
3. CCR no-storage mode (disables memory)
4. `settings.json` `auto_memory_enabled` field
5. Default: enabled

### KAIROS Daily Log

In KAIROS mode, memories are written as daily logs instead of the standard
taxonomy. Path format: `logs/YYYY/MM/YYYY-MM-DD.md` within the memory directory.

## Memory Scanning

TS source: `src/memdir/memoryFiles.ts`

### scanMemoryFiles()

Recursively scans the memory directory for `.md` files:
- **Cap**: maximum 200 files. Files beyond this limit are silently dropped.
- **Frontmatter-only**: only the first 30 lines of each file are read (enough
  for YAML frontmatter extraction without loading full content).
- **Sort order**: newest-first (by file mtime).

### formatMemoryManifest()

Formats the scanned memory files into a manifest string for injection into the
model context:
```
[type] filename (ISO-date): description
```
Each line is one memory file. The type comes from frontmatter, the date is the
file's last-modified time in ISO format, and the description is the frontmatter
`description` field.

### findRelevantMemories()

Sonnet-based side-query to select the most relevant memories for the current context:
- Sends the memory manifest + current conversation context to a fast model.
- Returns up to **5** memory files.
- **recentTools suppression**: memories that were already surfaced by recent tool
  calls are excluded from candidates to avoid redundant injection.
- **alreadySurfaced dedup**: tracks which memories have been surfaced in the
  current session to prevent repeated injection of the same file.

## Staleness

### memoryFreshnessNote()

System-reminder wrapper that generates a staleness note for the model context.
When injected memories are older than a threshold, a caveat is prepended:

```
MEMORY_DRIFT_CAVEAT: "These memories were last updated N days ago and may not
reflect the current state of the codebase. Verify before relying on them."
```

The caveat is injected as part of the system-reminder pipeline
(`SystemReminderOrchestrator`), not as a standalone message.

## Team Memory (TEAMMEM)

### Dual-Directory Architecture

Team memory maintains two separate directory trees:
- **Private directory**: `~/.coco/projects/<hash>/memory/` -- per-user memories,
  not shared with team members.
- **Team directory**: `.coco/memory/` in the project root (committed to git) --
  shared memories visible to all team members.

### Per-Type Scope Tags

Each memory entry carries a scope tag controlling visibility:
- Entries in the private directory are always private.
- Entries in the team directory are always shared.
- The `MemoryEntryType` taxonomy (User, Feedback, Project, Reference) applies
  equally to both directories. Type does not determine scope; directory does.
