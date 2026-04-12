# coco-compact — Crate Plan

Directory: `services/compact/` (matches TS `services/compact/`)
TS source: `src/services/compact/compact.ts` (60K), `src/services/compact/microCompact.ts` (500+), `src/services/compact/grouping.ts` (64), `src/services/compact/postCompactCleanup.ts` (78), `src/services/compact/apiMicrocompact.ts` (154), `src/services/compact/timeBasedMCConfig.ts` (44)

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

/// Compaction error types.
pub enum CompactError {
    LlmCallFailed { source: ApiError },
    TokenBudgetExceeded { actual: i64, limit: i64 },
    Cancelled,
    StreamRetryExhausted { attempts: i32 },
}

/// Micro-compaction result.
pub struct MicrocompactResult {
    pub messages_cleared: i32,
    pub tokens_saved_estimate: i64,
    pub was_time_triggered: bool,
}

/// API-level context management strategy
pub enum ContextEditStrategy {
    /// Clear tool results from older turns (API 2025-09-19)
    ClearToolUses {
        trigger: Option<TokenTrigger>,
        keep: Option<ToolUseKeep>,
        clear_tool_inputs: ClearToolInputs,
        exclude_tools: Vec<String>,
        clear_at_least: Option<TokenTarget>,
    },
    /// Clear thinking/reasoning from older turns (API 2025-10-15)
    ClearThinking {
        keep: ThinkingKeep,
    },
}

pub enum ClearToolInputs {
    All,
    SpecificTools(Vec<String>),
    None,
}

pub enum ThinkingKeep {
    Recent { turns: i32 },
    All,
}
```

## Core Logic

### Full Compaction (from `compact.ts`)

```rust
/// Summarize conversation via LLM call, preserving recent context.
/// 1. Strip images (replace with [image] marker to prevent prompt-too-long)
/// 2. Strip re-injectable attachments (skills, agents)
/// 3. Call LLM with compaction prompt (MAX_COMPACT_STREAMING_RETRIES = 2)
/// 4. Build summary messages
/// 5. Re-inject current file state (up to 5 files, 50K tokens budget)
/// 6. Re-inject active skills (25K token budget)
/// 7. Execute post-compact hooks (plugin cleanup)
/// 8. Run post-compact cleanup (cache clearing)
pub async fn compact_conversation(
    messages: &[Message],
    api_client: &ApiClient,
    model: &str,
    custom_instructions: Option<&str>,
    is_auto: bool,
    cancel: CancellationToken,  // LLM call can be long; must be cancellable
) -> Result<CompactionResult, CompactError>;

const POST_COMPACT_MAX_FILES: usize = 5;
const POST_COMPACT_TOKEN_BUDGET: i64 = 50_000;
const POST_COMPACT_MAX_TOKENS_PER_FILE: i64 = 5_000;
const POST_COMPACT_SKILLS_TOKEN_BUDGET: i64 = 25_000;
```

### Micro Compaction (from `microCompact.ts`, 500+ LOC)

```rust
/// Lightweight: clears old tool results without LLM call.
/// Triggered when server cache expired (stale connection).
/// Targets: FileRead, Bash, Grep, Glob, WebSearch, WebFetch, FileEdit, FileWrite
///
/// Time-based trigger (from timeBasedMCConfig):
/// - Gap since last assistant message > threshold (default 60 min)
/// - Cache TTL expires, so full prefix will rewrite anyway
/// - Clear old tool results BEFORE API call (shrink what gets rewritten)
///
/// Cached microcompact (ant-only, CACHED_MICROCOMPACT feature):
/// - Lazy-initialized cached MC module
/// - pendingCacheEdits → consumed after insertion
/// - pinnedCacheEdits → re-sent for cache hits
pub async fn micro_compact(
    messages: &mut Vec<Message>,
    context: Option<&ToolUseContext>,
) -> MicrocompactResult;
```

### Message Grouping (from `grouping.ts`, 64 LOC)

```rust
/// Segment messages at API-round boundaries for compaction.
/// Boundary fires when NEW assistant.id appears.
/// One group per API round-trip (well-formed conversations).
/// Replaces human-turn grouping with API-round grouping for finer-grained compaction.
pub fn group_messages_by_api_round(messages: &[Message]) -> Vec<Vec<Message>>;
```

### Post-Compact Cleanup (from `postCompactCleanup.ts`, 78 LOC)

TS uses a god function that hardcodes 10+ cache clears. This is an SRP violation
that doesn't scale. coco-rs uses an **observer pattern** instead:

```rust
/// Trait for crates that own caches invalidated by compaction.
/// Each crate registers its own observer at startup — no hardcoded list in coco-compact.
pub trait CompactionObserver: Send + Sync {
    /// Called after compaction completes. Return error to log (non-fatal).
    fn on_compaction_complete(
        &self,
        result: &CompactionResult,
        is_main_agent: bool,  // false for subagents — skip main-thread-only resets
    ) -> Result<(), anyhow::Error>;
}

