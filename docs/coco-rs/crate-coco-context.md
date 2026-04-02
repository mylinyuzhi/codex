# coco-context — Crate Plan

TS source: `src/context.ts`, `src/utils/systemPromptType.ts`, `src/utils/attachments.ts`, `src/utils/claudemd.ts`, `src/utils/cwd.ts`

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
/// Collected once per turn, injected into system prompt
pub struct SystemContext {
    pub cwd: PathBuf,
    pub platform: String,             // darwin, linux, win32
    pub shell: String,                // bash, zsh
    pub os_version: String,
    pub model: String,                // current model name
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
