# coco-types — Crate Plan

TS source: `src/types/` (7 files + `generated/`), `src/Tool.ts`, `src/Task.ts`

Note: `types/message.ts` does not exist as a source file — message types are build-time generated
(imports reference `types/message.js`). The message type definitions below are derived from the
generated output and the runtime usage patterns across the codebase.

## Data Definitions

### Message Types (from `types/message.js` — build-time generated)

**设计决策**: 直接包装 vercel-ai 类型，与 TS 包装 `@anthropic-ai/sdk` 的模式一致。
内部 Message = `LlmMessage` (vercel-ai re-export) + 元数据。
发 API 时直接取 `.message` 字段，零转换。

```rust
// === 版本隔离层：re-export vercel-ai 类型为版本无关的别名 ===
// 所有 crate 通过这些别名引用 vercel-ai 类型，不直接 use vercel_ai_provider::*。
// 升级 vercel-ai v5 时，只改这里的 re-export，其他代码无需修改。
pub use vercel_ai_provider::LanguageModelV4Message as LlmMessage;
pub use vercel_ai_provider::LanguageModelV4Prompt as LlmPrompt;  // Vec<LlmMessage>
pub use vercel_ai_provider::UserContentPart as UserContent;
pub use vercel_ai_provider::AssistantContentPart as AssistantContent;
pub use vercel_ai_provider::ToolContentPart as ToolContent;
pub use vercel_ai_provider::TextPart as TextContent;
pub use vercel_ai_provider::FilePart as FileContent;
pub use vercel_ai_provider::ToolCallPart as ToolCallContent;
pub use vercel_ai_provider::ToolResultPart as ToolResultContent;
pub use vercel_ai_provider::ReasoningPart as ReasoningContent;

pub enum Message {
    User(UserMessage),
    Assistant(AssistantMessage),
    System(SystemMessage),
    Attachment(AttachmentMessage),
    ToolResult(ToolResultMessage),
    Progress(ProgressMessage),
    Tombstone(TombstoneMessage),
    ToolUseSummary(ToolUseSummaryMessage),
}

pub struct UserMessage {
    // === LLM API 层（发 API 时直接用这个）===
    pub message: LlmMessage,  // User variant, content: Vec<UserContent>

    // === 内部元数据（不发到 API）===
    pub uuid: Uuid,
    pub timestamp: String,
    pub is_meta: bool,               // hidden from UI, visible to model
    pub is_visible_in_transcript_only: bool,
    pub is_virtual: bool,            // not sent to API
    pub is_compact_summary: bool,
    pub permission_mode: Option<PermissionMode>,
    pub origin: Option<MessageOrigin>,
}

pub struct AssistantMessage {
    // === LLM API 层 ===
    pub message: LlmMessage,  // Assistant variant, content: Vec<AssistantContent>

    // === 内部元数据 ===
    pub uuid: Uuid,
    pub model: String,
    pub stop_reason: Option<StopReason>,
    pub usage: Option<TokenUsage>,
    pub cost_usd: Option<f64>,
    pub request_id: Option<String>,
    pub api_error: Option<ApiError>,
}

pub enum StopReason { EndTurn, MaxTokens, StopSequence, ToolUse }

pub struct ProgressMessage {
    pub tool_use_id: String,
    pub data: Value,
    pub parent_message_uuid: Option<Uuid>,
}

pub struct TombstoneMessage {
    pub uuid: Uuid,
    pub original_type: String,  // what message type was removed
}

pub struct ToolUseSummaryMessage {
    pub uuid: Uuid,
    pub tool_name: String,
    pub summary: String,
}

/// System messages have sub-types for different notification kinds.
/// All system messages are `role: "user"` with `is_meta: true` for the API.
pub enum SystemMessage {
    Informational(SystemInformationalMessage),
    ApiError(SystemAPIErrorMessage),
    CompactBoundary(SystemCompactBoundaryMessage),
    MicrocompactBoundary(SystemMicrocompactBoundaryMessage),
    LocalCommand(SystemLocalCommandMessage),
    PermissionRetry(SystemPermissionRetryMessage),
    BridgeStatus(SystemBridgeStatusMessage),
    MemorySaved(SystemMemorySavedMessage),
    AwaySummary(SystemAwaySummaryMessage),
    AgentsKilled(SystemAgentsKilledMessage),
    ApiMetrics(SystemApiMetricsMessage),
    StopHookSummary(SystemStopHookSummaryMessage),
    TurnDuration(SystemTurnDurationMessage),
    ScheduledTaskFire(SystemScheduledTaskFireMessage),
}

pub enum SystemMessageLevel { Info, Warning, Error }

pub struct SystemInformationalMessage {
    pub uuid: Uuid,
    pub level: SystemMessageLevel,
    pub title: String,
    pub message: String,
}

pub struct SystemAPIErrorMessage {
    pub uuid: Uuid,
    pub error: String,
    pub status_code: Option<i32>,
}

pub struct SystemCompactBoundaryMessage {
    pub uuid: Uuid,
    pub tokens_before: i64,
    pub tokens_after: i64,
}

pub struct SystemMicrocompactBoundaryMessage {
    pub uuid: Uuid,
}

pub struct SystemLocalCommandMessage {
    pub uuid: Uuid,
    pub command: String,
    pub output: String,
}

// Other system message variants follow the same pattern:
// SystemPermissionRetryMessage, SystemBridgeStatusMessage,
// SystemMemorySavedMessage, SystemAwaySummaryMessage,
// SystemAgentsKilledMessage, SystemApiMetricsMessage,
// SystemStopHookSummaryMessage, SystemTurnDurationMessage,
// SystemScheduledTaskFireMessage — detailed during implementation.

pub enum MessageOrigin {
    UserInput,
    SystemInjected,
    ToolResult,
    CompactSummary,
    SubagentReply,
}

/// API-normalized message forms (produced by coco-messages normalize_for_api).
/// Distinct from internal Message — strips meta fields, merges consecutive same-role.
pub struct NormalizedUserMessage {
    pub role: String,  // "user"
    pub content: Vec<ContentBlock>,
}

pub struct NormalizedAssistantMessage {
    pub role: String,  // "assistant"
    pub content: Vec<ContentBlock>,
}

pub type NormalizedMessage = Either<NormalizedUserMessage, NormalizedAssistantMessage>;

/// Stream events emitted during API response processing.
pub enum StreamEvent {
    TextDelta { text: String },
    ThinkingDelta { text: String },
    ToolUseStart { id: String, name: String },
    ToolUseInput { id: String, delta: String },
    ToolUseEnd { id: String },
    RequestStart(RequestStartEvent),
    MessageComplete,
}

pub struct RequestStartEvent {
    pub model: String,
    pub request_id: Option<String>,
}

/// Direction hint for partial compaction.
pub enum PartialCompactDirection { Oldest, Newest }

/// Streaming accumulation types — used by coco-messages, coco-inference, coco-query (3+ crates).
/// Task budget for API output pacing. Used by coco-inference and coco-query.
pub struct TaskBudget {
    pub total: i64,
    pub remaining: Option<i64>,
}

pub struct StreamingToolUse {
    pub id: String,
    pub name: String,
    pub input_json: String,  // Accumulated JSON string
}

pub struct StreamingThinking {
    pub text: String,
}
```