/// Registry of compaction observers. Populated by coco-cli at startup.
pub struct CompactionObserverRegistry {
    observers: Vec<Arc<dyn CompactionObserver>>,
}

impl CompactionObserverRegistry {
    pub fn register(&mut self, observer: Arc<dyn CompactionObserver>);

    /// Notify all observers. Errors are logged but don't fail compaction.
    pub fn notify_all(&self, result: &CompactionResult, is_main_agent: bool);
}

/// Expected observers (each crate registers its own):
/// - coco-context: clear getUserContext cache, memory files cache
/// - coco-permissions: clear classifier approvals, speculative checks
/// - coco-messages: clear session messages cache
/// - coco-inference: clear beta tracing state
/// - coco-tools: sweep file content cache (if COMMIT_ATTRIBUTION)
///
/// Note: coco-compact's own microcompact state is reset BEFORE calling observers
/// (inside compact_conversation), not via self-registration. This avoids recursion.
```

### API Microcompact (from `apiMicrocompact.ts`, 154 LOC)

```rust
/// Native API context management (2025-09-19 / 2025-10-15).
/// Uses API-level clear_tool_uses / clear_thinking strategies.
///
/// Env vars (ant-only):
/// - USE_API_CLEAR_TOOL_RESULTS: clear tool result content
/// - USE_API_CLEAR_TOOL_USES: clear entire tool use blocks
/// - API_MAX_INPUT_TOKENS: trigger threshold (default: 180K)
/// - API_TARGET_INPUT_TOKENS: keep target (default: 40K)
pub fn get_api_context_management(options: ApiContextOptions) -> Option<ContextManagementConfig>;

pub struct ApiContextOptions {
    pub has_thinking: bool,
    pub is_redact_thinking_active: bool,
    pub clear_all_thinking: bool,
}
```

### Time-Based MC Config (from `timeBasedMCConfig.ts`, 44 LOC)

```rust
/// GrowthBook-driven config for time-based microcompact trigger.
pub struct TimeBasedMcConfig {
    pub enabled: bool,
    pub gap_threshold_minutes: i32,  // default: 60 (matches cache TTL)
    pub keep_recent: i32,            // default: 5 (API rounds to keep)
}

pub fn get_time_based_mc_config() -> TimeBasedMcConfig;
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

## Compact Warning State (from `compactWarningState.ts`, 18 LOC)

```rust
/// State store tracking whether autocompact warning should be suppressed.
/// Set to true after successful compaction to avoid showing redundant warnings.
/// Pure state — no UI dependency (TS separates from React hook for startup path).
pub struct CompactWarningState {
    pub suppressed: AtomicBool,
}
```

## Compact Prompt Generation (from `prompt.ts`, 374 LOC)

```rust
/// Generates compact prompts with branching logic:
/// - Proactive mode: different prompt structure for proactive compaction
/// - Cache-sharing fork: prompt designed to maximize cache hits across turns
/// - Includes message selection criteria and format instructions
pub fn build_compact_prompt(
    mode: CompactMode,
    proactive: bool,
    cache_sharing: bool,
) -> String;
```
