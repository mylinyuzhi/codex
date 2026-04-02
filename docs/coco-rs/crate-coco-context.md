# coco-context — Crate Plan

TS source: `src/context.ts`, `src/utils/systemPromptType.ts`, `src/utils/attachments.ts`, `src/utils/claudemd.ts`, `src/utils/cwd.ts`, `src/utils/fileHistory.ts` (200 LOC), `src/utils/fileStateCache.ts` (1.5K LOC), `src/utils/fileRead.ts`

## Dependencies

```
coco-context depends on:
  - coco-types   (Message, Attachment types, PermissionMode)
  - coco-config  (ModelInfo — for context_window, max_output_tokens)
  - coco-error
  - utils/git    (git status)

coco-context does NOT depend on:
  - coco-inference (no LLM calls)
  - coco-tool (no tool types)
  - any app/ crate
```

## Data Definitions

### System Context (from `context.ts`)

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Platform { Darwin, Linux, Windows }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShellKind { Bash, Zsh, Sh, PowerShell }

/// Collected once per turn, injected into system prompt
pub struct SystemContext {
    pub cwd: PathBuf,
    pub platform: Platform,
    pub shell: ShellKind,
    pub os_version: String,
    pub model: String,                // current model name (dynamic, stays String)
    pub knowledge_cutoff: String,     // "May 2025"
    pub current_date: String,
    pub git_status: Option<GitStatus>,
}

pub struct GitStatus {
    pub branch: String,
    pub main_branch: String,
    pub user: String,
    pub status: String,           // short status output
    pub recent_commits: String,   // last 5 commits
}

pub fn get_system_context() -> SystemContext;
pub fn get_git_status(cwd: &Path) -> Option<GitStatus>;
```

### File History & Encoding (from `fileHistory.ts` 200 LOC, `fileRead.ts`, `fileStateCache.ts` 1.5K LOC)

```rust
/// File edit tracking per turn: records file contents keyed by message UUID.
/// Used by /rewind command to restore file state to a previous turn.
pub struct FileHistoryState {
    /// Snapshots keyed by message UUID → file path → content.
    snapshots: HashMap<String, HashMap<PathBuf, FileSnapshot>>,
}

pub struct FileSnapshot {
    pub content: String,
    pub encoding: FileEncoding,
    pub line_ending: LineEnding,
}

pub enum FileEncoding { Utf8, Latin1, Binary }
pub enum LineEnding { Lf, CrLf, Cr }

impl FileHistoryState {
    /// Record a file's content before modification.
    pub fn track_edit(&mut self, message_uuid: &str, path: &Path);
    /// Create a snapshot at a specific message point (for /rewind).
    pub fn make_snapshot(&self, message_uuid: &str) -> HashMap<PathBuf, FileSnapshot>;
}

/// File read with encoding detection and line ending preservation.
/// Detects encoding via BOM or byte analysis.
/// Preserves line endings (LF vs CRLF) on write-back.
pub fn read_file_with_metadata(path: &Path) -> Result<(String, FileEncoding, LineEnding), io::Error>;

/// LRU file read cache (per turn, up to 50 entries).
/// Avoids re-reading unchanged files within a turn.
pub struct FileStateCache {
    cache: lru::LruCache<PathBuf, CachedFileState>,
}
```

### Attachments (from `utils/attachments.ts` — 4K LOC)

```rust
pub enum Attachment {
    File(FileAttachment),
    PdfReference(PdfReferenceAttachment),
    AlreadyReadFile(AlreadyReadFileAttachment),
    AgentMention(AgentMentionAttachment),
    Hook(HookAttachment),
    Memory(MemoryAttachment),
    TeammateMail(TeammateMailboxAttachment),
}

pub struct FileAttachment {
    pub filename: String,
    pub content: String,
    pub truncated: bool,
    pub display_path: String,
}

pub struct PdfReferenceAttachment {
    pub filename: String,
    pub page_count: i32,
    pub file_size: i64,
    pub display_path: String,
}

pub enum HookAttachment {
    Success { content: String, hook_name: String },
    BlockingError { error: HookBlockingError },
    Cancelled { hook_name: String },
    PermissionDecision { decision: PermissionBehavior },
}
```

### CLAUDE.md Discovery (from `utils/claudemd.ts`)

```rust
pub struct MemoryFileInfo {
    pub path: PathBuf,
    pub memory_type: MemoryType,  // Managed, User, Project, Local
    pub content: String,
    pub parent: Option<PathBuf>,
    pub globs: Option<Vec<String>>,
}

pub enum MemoryType { Managed, User, Project, Local }

/// Discovery order (priority):
/// 1. Managed: /etc/claude-code/CLAUDE.md
/// 2. User: ~/.claude/CLAUDE.md
/// 3. Project: CLAUDE.md, .claude/CLAUDE.md, .claude/rules/*.md
/// 4. Local: CLAUDE.local.md
pub fn get_memory_files(cwd: &Path) -> Vec<MemoryFileInfo>;

pub const MAX_MEMORY_LINES: usize = 200;
pub const MAX_MEMORY_BYTES: usize = 25_000;  // per-file truncation

pub fn strip_html_comments(content: &str) -> String;
pub fn truncate_entrypoint(raw: &str) -> (String, bool);  // (content, was_truncated)
```

### System Prompt Building (from `utils/systemPromptType.ts`)

```rust
pub struct SystemPrompt {
    pub blocks: Vec<SystemPromptBlock>,
}

pub enum SystemPromptBlock {
    Text(String),
    CacheBreakpoint,
}

/// Build system prompt from:
/// 1. Identity block (model name, date, knowledge cutoff)
/// 2. Tool policy block (tool descriptions, usage instructions)
/// 3. Security block (safety guidelines)
/// 4. CLAUDE.md memory files
/// 5. Environment block (cwd, git status, platform)
/// 6. Permission mode block
/// 7. Custom system prompt (if configured)
pub fn build_system_prompt(
    context: &SystemContext,
    memory_files: &[MemoryFileInfo],
    tool_descriptions: &[String],
    config: &SystemPromptConfig,
) -> SystemPrompt;
```

### Context Window (from `utils/context.ts`)

```rust
// Constants (defaults, not hardcoded per-model)
pub const COMPACT_MAX_OUTPUT_TOKENS: i64 = 20_000;
pub const CAPPED_DEFAULT_MAX_TOKENS: i64 = 8_000;
pub const ESCALATED_MAX_TOKENS: i64 = 64_000;

// Reads from ModelInfo (coco-config), NOT hardcoded
pub fn get_context_window(model_info: &ModelInfo) -> i64 {
    model_info.context_window  // from coco-config, default 200_000
}
pub fn get_max_output_tokens(model_info: &ModelInfo) -> (i64, i64);
pub fn calculate_context_pct(usage: &TokenUsage, window: i64) -> (f64, f64);
```
