# coco-query — Crate Plan

TS source: `src/QueryEngine.ts`, `src/query.ts`, `src/query/tokenBudget.ts`, `src/query/buildQueryConfig.ts`

## Dependencies

```
coco-query depends on:
  - coco-types       (Message, TokenUsage, SessionId)
  - coco-config      (ModelRoles, ModelInfo — role resolution)
  - coco-inference   (ModelHub, ApiClient — LLM calls)
  - coco-tool        (ToolRegistry, StreamingToolExecutor, run_tools)
  - coco-context     (build_system_prompt, get_context_window)
  - coco-messages    (normalize_for_api, history)
  - coco-compact     (compact_conversation, should_auto_compact)
  - coco-permissions (evaluate_permission — for pre-tool hooks)
  - coco-hooks       (run_hooks — pre/post tool use)
  - coco-state       (AppState — shared state, lateral dep within app/)
  - coco-error

coco-query does NOT depend on:
  - coco-tools       (concrete tools — injected via ToolRegistry at init)
  - coco-commands    (commands — injected via CommandRegistry)
  - coco-skills      (skills — loaded into commands before query starts)
  - coco-shell       (used by BashTool inside coco-tools, not by query)
```

## Data Definitions

### QueryEngine (from `QueryEngine.ts`)

```rust
pub struct QueryEngine {
    config: QueryEngineConfig,
    tool_registry: Arc<ToolRegistry>,
    command_registry: Arc<CommandRegistry>,
    api_client: Arc<ApiClient>,          // vercel-ai based
    state: Arc<RwLock<AppState>>,
    messages: Vec<Message>,
    budget_tracker: BudgetTracker,
    total_usage: ModelUsage,             // cumulative API usage
    permission_denials: Vec<SdkPermissionDenial>,  // permission audit trail
    read_file_state: Arc<RwLock<FileStateCache>>,  // shared file read cache
    discovered_skill_names: HashSet<String>,        // cleared each turn
    loaded_nested_memory_paths: HashSet<String>,    // nested CLAUDE.md dedup
}

/// Tracks permission denials for SDK audit trail.
pub struct SdkPermissionDenial {
    pub tool_name: String,
    pub input_summary: String,
    pub reason: String,
    pub timestamp: i64,
}

/// Recovery mechanism when tool fails after permission was already granted.
pub struct OrphanedPermission {
    pub tool_name: String,
    pub tool_use_id: String,
    pub granted_input: Value,
}

/// Snip compaction boundary handler.
/// Called when a snip boundary is encountered during message replay.
pub type SnipReplayFn = Arc<dyn Fn(&Message, &[Message]) -> Option<SnipReplayResult> + Send + Sync>;

pub struct SnipReplayResult {
    pub messages: Vec<Message>,
    pub executed: bool,
}

pub struct QueryEngineConfig {
    pub cwd: PathBuf,
    pub tools: Arc<Vec<Arc<dyn Tool>>>,
    pub commands: Vec<Command>,
    pub mcp_clients: Vec<McpConnection>,
    pub agent_definitions: Vec<AgentDefinition>,
    pub read_file_cache: Arc<RwLock<FileStateCache>>,
    pub custom_system_prompt: Option<String>,
    pub append_system_prompt: Option<String>,
    pub model: Option<String>,
    pub fallback_model: Option<String>,
    pub thinking_config: Option<ThinkingConfig>,
    pub max_turns: Option<i32>,
    pub max_budget_usd: Option<f64>,
    pub task_budget: Option<TaskBudget>,    // token budget for task
    pub json_schema: Option<Value>,         // structured output schema
    pub verbose: bool,
    pub replay_user_messages: bool,         // SDK message replay mode
    pub include_partial_messages: bool,     // include partial messages in output
    pub handle_elicitation: Option<Arc<dyn Fn(String, ElicitRequestParams, CancellationToken) -> BoxFuture<ElicitResult> + Send + Sync>>,
    pub abort_controller: Option<CancellationToken>,
    pub orphaned_permission: Option<OrphanedPermission>,
    pub snip_replay: Option<SnipReplayFn>,  // snip compaction boundary handler
    pub set_sdk_status: Option<Arc<dyn Fn(SdkStatus) + Send + Sync>>,
}

pub struct TaskBudget {
    pub total: i64,  // total token budget
}
```

### QueryConfig (from `query/config.ts`)

```rust
pub struct QueryConfig {
    pub session_id: SessionId,
    pub gates: QueryGates,
}

pub struct QueryGates {
    pub streaming_tool_execution: bool,
    pub emit_tool_use_summaries: bool,
    pub fast_mode_enabled: bool,
}
```

### Budget Tracker (from `query/tokenBudget.ts`)