### Permission Types (from `types/permissions.ts`)

```rust
pub enum PermissionMode {
    Default,
    Plan,
    BypassPermissions,
    DontAsk,
    AcceptEdits,
    Auto,     // feature-gated
    Bubble,   // internal: escalate to parent
}

pub enum PermissionBehavior { Allow, Deny, Ask }

pub enum PermissionRuleSource {
    UserSettings, ProjectSettings, LocalSettings, FlagSettings,
    PolicySettings, CliArg, Command, Session,
}

pub struct PermissionRule {
    pub source: PermissionRuleSource,
    pub behavior: PermissionBehavior,
    pub value: PermissionRuleValue,
}

pub struct PermissionRuleValue {
    pub tool_name: String,
    pub rule_content: Option<String>,  // e.g. "prefix:git *"
}

pub enum PermissionDecision {
    Allow { updated_input: Option<Value>, feedback: Option<String> },
    Ask { message: String, suggestions: Vec<PermissionUpdate> },
    Deny { message: String, reason: PermissionDecisionReason },
}

/// Why a permission decision was made. Attached to PermissionDecision::Deny.
pub enum PermissionDecisionReason {
    Rule { rule: PermissionRule },
    Mode { mode: PermissionMode },
    Classifier { classifier: String, reason: String },
    Hook { hook_name: String, reason: Option<String> },
    SafetyCheck { reason: String, classifier_approvable: bool },
    AsyncAgent { reason: String },
    User,
    Sandboxed,
}

pub struct ToolPermissionContext {
    pub mode: PermissionMode,
    pub additional_dirs: HashMap<String, AdditionalWorkingDir>,
    pub allow_rules: PermissionRulesBySource,
    pub deny_rules: PermissionRulesBySource,
    pub ask_rules: PermissionRulesBySource,
    pub bypass_available: bool,
    pub pre_plan_mode: Option<PermissionMode>,
}
```

