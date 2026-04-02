# coco-messages — Crate Plan

TS source: `src/utils/messages.ts`, `src/history.ts`, `src/cost-tracker.ts`

## Dependencies

```
coco-messages depends on:
  - coco-types    (Message wraps LanguageModelV4Message — vercel-ai types come via coco-types)
  - coco-error
  - serde, serde_json, uuid, chrono

coco-messages does NOT depend on:
  - vercel-ai-provider directly (gets vercel-ai types transitively through coco-types)
  - coco-config   (no model/settings knowledge)
  - coco-inference (no LLM calls)
  - coco-tool     (no tool awareness)
  - any app/ crate
```

## Data Definitions

### Message Creation

```rust
pub fn create_user_message(params: CreateUserMessageParams) -> UserMessage;
pub fn create_assistant_message(content: Vec<AssistantContentBlock>, usage: Option<TokenUsage>) -> AssistantMessage;
pub fn create_assistant_error_message(content: &str, error: Option<ApiError>) -> AssistantMessage;

pub struct CreateUserMessageParams {
    pub content: MessageContent,  // String or Vec<ContentBlock>
    pub is_meta: bool,
    pub is_virtual: bool,
    pub is_compact_summary: bool,
    pub permission_mode: Option<PermissionMode>,
    pub origin: Option<MessageOrigin>,
}
```

### Message Normalization & Conversion Pipeline

This is the critical bridge between internal types and the LLM API.
See CLAUDE.md "Message Model" section for the full architecture.

```rust
/// Step 1: Filter and normalize internal messages for API.
/// - Strip: virtual, progress, meta-only system messages
/// - Merge: consecutive same-role messages (Bedrock compat)
/// - Filter: tool refs for disabled tools
/// - Replace: oversized content per error feedback
pub fn normalize_for_api(messages: &[Message]) -> Vec<Message>;

/// Step 2: Assemble vercel-ai prompt from normalized messages.
/// Since Message already wraps LanguageModelV4Message, this is trivial:
///   messages.iter().map(|m| m.message().clone()).collect()
/// Plus prepending SystemPrompt as LanguageModelV4Message::System.
///
/// No content-block-level type conversion needed — Message.message IS the API type.
pub fn to_language_model_prompt(
    normalized: &[Message],
    system: &SystemPrompt,
) -> LanguageModelV4Prompt;

pub fn get_last_assistant_message(messages: &[Message]) -> Option<&AssistantMessage>;
pub fn has_tool_calls_in_last_turn(messages: &[Message]) -> bool;
pub fn derive_short_message_id(uuid: &str) -> String;  // 6-char base36
```

### History (from `history.ts`)

```rust
pub struct HistoryEntry {
    pub display: String,
    pub timestamp: i64,
    pub project: String,
    pub session_id: Option<String>,
    pub pasted_contents: HashMap<i32, StoredPastedContent>,
}

pub struct StoredPastedContent {
    pub id: i32,
    pub content_type: PasteType,  // Text, Image
    pub content: Option<String>,        // inline for small
    pub content_hash: Option<String>,   // hash ref for large
    pub media_type: Option<String>,
    pub filename: Option<String>,
}

/// Read history in reverse (most recent first)
pub fn read_history(project: &str) -> impl Iterator<Item = HistoryEntry>;
pub fn add_to_history(entry: &HistoryEntry);
pub fn expand_pasted_refs(input: &str, pastes: &HashMap<i32, StoredPastedContent>) -> String;
```

### Cost Tracking (from `cost-tracker.ts`)

```rust
pub struct SessionCostState {
    pub total_cost_usd: f64,
    pub total_api_duration_ms: i64,
    pub total_tool_duration_ms: i64,
    pub total_lines_added: i64,
    pub total_lines_removed: i64,
    pub model_usage: HashMap<String, ModelUsage>,  // keyed by canonical model name
}

pub fn get_total_cost() -> f64;
pub fn add_session_cost(cost: f64, usage: &TokenUsage, model: &str);
pub fn format_total_cost() -> String;              // "$0.42 | 12.3K in / 4.5K out"
pub fn save_session_costs(session_id: &str);
pub fn restore_session_costs(session_id: &str) -> Option<SessionCostState>;
```