```rust
pub struct BudgetTracker {
    pub continuation_count: i32,
    pub last_delta_tokens: i64,
    pub last_global_turn_tokens: i64,
    pub started_at: Instant,
}

pub enum BudgetDecision {
    Continue,
    Stop { reason: String },
    Nudge { message: String },
}

/// Budget logic:
/// - Max 3 continuations per turn
/// - Stop if < 500 tokens per continuation (diminishing returns)
/// - Stop at 90% budget threshold
pub fn check_budget(
    tracker: &BudgetTracker,
    budget: Option<f64>,
    global_tokens: i64,
) -> BudgetDecision;
```

### QueryEngine public API

```rust
impl QueryEngine {
    pub fn new(config: QueryEngineConfig) -> Self;

    /// Main conversation loop (async generator in TS → Stream in Rust).
    pub async fn run(&mut self, cancel: CancellationToken, event_tx: mpsc::Sender<QueryEvent>) -> Result<(), QueryError>;

    /// Interrupt the current turn. Cancels in-flight API call and tool execution.
    pub fn interrupt(&self);

    /// Get current session ID.
    pub fn session_id(&self) -> &SessionId;

    /// Switch model mid-session.
    pub fn set_model(&mut self, model: String);

    /// Get accumulated messages.
    pub fn messages(&self) -> &[Message];

    /// Get shared file read state cache.
    pub fn read_file_state(&self) -> &Arc<RwLock<FileStateCache>>;

    /// Get accumulated permission denials (for SDK audit trail).
    pub fn permission_denials(&self) -> &[SdkPermissionDenial];
}
```

## Core Logic

### Multi-Turn Loop (from `QueryEngine.ts`)

```rust
impl QueryEngine {
    /// Main conversation loop. Runs until end_turn or budget exhausted.
    pub async fn run(
        &mut self,
        cancel: CancellationToken,
        event_tx: mpsc::Sender<QueryEvent>,
    ) -> Result<(), QueryError> {
        loop {
            // 1. Build system prompt
            let system = build_system_prompt(&self.config, &self.state);

            // 2. Normalize messages for API
            let api_messages = normalize_for_api(&self.messages);

            // 3. Call LLM via vercel-ai (streaming)
            let stream = self.api_client.query_streaming(QueryParams {
                messages: api_messages,
                model: self.config.model.clone(),
                system: Some(system),
                tools: Some(self.tool_registry.definitions()),
                thinking: self.config.thinking_config.clone(),
                ..Default::default()
            }, cancel.clone()).await?;

            // 4. Process stream events
            let assistant_msg = self.process_stream(stream, &event_tx).await?;
            self.messages.push(Message::Assistant(assistant_msg.clone()));

            // 5. Check stop reason
            match assistant_msg.stop_reason {
                Some(StopReason::EndTurn) => break,
                Some(StopReason::ToolUse) => {
                    // 6. Execute tools
                    let tool_results = run_tools(
                        &assistant_msg.tool_uses,
                        &assistant_msg,
                        &self.build_tool_context(),
                        cancel.clone(),
                    ).await;

                    // 7. Append results as user messages
                    for result in tool_results {
                        self.messages.push(result.into());
                    }
                }
                Some(StopReason::MaxTokens) => {
                    // 8. Retry with escalated budget or compact
                    self.handle_max_tokens().await?;
                }
                _ => break,
            }

            // 9. Auto-compact if needed
            if self.should_auto_compact() {
                self.compact(&event_tx).await?;
            }

            // 10. Check budget
            match check_budget(&self.budget_tracker, self.config.max_budget_usd, self.total_tokens()) {
                BudgetDecision::Stop { reason } => break,
                BudgetDecision::Nudge { message } => { /* inject nudge */ }
                BudgetDecision::Continue => {}
            }
        }
        Ok(())
    }
}
```

### Single-Turn Flow (from `query.ts`)

```rust
/// Execute a single query turn:
/// 1. Validate budget
/// 2. Build prompt (context + tools + memory)
/// 3. Call API (streaming)
/// 4. Collect assistant message
/// 5. Extract & execute tool calls (with permission hooks)
/// 6. Return results for next turn
pub async fn execute_turn(
    messages: &mut Vec<Message>,
    context: &ToolUseContext,
    config: &QueryConfig,
    cancel: CancellationToken,
) -> Result<TurnResult, QueryError>;

pub struct TurnResult {
    pub assistant_message: AssistantMessage,
    pub tool_results: Vec<Message>,
    pub should_continue: bool,
    pub compaction_needed: bool,
}
```

### Query Events (emitted to UI)

```rust
pub enum QueryEvent {
    StreamStart { model: String },
    TextDelta { text: String },
    ThinkingDelta { text: String },
    ToolUseStart { tool_name: String, tool_use_id: String },
    ToolUseEnd { tool_use_id: String },
    ToolResult { tool_use_id: String, result: String },
    TurnComplete { usage: TokenUsage, cost_usd: f64 },
    CompactStart,
    CompactEnd { tokens_before: i64, tokens_after: i64 },
    Error { message: String },
}
```