### Command Types (from `types/command.ts`)

```rust
pub struct CommandBase {
    pub name: String,
    pub description: String,
    pub aliases: Vec<String>,
    pub availability: Vec<CommandAvailability>,  // ClaudeAi, Console
    pub is_hidden: bool,
    pub argument_hint: Option<String>,
    pub when_to_use: Option<String>,
    pub user_invocable: bool,
    pub is_sensitive: bool,
    pub loaded_from: Option<CommandSource>,  // Skills, Plugin, Bundled, Mcp
}

pub enum CommandType {
    Prompt(PromptCommandData),
    Local(LocalCommandData),
}

pub struct PromptCommandData {
    pub progress_message: String,
    pub content_length: i64,
    pub allowed_tools: Option<Vec<String>>,
    pub model: Option<String>,
    pub context: CommandContext,  // Inline or Fork
    pub agent: Option<String>,
    pub effort: Option<EffortValue>,
    pub hooks: Option<Value>,  // deserialized by coco-hooks, not typed here (avoids L1->L4 dep)
}
```

### Tool Types (from `Tool.ts`)

```rust
pub struct ToolInputSchema {
    pub schema_type: String,  // always "object"
    pub properties: HashMap<String, Value>,
}

pub struct ToolResult<T> {
    pub data: T,
    pub new_messages: Vec<Message>,
    // Note: context modification is handled by Tool::modify_context_after() in coco-tool,
    // NOT by a closure here. ToolResult is a plain data struct with no trait objects.
}

pub struct ToolProgress {
    pub tool_use_id: String,
    pub data: Value,
}
```

### Task Types (from `Task.ts`)

```rust
pub enum TaskType {
    LocalBash,
    LocalAgent,
    RemoteAgent,
    InProcessTeammate,
    LocalWorkflow,
    MonitorMcp,
    Dream,
}

pub enum TaskStatus { Pending, Running, Completed, Failed, Killed }

pub struct TaskStateBase {
    pub id: String,          // prefix + 8 random base36 chars
    pub task_type: TaskType,
    pub status: TaskStatus,
    pub description: String,
    pub tool_use_id: Option<String>,
    pub start_time: i64,
    pub end_time: Option<i64>,
    pub total_paused_ms: Option<i64>,
    pub output_file: String,
    pub output_offset: i64,
    pub notified: bool,
}

pub struct TaskHandle {
    pub task_id: String,
    pub cleanup: Option<Box<dyn FnOnce()>>,
}
```

### ID Types (from `types/ids.ts`)

```rust
// Branded newtype pattern
pub struct SessionId(pub String);
pub struct AgentId(pub String);  // format: `a(?:.+-)?[0-9a-f]{16}$`
pub struct TaskId(pub String);

// SandboxMode (from cocode-rs, needed by exec/sandbox)
pub enum SandboxMode { ReadOnly, WorkspaceWrite, FullAccess, ExternalSandbox }
```

### Hook Types (from `types/hooks.ts`)

```rust
pub enum HookEventType {
    PreToolUse,
    PostToolUse,
    SessionStart,
    Setup,
    SubagentStart,
    UserPromptSubmit,
    FileChanged,
}

pub struct HookResult {
    pub outcome: HookOutcome,  // Success, Blocking, NonBlockingError, Cancelled
    pub message: Option<Message>,
    pub permission_behavior: Option<PermissionBehavior>,
    pub stop_reason: Option<String>,
    pub updated_input: Option<Value>,
}
```

### Token & Cost Types

