# coco-compact — Crate Plan

Directory: `services/compact/` (matches TS `services/compact/`)
TS source: `src/services/compact/`

## Dependencies

```
coco-compact depends on:
  - coco-types    (Message, UserMessage, SystemMessage, AttachmentMessage, TokenUsage)
  - coco-inference (ApiClient — LLM call for summarization)
  - coco-messages (message filtering, normalization helpers)
  - coco-error
  - tokio

coco-compact does NOT depend on:
  - coco-tool     (receives ToolUseContext via function parameter, not crate dep)
  - coco-tools    (no concrete tool knowledge)
  - coco-config   (model info passed via parameters)
  - any app/ crate
```

Note: `compact_conversation` takes `&ApiClient` (from coco-inference) for the summarization LLM call, not `ToolUseContext`.

## Data Definitions

```rust
pub struct CompactionResult {
    pub boundary_marker: SystemMessage,
    pub summary_messages: Vec<UserMessage>,
    pub attachments: Vec<AttachmentMessage>,
    pub messages_to_keep: Option<Vec<Message>>,
    pub pre_compact_tokens: Option<i64>,
    pub post_compact_tokens: Option<i64>,
}

pub struct RecompactionInfo {
    pub is_recompaction: bool,
    pub turns_since_previous: i32,
    pub auto_compact_threshold: i64,
}
```

## Core Logic

### Full Compaction (from `compact.ts`)

```rust
/// Summarize conversation via LLM call, preserving recent context.
/// 1. Strip images (replace with [image] marker)
/// 2. Strip re-injectable attachments (skills, agents)
/// 3. Call LLM with compaction prompt
/// 4. Build summary messages
/// 5. Re-inject current file state (up to 5 files, 50K tokens budget)
/// 6. Re-inject active skills (25K token budget)
pub async fn compact_conversation(
    messages: &[Message],
    api_client: &ApiClient,
    model: &str,
    custom_instructions: Option<&str>,
    is_auto: bool,
) -> Result<CompactionResult, CompactError>;

const POST_COMPACT_MAX_FILES: usize = 5;
const POST_COMPACT_TOKEN_BUDGET: i64 = 50_000;
const POST_COMPACT_MAX_TOKENS_PER_FILE: i64 = 5_000;
const POST_COMPACT_SKILLS_TOKEN_BUDGET: i64 = 25_000;
```

### Micro Compaction (from `microCompact.ts`)

```rust
/// Lightweight: clears old tool results without LLM call.
/// Triggered when server cache expired (stale connection).
/// Targets: FileRead, Bash, Grep, Glob, WebSearch, WebFetch, FileEdit, FileWrite
pub async fn micro_compact(
    messages: &mut Vec<Message>,
    context: Option<&ToolUseContext>,
) -> MicrocompactResult;
```

### Auto Compact (from `autoCompact.ts`)

```rust
/// Trigger full compaction when context usage exceeds threshold.
/// Threshold based on model context window size.
pub fn should_auto_compact(
    usage: &TokenUsage,
    model: &str,
    context_window: i64,
) -> bool;
```

### Session Memory Compact (from `sessionMemoryCompact.ts`)

```rust
/// Specialized compaction for session memory extraction.
/// Produces a structured summary for cross-session persistence.
pub async fn compact_for_session_memory(
    messages: &[Message],
    api_client: &ApiClient,
    model: &str,
) -> Result<String, CompactError>;
```