```rust
/// Per-request token counts (returned by LLM API)
pub struct TokenUsage {
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_input_tokens: i64,
    pub cache_creation_input_tokens: i64,
}

/// Per-model accumulated usage (for cost tracking in coco-messages)
pub struct ModelUsage {
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_input_tokens: i64,
    pub cache_creation_input_tokens: i64,
    pub web_search_requests: i64,
    pub cost_usd: f64,
}
```

### Provider & Model Types (from multi-provider-plan.md)

```rust
/// Which LLM provider implementation to use.
/// Consumed by coco-config (ProviderInfo) and coco-inference (ProviderFactory).
pub enum ProviderApi {
    Anthropic,
    Openai,
    Gemini,
    Volcengine,
    Zai,
    OpenaiCompat,
}

/// Which purpose a model serves. Multiple roles can map to different models.
/// Consumed by coco-config (ModelRoles) and coco-query (role resolution).
pub enum ModelRole {
    Main,       // Primary conversation
    Fast,       // Quick/cheap (Haiku)
    Compact,    // Summarization (falls back to Main)
    Plan,       // Planning/architecture
    Explore,    // Codebase exploration
    Review,     // Code review
    HookAgent,  // Hook agent execution
    Memory,     // Memory relevance ranking
}

/// A resolved model identity: provider + model ID.
/// Produced by coco-config, consumed by coco-inference.
pub struct ModelSpec {
    pub provider: ProviderApi,
    pub model_id: String,
    pub canonical_id: String,
}

/// Model capabilities (checked at request time).
pub enum Capability {
    ToolUse,
    Vision,
    Thinking,
    AdaptiveThinking,
    StructuredOutput,
    Effort,
    FastMode,
    PromptCaching,
    Streaming,
}

/// How a model handles file editing tools.
pub enum ApplyPatchToolType {
    None,           // Use FileEdit (Anthropic default)
    CustomToolCall, // apply_patch via custom tool_call (OpenAI)
    BuiltIn,        // Native apply_patch support (future)
}

/// Communication protocol (OpenAI has two APIs).
pub enum WireApi {
    Chat,       // Standard chat completions
    Responses,  // OpenAI responses API (supports apply_patch)
}
```

### Log Types (from `types/logs.ts`)

```rust
/// Serialized message for log persistence (session replay, analytics)
pub struct SerializedMessage {
    pub message: Message,           // flattened
    pub cwd: String,
    pub user_type: String,
    pub entrypoint: Option<String>, // cli, sdk-ts, sdk-py, etc.
    pub session_id: String,
    pub timestamp: String,
    pub version: String,
    pub git_branch: Option<String>,
    pub slug: Option<String>,       // session slug for plans (resume)
}

pub struct LogOption {
    pub date: String,
    pub path: String,
}
```

### Plugin Types (from `types/plugin.ts`)

```rust
/// Re-exports from utils/plugins/schemas (canonical definitions in coco-modules)
/// Provided here for convenience: PluginAuthor, PluginManifest, CommandMetadata

/// Built-in plugin that ships with the CLI (can be enabled/disabled by users)
pub struct BuiltinPluginDefinition {
    pub name: String,
    pub display_name: String,
    pub description: String,
    pub manifest: PluginManifest,
}
```

### Text Input Types (from `types/textInputTypes.ts`)

```rust
/// These are TUI-layer types. In coco-rs, they live in coco-tui, not coco-types.
/// Listed here for TS mapping completeness only.
///
/// InlineGhostText, InputCommand, TextInputProps, PromptInputState, etc.
/// → coco-tui (v1, TUI-specific)
```

## Dependencies

```
coco-types depends on:
  - vercel-ai-provider (L0 types: LanguageModelV4Message, UserContentPart, AssistantContentPart, ToolContentPart)
  - serde, serde_json (serialization)
  - uuid (ID types)
  - chrono (timestamps)
  - strum (enum derive)

coco-types depends on vercel-ai-provider because Message wraps LanguageModelV4Message directly
(same pattern as TS wrapping @anthropic-ai/sdk types). vercel-ai-provider is L0 pure types.

coco-types does NOT depend on:
  - any other coco-* crate (it is the foundation layer)
  - any app/, services/, exec/ crate

Every other crate in the workspace can depend on coco-types.
```
