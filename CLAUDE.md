# CLAUDE.md

Multi-provider LLM SDK and CLI. All development in `coco-rs/`.

> `cocode-rs/` and `codex-rs/` in this repo are reference implementations; active development is in `coco-rs/`.

## Commands

Run from `coco-rs/` directory:

```bash
just fmt          # After Rust changes (auto-approve)
just pre-commit   # REQUIRED before commit (fmt + check + clippy + test via nextest)
just test         # If changed vercel-ai or core crates
just test-crate <name>   # Scope to a single crate
just check        # Type-check all crates
just clippy       # Run clippy on all crates
just fix -p <name>       # Auto-fix clippy warnings for a single crate
just help         # All commands
```

## Code Style

### Format and Lint

- When using `format!` and you can inline variables into `{}`, always do that
  ```rust
  // Correct
  format!("{name} is {age}")
  // Avoid
  format!("{} is {}", name, age)
  ```
- Always collapse if statements per [collapsible_if](https://rust-lang.github.io/rust-clippy/master/index.html#collapsible_if)
- Use method references over closures when possible per [redundant_closure_for_method_calls](https://rust-lang.github.io/rust-clippy/master/index.html#redundant_closure_for_method_calls)
- When possible, make `match` statements exhaustive and avoid wildcard arms

### Integer Types

- Use `i32`/`i64` instead of `u32`/`u64` for most cases
- This avoids subtle overflow bugs and matches common API conventions

### Error Handling

- Never use `.unwrap()` in non-test code
- Use `?` for propagation or `.expect("reason")` with clear context
- Avoid mixing `Result` and `Option` without clear conversion
- See Error Handling section below for per-layer error types

### Serde Conventions

- Add `#[serde(default)]` for optional config fields
- Add `#[derive(Default)]` for structs used with `..Default::default()`
- Use `#[serde(rename_all = "snake_case")]` for enums

### Parameter Design

- Avoid bool or ambiguous `Option` parameters that force callers to write hard-to-read code such as `foo(false)` or `bar(None)`
- Prefer enums, named methods, newtypes, or other idiomatic Rust API shapes when they keep the callsite self-documenting

### Argument Comments

When you cannot avoid a small positional-literal callsite, use an exact `/*param_name*/` comment before opaque literal arguments:

```rust
// Good — self-documenting
connect(/*timeout*/ None, /*retries*/ 3, /*verbose*/ false)

// Bad — opaque
connect(None, 3, false)
```

Rules:
- Add `/*param*/` for `None`, booleans, and numeric literals passed by position
- Do not add for string or char literals unless the comment adds real clarity
- The parameter name must exactly match the callee signature

### Module Size

- Target Rust modules under 500 LoC (excluding tests)
- If a file exceeds ~1400 LoC, add new functionality in a new module instead of extending the existing file
- Prefer adding new modules over growing existing ones

### Comments

- Keep concise — describe purpose, not implementation details
- Field docs: 1-2 lines max, no example configs/commands
- Code comments: state intent only when non-obvious

## Architecture

```
┌──────────────────────────────────────────────────────────────────────┐
│  App: cli, tui, session, query, state                                │
├──────────────────────────────────────────────────────────────────────┤
│  Root Modules: commands, skills, hooks, tasks, memory, plugins,      │
│                keybindings                                            │
├──────────────────────────────────────────────────────────────────────┤
│  Core: tool → tools, permissions, messages, context                  │
├──────────────────────────────────────────────────────────────────────┤
│  Services: inference, compact, mcp, rmcp-client, mcp-types, lsp      │
│  Exec:     shell, sandbox, process-hardening, exec-server,           │
│            apply-patch                                                │
│  Standalone: bridge, retrieval                                        │
├──────────────────────────────────────────────────────────────────────┤
│  Vercel AI: ai → openai, openai-compatible, google, anthropic,       │
│             bytedance (on provider + provider-utils)                  │
├──────────────────────────────────────────────────────────────────────┤
│  Common: types, config, error, otel, stack-trace-macro                │
│  Utils: 26 utility crates (see table below)                           │
└──────────────────────────────────────────────────────────────────────┘
```

Total: **69 crates**. Arrows indicate primary dependency direction (bottom depends on nothing above it).

## Key Data Flows

### Agent Turn Lifecycle (driven by `app/query::QueryEngine`)

```
User input
  → ConversationContext.build()                         [core/context]
  → MessageHistory normalization + attachment injection [core/messages, core/context]
  → ApiClient.query(QueryParams)                        [services/inference → vercel-ai]
  → Parse stream → StreamAccumulator → tool calls       [app/query]
  → StreamingToolExecutor (via ToolBatch):
      safe tools → execute concurrently                 [core/tool]
      unsafe tools → queue, execute after stop          [core/tool]
  → Hook orchestration: PreToolUse / PostToolUse        [hooks, core/tool::HookHandle]
  → Tool results → MessageHistory                       [core/messages]
  → emit CoreEvent (Protocol / Stream / Tui)            [app/query::emit]
  → Check ContinueReason (NextTurn / ReactiveCompactRetry /
      MaxOutputTokensEscalate / TokenBudgetContinuation)
  → If compaction needed → micro / full / reactive      [services/compact]
  → Drain CommandQueue + re-inject attachments          [app/query]
  → If tool calls exist → loop back
```

### Configuration Resolution

```
~/.coco/{provider,model,config}.json + env + CLI overrides
  → ConfigLoader → Settings (+ SettingsWithSource)      [common/config]
  → EnvOnlyConfig → RuntimeOverrides → ResolvedConfig
  → GlobalConfig (hot-reload via SettingsWatcher)
  → ModelRoles (8 roles) + ModelAlias + FastModeState
  → BootstrapConfig → app/query → services/inference
```

### Provider Call Chain

```
QueryEngine → ApiClient                                 [services/inference]
  ↓ (uses Arc<dyn LanguageModelV4>)
vercel-ai high-level (generate_text / stream_text)      [vercel-ai/ai]
  ↓
LanguageModelV4 impl                                    [vercel-ai/{openai,anthropic,
                                                          google,bytedance,openai-compatible}]
  ↓ (via vercel-ai-provider-utils: Fetch, ResponseHandler)
HTTP request → provider API → typed stream events
  ↓
CacheBreakDetector + UsageAccumulator                   [services/inference]
  → StopReason → QueryResult
```

### Shell Execution Flow

```
BashTool → ShellExecutor.execute(command)               [core/tools, exec/shell]
  → tokenize → parse → BashNode AST
  → security checks (SecurityCheckId, 23 analyzers)     [utils/shell-parser]
  → read-only rules + destructive warnings + sed checks
  → mode_validation (accept_edits / plan / default)
  → SandboxSettings check                               [exec/sandbox]
  → spawn via tokio process                             [exec/shell::executor]
  → ShellProgress (stdout, stderr, exit_code)
  → CommandResult + CommandResultInterpretation
```

### MCP Integration Flow

```
McpConnectionManager.connect(server_name)               [services/mcp]
  → McpConfigLoader (stdio / HTTP / SSE)
  → RmcpClient state machine (Connecting → Ready)       [services/rmcp-client]
  → OAuthConfig / OAuthTokens + xaa IDP login (auth)
  → DiscoveryCache (tools, resources, capabilities)
  → convert_mcp_tool_to_tool_def → ToolRegistry
  → Tool call: ChannelPermissionRelay check →
      McpConnectionManager.call_tool() → rmcp → server
  → Elicitation requests surfaced via ElicitationMode
```

### Background Task Lifecycle

```
TaskManager (with_event_sink)                           [tasks]
  → create(TaskType, TaskStateBase)
  → update_status(TaskStatus) / set_output(TaskOutput)
  → emits CoreEvent::Protocol(TaskStarted | TaskProgress |
      TaskCompleted) through mpsc::Sender<CoreEvent>
  → SDK consumers receive via NDJSON stream
```

## Crate Guide (69 crates)

### Common (5)

| Crate | Purpose | Key Types |
|-------|---------|-----------|
| `types` | Foundation types shared across all crates (zero internal deps; re-exports vercel-ai types as version-agnostic aliases) | `Message`, `UserMessage`, `AssistantMessage`, `PermissionMode`, `CommandBase` (41 tool variants), `HookEventType`, `SandboxMode`, `TokenUsage`, `ThinkingLevel`, `ProviderApi`, `ModelRole`, `Capability`, `ToolAppState`, `AppStatePatch`, `AgentDefinition`, `SubagentType`, `CoreEvent` (3-layer: Protocol/Stream/Tui) |
| `config` | Layered config: JSON files + env + runtime overrides + hot reload | `Settings`, `SettingsWithSource`, `SettingSource`, `GlobalConfig`, `ResolvedConfig`, `BootstrapConfig`, `EnvOnlyConfig`, `RuntimeOverrides`, `ModelInfo`, `ProviderInfo`, `ProviderConfig`, `ModelRoles`, `ModelAlias`, `FastModeState`, `CooldownReason`, `PlanModeSettings`, `PlanModeWorkflow`, `PlanPhase4Variant`, `SessionSettings`, `SettingsWatcher`, `AnalyticsPipeline`, `SessionAnalytics` |
| `error` | Unified errors with StatusCode classification | `StatusCode` (5-digit `XX_YYY`), `ErrorExt` trait (`status_code()`, `is_retryable()`, `retry_after()`, `output_msg()`), snafu + snafu-virtstack |
| `otel` | OpenTelemetry tracing and metrics | `OtelManager` (counters, histograms, timers, session span) |
| `stack-trace-macro` | Proc macro for snafu error enums with virtual stack traces | `#[stack_trace_debug]` (proc macro attribute) |

### Vercel AI (8)

| Crate | Purpose | Key Types |
|-------|---------|-----------|
| `vercel-ai-provider` | Standalone types matching `@ai-sdk/provider` v4 (no coco deps) | `LanguageModelV4` trait, `EmbeddingModelV4` trait, `ImageModelV4` trait, `ProviderV4` trait, `LanguageModelV4Prompt`, `UserContentPart` / `AssistantContentPart` / `ToolContentPart`, `ProviderOptions`, `ProviderMetadata`, `AISdkError` |
| `vercel-ai-provider-utils` | Utilities for implementing AI SDK v4 providers | `ApiResponse`, `ResponseHandler` / `JsonResponseHandler` / `StreamResponseHandler`, `ErrorHandler`, `Fetch` / `FetchOptions`, `Schema` / `ValidationError`, `ToolMapping` |
| `vercel-ai` | High-level SDK matching `@ai-sdk/ai` | `generate_text`, `stream_text`, `generate_object`, `stream_object`, `embed`, `embed_many`, `rerank`, `generate_image`, `generate_speech`, `transcribe`; `GenerateTextOptions` / `Result`, `StreamTextOptions`, `GenerateObjectOptions`, `OutputStrategy`, global default-provider pattern |
| `vercel-ai-openai` | OpenAI provider for Vercel AI SDK v4 | `OpenAIProvider`, `OpenAIProviderSettings`, `OpenAIChatLanguageModel`, `OpenAIResponsesLanguageModel`, `OpenAIEmbeddingModel`, `OpenAIImageModel` |
| `vercel-ai-openai-compatible` | Generic OpenAI-compatible provider (xAI, Groq, Together, etc.) | `OpenAICompatibleProvider`, `OpenAICompatibleProviderSettings`, `OpenAICompatibleChatLanguageModel`, `MetadataExtractor`, `StreamMetadataExtractor` |
| `vercel-ai-google` | Google Gemini provider | `GoogleGenerativeAIProvider`, `GoogleGenerativeAIProviderSettings`, `GoogleGenerativeAILanguageModel`, `GoogleGenerativeAIEmbeddingModel`, `GoogleGenerativeAIImageModel` |
| `vercel-ai-anthropic` | Anthropic Claude provider | `AnthropicProvider`, `AnthropicProviderSettings`, `AnthropicMessagesLanguageModel`, `AnthropicConfig`, `CacheControlValidator` |
| `vercel-ai-bytedance` | ByteDance video provider (Seedance) via ModelArk API | `ByteDanceProvider`, `ByteDanceProviderSettings`, `ByteDanceVideoModel`, `ByteDanceVideoModelConfig` |

### Services (6)

| Crate | Purpose | Key Types |
|-------|---------|-----------|
| `inference` | Thin multi-provider wrapper over vercel-ai: generic retry + usage aggregation + tool-schema generation + cache-break detection. **Auth, prompt caching, provider-specific betas/rate-limits/policy limits live in the vercel-ai provider crates, NOT here.** | `ApiClient` (wraps `Arc<dyn LanguageModelV4>`), `QueryParams` (prompt, max_tokens, thinking_level, fast_mode, tools), `QueryResult`, `RetryConfig`, `UsageAccumulator`, `CacheBreakDetector` / `CacheBreakResult` / `CacheState`, `StreamEvent`, `StopReason`, `GeneratedSchemas`, `ToolSchemaSource`, `InferenceError`, `merge_provider_options()` |
| `compact` | Context compaction: full (LLM summarize), micro (tool-result clearing), reactive (on `prompt_too_long`), session-memory, plus auto-trigger | `CompactConfig`, `CompactResult`, `CompactError`, `ContextEditStrategy`, `MicroCompactBudgetConfig`, `MicrocompactResult`, `TokenWarningState`, `ReactiveCompactConfig` / `ReactiveCompactState`, `SessionMemoryCompactConfig`, `CompactionObserver` / `CompactionObserverRegistry`, `compact_conversation()`, `micro_compact()`, `should_auto_compact()` |
| `mcp` | MCP server lifecycle, config, discovery, OAuth (including xaa IDP), elicitation, channel permissions | `McpConnectionManager`, `McpClientError`, `McpConfigLoader`, `DiscoveryCache`, `DiscoveredTool`, `DiscoveredResource`, `ServerCapabilities`, `ToolAnnotations`, `OAuthConfig`, `OAuthTokens`, `OAuthTokenStore`, `ChannelPermission` / `ChannelPermissionRelay`, `ElicitationRequest` / `ElicitResult` / `ElicitationMode`, `McpConfigChanged` |
| `mcp-types` | Auto-generated MCP message types (from spec) | `InitializeRequest` / `Result`, `CallToolRequest` / `Result`, `ListToolsRequest` / `Result`, `ModelContextProtocolRequest` trait |
| `rmcp-client` | MCP client: stdio + HTTP/SSE transport, OAuth persistence | `RmcpClient` (state machine: Connecting → Ready), dual transport, `OAuthPersistor` (auto-refresh), bidirectional conversion with `mcp-types` |
| `lsp` | AI-friendly LSP client (query by name+kind, not position) | `LspServerManager`, `LspClient`, `SymbolKind`, `ResolvedSymbol`, `DiagnosticsStore` (debounce), `ServerLifecycle` (max restarts, exponential backoff), `BUILTIN_SERVERS` (rust-analyzer, gopls, pyright, typescript-language-server) |

### Core (5)

| Crate | Purpose | Key Types |
|-------|---------|-----------|
| `tool` | Tool trait, streaming executor, registry; callback handles (interface layer) | `Tool` trait, `ToolUseContext`, `ToolError`, `SyntheticToolError`, `StreamingToolExecutor`, `ToolRegistry`, `ToolBatch`, `BatchResult`, `ToolCallResult`, `ToolStatus`, `PendingToolCall`, `AgentHandle` / `AgentSpawnRequest` / `AgentSpawnResponse`, `AgentQueryEngine`, `HookHandle` (Pre/PostToolUseOutcome, HookPermission), `MailboxHandle` (`MailboxEnvelope`, `InboxMessage`), `McpHandle` (`McpToolSchema`, `McpToolAnnotations`), `ToolPermissionBridge` (`ToolPermissionRequest` / `Decision` / `Resolution`), `PlanApprovalMessage` / `Request` / `Response`, `ScheduleStore`, `SideQuery` |
| `tools` | 42 built-in tool implementations (41 static + MCPTool dynamic wrapper) | File I/O (7): Bash/Read/Write/Edit/Glob/Grep/NotebookEdit; Web (2): WebFetch/WebSearch; Agent (5): Agent/Skill/SendMessage/TeamCreate/TeamDelete; Task (7): TaskCreate/Get/List/Update/Stop/Output/TodoWrite; Plan & Worktree (4): Enter/ExitPlanMode, Enter/ExitWorktree; Utility (5): AskUserQuestion/ToolSearch/Config/Brief/Lsp; MCP mgmt (3): McpAuth/ListMcpResources/ReadMcpResource; Scheduling (4): CronCreate/Delete/List/RemoteTrigger; Shell (4): PowerShell/Repl/… . Input enums: `GrepOutputMode`, `ConfigAction`, `LspAction` |
| `permissions` | Permission evaluation, 2-stage auto-mode/yolo classifier, denial tracking, bypass killswitch | `PermissionEvaluator`, `AutoModeDecision`, `AutoModeInput`, `AutoModeState`, `AutoModeRules`, `ClassifyRequest`, `YoloClassifierResult`, `classify_for_auto_mode()` (XML LLM), `DenialTracker`, `ExplainerParams`, `InitialPermissionMode`, `KillswitchCheck`, `compute_bypass_capability()`, `rule_compiler`, shell-rules, dangerous-rules, shadowed-rules |
| `messages` | Message creation (13), normalization (10), filtering (11), predicates (19), history, lookups, cost tracking | `MessageHistory`, `MessageLookups`, `CostTracker`, `calculate_cost_usd()`, `create_user_message()` / `create_assistant_message()` / `create_tool_result_message()` / `create_meta_message()` / … , `normalize_messages_for_api()`, `to_llm_prompt()`, `strip_images_from_messages()` |
| `context` | System context assembly, CLAUDE.md discovery, attachments, plan-mode reminders, file history | `ConversationContext`, `EnvironmentInfo`, `GitStatus`, `Platform`, `ShellKind`, `ClaudeMdFile` / `ClaudeMdSource`, `discover_claude_md_files()`, `Attachment` / `AttachmentBatch` / `AttachmentBudget` / `AttachmentSource` / `AttachmentDeduplicator`, `PlanModeAttachment` / `PlanModeExitAttachment` / `Phase4Variant` / `PlanWorkflow` / `ReminderType`, `FileHistorySnapshot` / `FileHistoryBackup` / `DiffStats`, `FileReadCache` |

### Exec (5)

| Crate | Purpose | Key Types |
|-------|---------|-----------|
| `shell` | Shell execution with security analysis, destructive warnings, sandbox decisions, CWD tracking | `ShellExecutor`, `ShellProgress`, `CommandResult`, `ExecOptions`, `BashNode` / `SimpleCommand`, `SafetyResult`, `SecurityCheck` / `SecurityCheckId` / `SecuritySeverity`, `HeredocContent`, `SedEditInfo`, `CommandResultInterpretation`, destructive command warnings, read-only rules, plan/accept-edits mode validation |
| `sandbox` | Sandbox config (disabled by default, three modes) | `SandboxMode` (None/ReadOnly/Strict), `SandboxConfig`, `SandboxSettings`, `PermissionChecker`, `SandboxPlatform` trait |
| `process-hardening` | OS-level security (prctl, ptrace deny, env sanitization) | Platform-specific hardening (macOS: PT_DENY_ATTACH, Linux: prctl) |
| `exec-server` | Minimal filesystem abstraction ported from codex-rs so `coco-apply-patch` can consume a shared `ExecutorFileSystem` trait | `ExecutorFileSystem` trait (async, tokio-backed), `LOCAL_FS` static, `LocalFileSystem`, `FileMetadata`, `ReadDirectoryEntry`, `CreateDirectoryOptions`, `RemoveOptions`, `CopyOptions`, `FileSystemResult` |
| `apply-patch` | Unified diff/patch application with fuzzy matching (ported from codex-rs; lives under `exec/` because it performs filesystem side-effects) | `ApplyPatchError`, `Hunk`, `ParseError`, `parse_patch()`, `maybe_parse_apply_patch_verified()`, `APPLY_PATCH_TOOL_INSTRUCTIONS`, `COCO_CORE_APPLY_PATCH_ARG1` self-invocation flag |

### Root Modules (7)

| Crate | Purpose | Key Types |
|-------|---------|-----------|
| `commands` | Slash command registry + ~96 command implementations across v1/v2/v3 | `CommandHandler` trait (`execute(&str) -> Result<String>`), `RegisteredCommand`, `CommandType`, `CommandBase`, `IsEnabledFn` feature gate. Commands: help/config/clear/diff/doctor/compact/session/login/mcp/plan/plugin/tasks/agents/skills/… |
| `skills` | Skill markdown workflows from bundled / project / user / plugin sources | `SkillDefinition` (name, description, prompt, source, aliases, allowed_tools, model, when_to_use, params), `SkillContext` (Inline / Fork), `SkillSource`, `SkillManager` (`Arc<Mutex>`), bundled skills, watcher |
| `hooks` | Pre/post event interception with scoped priority, async registry, SSRF guard | `HookDefinition` (event, matcher, handler, priority, scope, if_condition, once, is_async, async_rewake, shell, status_message), `HookHandler` (Command / Prompt / Http / Agent), `HookScope` (Session > Local > Project > User > Builtin), `HookEventType`, `AsyncHookRegistry`, orchestration module |
| `tasks` | Three task-state kinds: running background tasks (`running`), durable plan items on disk (`task_list`), per-agent V1 checklists (`todos`) | `running::TaskManager` (Arc+RwLock HashMap, optional `CoreEvent` sink via `with_event_sink()`); `task_list::{TaskListStore, Task, TaskStatus, TaskUpdate, ClaimResult, resolve_task_list_id, TaskHookSink}` (fs2 lockfile + highwatermark, matching TS `utils/tasks.ts`); `todos::{TodoStore, TodoItem, should_nudge_verification}` (per-agent ephemeral); `handle_impls` wires `coco_tool::TaskListHandle` + `TodoListHandle` |
| `memory` | Persistent cross-session knowledge: CLAUDE.md mgmt, auto-extraction, session memory, auto-dream (KAIROS), team sync | `MemoryEntry`, `MemoryEntryType` (User / Feedback / Project / Reference), `MemoryManager`, `MemoryConfig`, `ExtractionHook`, `PrefetchState`, `StalenessInfo`, `ScannedMemory`, `MemoryEvent`. 18 modules: auto_dream, classify, config, hooks, kairos, memdir, permissions, prefetch, prompt, scan, security, session_memory, staleness, team_paths, team_prompts, team_sync, telemetry |
| `plugins` | Plugin system via `PLUGIN.toml` manifests (contributions, marketplace, hot-reload) | `PluginManifest` (name, version, description, skills, hooks, mcp_servers), `LoadedPlugin`, `PluginSource` (Builtin / User / Project / Marketplace), `PluginLoader`, command / hook / skill bridges, marketplace |
| `keybindings` | Keyboard shortcuts with context-based resolution and chord support | `Keybinding` (key, action, context, when), `KeyChord`, `KeyCombo`, `ChordResolver`, `ResolveOutcome`, `ValidationIssue`, 18 contexts, 73+ actions, `load_default_keybindings()` |

### App (5)

| Crate | Purpose | Key Types |
|-------|---------|-----------|
| `cli` | CLI entry point (clap), transports (SSE / WS / NDJSON), server / daemon / SDK modes | `Cli` (clap), `Commands`, binary name `coco`, `CliInitializeBootstrap`, `TransportError`, profile support, `sdk_server` module |
| `tui` | Terminal UI using Elm architecture (TEA) with ratatui + rust-i18n locales | `App` (async run loop), `AppState` (model; legacy), `TuiEvent` / `TuiCommand` (messages: keyboard / agent / file-search / server-notification), `UserCommand` (TUI→Core: SubmitInput / Interrupt / ApprovalResponse), `ClearScope`, `Animation`, `ImageData`, overlays, vim mode, streaming |
| `session` | Session persistence, title generation, transcript recovery | `Session`, `SessionManager`, `HistoryEntry`, `PromptHistory`, `TranscriptStore` (`TranscriptEntry`, `TranscriptMetadata`, `TranscriptUsage`, `MetadataEntry`, `ModelCostEntry`), `RestoredCostSummary`, `restore_cost_from_transcript()`, `title_generator` |
| `query` | Multi-turn agent loop driver (`QueryEngine`) + single-turn execution + budget + command queue | `QueryEngine`, `QueryEngineConfig`, `QueryResult`, `SessionBootstrap`, `ContinueReason` (NextTurn / ReactiveCompactRetry / MaxOutputTokensEscalate / TokenBudgetContinuation), `BudgetTracker` / `BudgetDecision`, `CommandQueue`, `Inbox` / `InboxMessage`, `QueuedCommand` / `QueuePriority`, `QueryGuard` / `QueryGuardStatus`, `StreamAccumulator`, emits `CoreEvent` / `AgentStreamEvent` / `ServerNotification` |
| `state` | Central application-state tree + swarm orchestration | `AppState` (80+ fields across model / session / agent / token / tasks / MCP / plugins / notifications / remote / feature flags), `McpClientState`, `PluginState`, `NotificationState`, `TaskEntry`, `InboxEntry`, `TeamContext`, `TeammateEntry`, `StandaloneAgentContext`, `PendingWorkerRequest` / `PendingSandboxRequest`, `WorkerSandboxPermissions`, `SandboxQueueEntry`, `ElicitationEntry`, `SubAgentState` / `Status`, `AgentMessage`, `IdleReason`. 21 swarm modules (runner, agent_handle, backend (iterm2/pane/tmux), config, discovery, identity, layout, mailbox, prompt, reconnect, spawn_utils, task, teammate, …) |

### Standalone (2)

| Crate | Purpose | Key Types |
|-------|---------|-----------|
| `bridge` | IDE bridge (VS Code / JetBrains), REPL bridge for SDK / daemon callers, JWT auth, trusted-device store | `BridgeInMessage` / `BridgeOutMessage`, `BridgeTransport`, `ReplBridge`, `ReplInMessage` / `ReplOutMessage`, `BridgeState`, `BridgeServer`, `ControlRequest` / `ControlRequestHandler`, `BridgePermissionRequest` / `Response` / `Decision` / `RiskLevel`, `Claims` (JWT), `TrustedDevice` / `TrustedDeviceStore`, `work_secret` helpers |
| `retrieval` | Code search: BM25 + vector + AST + RepoMap (PageRank) via Facade pattern, isolated `RetrievalEvent` stream | `RetrievalFacade` (`search`, `build_index`, `generate_repomap`), `SearchRequest` (fluent: `.bm25()` / `.vector()` / `.hybrid()` / `.snippet()` / `.limit()`), `CodeChunk`, `IndexManager`, `RepoMapGenerator`, `RetrievalFeatures` presets (NONE/MINIMAL/STANDARD/FULL), `RetrievalErr` (`is_retryable`, `suggested_retry_delay_ms`). Features: `local-embeddings`, `neural-reranker` |

### Utils (26)

| Crate | Purpose |
|-------|---------|
| `absolute-path` | Absolute path types with deserialization support |
| `async-utils` | Async runtime utilities and cancellation helpers |
| `cache` | LRU cache with Tokio mutex protection |
| `cargo-bin` | Cargo binary helpers for test harnesses |
| `common` | Shared cross-crate utility functions |
| `cursor` | Text cursor with kill ring (Ctrl+K/Y), word boundaries, UTF-8 safe |
| `file-encoding` | File encoding and line-ending detection/preservation |
| `file-ignore` | .gitignore-aware file filtering (unified ignore service) |
| `file-search` | Fuzzy file search with nucleo and gitignore support |
| `file-watch` | Generic reusable file-watch infrastructure |
| `frontmatter` | YAML frontmatter parser for skills, commands, agents, memory files |
| `git` | Git operations wrapper |
| `image` | Image processing utilities |
| `json-to-toml` | JSON to TOML conversion |
| `keyring-store` | Secure credential storage using system keyring |
| `pty` | Pseudo-terminal handling |
| `readiness` | Readiness flag with token-based auth and async waiting |
| `rustls-provider` | TLS provider init via rustls crypto ring |
| `secret-redact` | Secret redaction (OpenAI, Anthropic, GitHub, Slack, AWS, bearer tokens) |
| `shell-parser` | Shell command parsing and security analysis (24 analyzers) |
| `sleep-inhibitor` | Cross-platform sleep prevention (macOS/Linux/Windows) |
| `stdio-to-uds` | Bridge stdio streams to Unix domain sockets |
| `stream-parser` | Stream parsing (text, citation, inline hidden tag, proposed plan, UTF-8) |
| `string` | String truncation and boundary utilities |
| `symbol-search` | Symbol search for code navigation |
| `test-harness` | Test harness utilities for integration tests |

## Key Design Patterns

| Pattern | Where | Details |
|---------|-------|---------|
| **Builder** | Most crates | `QueryEngine::new()`, `SearchRequest` (fluent), `RetrievalFacade` |
| **Arc-heavy sharing** | core/, root modules, app/ | Registries, managers, trackers: `Arc<Mutex<T>>` or `Arc<RwLock<T>>` (e.g. `TaskManager`, `SkillManager`, `AppState`, `McpConnectionManager`) |
| **Event-driven** | query, tui, tasks | `mpsc::Sender<CoreEvent>` sinks, `tokio::select!` multiplexing, `with_event_sink()` opt-in emitters |
| **3-layer event dispatch** | types, query | `CoreEvent::Protocol` (SDK NDJSON), `Stream` (agent stream), `Tui` (terminal updates) — single source emits, consumers pick layer |
| **Cancellation** | All async | `CancellationToken` threaded through all layers |
| **Registry** | core/tool, commands, skills, plugins, keybindings, services/mcp | `ToolRegistry`, `CommandRegistry`, `SkillManager`, `PluginLoader`, `ChordResolver`, `DiscoveryCache` |
| **State Machine** | query, permissions, mcp | `ContinueReason` (loop control), `AutoModeState`, `RmcpClient` (Connecting/Ready) |
| **Callback decoupling** | core/tool | `AgentHandle`, `HookHandle`, `MailboxHandle`, `McpHandle`, `ToolPermissionBridge`, `ScheduleStore`, `SideQuery` — avoid tool→subsystem circular deps; `NoOp*` test doubles provided |
| **Permission pipeline** | permissions, tool | `Tool.check_permission()` → 2-stage auto-mode / yolo XML LLM classifier → `DenialTracker` + bypass killswitch |
| **Facade** | retrieval | Single `RetrievalFacade` entry point hides search + index + repomap + reranker |
| **Elm (TEA)** | tui | Model (`AppState`) + Message (`TuiEvent`) + Update (`handle_command`) + View (`render`) |
| **Middleware** | vercel-ai | `FnOnce` + `BoxFuture` callbacks for `do_generate`/`do_stream` delegation |
| **Typed extension slots** | vercel-ai providers | `ProviderOptions` / `ProviderMetadata` = `serde_json::Value` on purpose (pass-through for unknown provider fields) |
| **Isolated event stream** | retrieval | `RetrievalEvent` intentionally not bridged into `CoreEvent` — subscribe via `EventEmitter` instead |

## Error Handling

| Layer | Error Type |
|-------|------------|
| common/, core/, services/ | `coco-error` + snafu + snafu-virtstack (StatusCode `XX_YYY` classification, retryable flag) |
| root modules | snafu + `coco-error` |
| utils/ | `anyhow::Result` |
| vercel-ai/ | `thiserror` (standalone, no coco deps) |
| app/, exec/, standalone | `anyhow::Result` (retrieval uses its own `RetrievalErr`; apply-patch uses `thiserror`) |

StatusCode categories: General (00-05), Config (10), Provider (11), Resource (12). See [common/error/README.md](coco-rs/common/error/README.md).

## Testing

### Test Assertions

- Use `pretty_assertions::assert_eq` for clearer diffs
- Prefer comparing entire objects over individual fields
  ```rust
  // Correct
  assert_eq!(actual, expected);
  // Avoid
  assert_eq!(actual.name, expected.name);
  assert_eq!(actual.value, expected.value);
  ```
- Avoid mutating process environment in tests; prefer passing flags or dependencies

### Test Organization

- **Separate test files** (MANDATORY): Never write `#[cfg(test)] mod tests { ... }` inline. Always use `#[path]` to keep source files focused:
  ```rust
  // At the end of implementation.rs
  #[cfg(test)]
  #[path = "implementation.test.rs"]
  mod tests;
  ```
  Tests go in the companion `implementation.test.rs` file in the same directory. This applies to ALL crates with no exceptions.
- Integration tests in `tests/` directory
- Use descriptive test names: `test_<function>_<scenario>_<expected>`

### Test & Lint Workflow

- Test the specific changed crate first: `just test-crate coco-<name>`
- Only run full `just test` if changes affect shared crates (common/, core/, services/)
- Use `just fix -p coco-<name>` for scoped clippy fixes; only run `just fix` without `-p` if you changed shared crates

### Snapshot Tests

This repo uses snapshot tests (via `insta`) to validate rendered output, especially in TUI.

- Any change that affects user-visible UI must include corresponding `insta` snapshot coverage
- Run tests to generate updated snapshots: `cargo test -p coco-tui`
- Check pending: `cargo insta pending-snapshots -p coco-tui`
- Review by reading `*.snap.new` files directly, or: `cargo insta show -p coco-tui path/to/file.snap.new`
- Accept: `cargo insta accept -p coco-tui`
- Install if missing: `cargo install cargo-insta`

## TUI Conventions

### Styling (ratatui)

- Use Stylize helpers: `"text".dim()`, `.bold()`, `.cyan()`, `.italic()`, `.underlined()` instead of manual `Style`
- Simple conversions: `"text".into()` for spans, `vec![...].into()` for lines
- Computed styles: `Span::styled` or `Span::from(text).set_style(style)` is OK when style is runtime-computed
- Avoid hardcoded `.white()` — prefer the default foreground (no color)
- Chain for readability: `url.cyan().underlined()`
- Use `Line::from(text)` or `Span::from(text)` only when the target type isn't obvious from context
- Avoid churn: don't refactor between equivalent forms without a clear readability gain
- Compactness: prefer the form that stays on one line after rustfmt

### Text Wrapping

- Use `textwrap::wrap` for plain strings
- For indentation, use `initial_indent` / `subsequent_indent` options rather than custom logic

## Async Conventions

- Use `tokio::task::spawn_blocking` for blocking operations
- Prefer `tokio::sync` primitives over `std::sync` in async contexts
- Add `Send + Sync` bounds to traits used with `Arc<dyn Trait>`

## Dependencies

- Prefer well-maintained crates with active development
- Check for security advisories before adding
- Use workspace dependencies when possible

| Purpose | Crate |
|---------|-------|
| Async runtime | `tokio` |
| HTTP client | `reqwest` |
| JSON | `serde_json` |
| Error handling | `anyhow`, `snafu`, `thiserror` |
| Logging | `tracing` |
| Testing | `pretty_assertions`, `insta`, `wiremock` |
| Terminal UI | `ratatui`, `crossterm` |
| MCP | `rmcp` |

## Design Decisions

### Code Hygiene

| Rule | Note |
|------|------|
| **No deprecated code** | Delete obsolete code outright. No `#[deprecated]`, no backward-compat shims. |
| **No inline tests** | Extract to `<name>.test.rs`; reference via `#[cfg(test)] #[path = "<name>.test.rs"] mod tests;`. Never `#[cfg(test)] mod tests { ... }` inline. |
| **No `unsafe`** | All code is safe Rust. Wrap any `unsafe` dependency inside its own crate. If truly unavoidable, discuss first. |
| **No single-use helpers** | Inline at the call site instead of naming a function used once. |

### Type Safety

- **No hardcoded strings for closed sets** (tool names, event types, config keys, protocol discriminators). In order of preference:
  1. **Enum + `.as_str()`** — e.g. `CommandBase::Read.as_str()`, `HookEventType::PreToolUse.as_str()`.
  2. **Module constants** (`pub const X: &str = "..."`) — when the canonical enum lives in a crate you can't depend on.
  3. **Typed struct** instead of `serde_json::Value` map — see below.

  Raw string literals only for unconstrained input (user text, opaque external IDs, third-party wire formats). Typos in magic strings silently desync crates and defeat IDE rename.

- **Typed structs over `serde_json::Value`**: if a payload is produced *and* consumed inside coco-rs, make it a struct. Use `Option<T>` + `#[serde(default, skip_serializing_if = "Option::is_none")]` for optional fields, `#[serde(tag = "type")]` for variants. Reference migration: `ToolAppState` (8 fields, 3 crates) moved from `Arc<RwLock<Value>>` → compile-checked struct.

  **Exception**: `vercel-ai-*` provider-extension slots (`ProviderOptions`, `ProviderMetadata`, raw provider responses, model-specific blobs like Anthropic `thinking.budget_tokens`) keep `Value` — they're deliberate pass-through points. Unpack to typed struct at the coco-rs boundary; never let `Value` leak inward.

### Multi-Provider Boundaries

- **Provider concerns stay in provider crates.** OAuth, API-key helpers, cloud credentials (Bedrock/Vertex/Foundry), prompt-cache breakpoint detection, beta headers, 529-capacity retry, rate-limit messaging, and Claude.ai/Anthropic policy limits live in `vercel-ai-<provider>` crates — **not** `services/inference`. When porting TS, skip `services/api/`, `services/oauth/`, `services/policyLimits/`, `services/claudeAiLimits*`, `services/rateLimitMessages*`, `utils/auth.ts`, `services/api/promptCacheBreakDetection.ts`, `utils/betas.ts`. `services/inference` owns only generic concerns: retry shape, usage aggregation, thinking-level conversion, streaming composition, tool-schema generation.

- **Models are `(provider, api, model_id)`, never a bare string.** Always go through `coco_config::ModelRoles::get(ModelRole::X)`. The 8 roles — `Main`, `Fast`, `Compact`, `Plan`, `Explore`, `Review`, `HookAgent`, `Memory` — are the only way to address "which model runs this". Never add a `title_model: String` config key; expose a `bool` flag and route internally via the appropriate role. Add a new `ModelRole` variant rather than a raw string. TS `queryHaiku()` maps to `ModelRole::Fast`; TS specific-model summarizers map to `ModelRole::Compact`, etc.

- **Compaction — three generic strategies only**: micro-compact (clear old tool results), full LLM summarization, reactive (on `prompt_too_long`). Do **not** port TS `HISTORY_SNIP` or `CONTEXT_COLLAPSE` — they're Anthropic cache-aware optimizations tied to the prompt-cache + protected-tail architecture. Provider-specific cache-aware compaction belongs in that `vercel-ai-*` crate.

- **Plan Mode — skip Ultraplan only.** Port the core lifecycle, Pewter-ledger (Phase-4 variants `null`/`trim`/`cut`/`cap`), and Interview phase — but gate them on `settings.json` keys (`plan_mode.phase4_variant`, `plan_mode.workflow`), not GrowthBook or `USER_TYPE=ant`. Skip every `feature('ULTRAPLAN')` path (CCR web-UI refinement flow) — it requires the Anthropic CCR backend coco-rs doesn't ship. Re-root `planModeV2.ts` helpers on `settings.json`.

### Event System

- **Single `CoreEvent` enum, three dispatch layers.** `Protocol` (SDK NDJSON stream), `Stream` (agent content stream), `Tui` (terminal UI updates). Emitters produce a `CoreEvent`; consumers pick the layer they care about. `QueryEngine::emit_*` is the reference emitter; see `event-system-design.md`.
- **Opt-in lifecycle emitters.** Background subsystems (`TaskManager`, future retrieval sinks) expose `with_event_sink(mpsc::Sender<CoreEvent>)` builders — zero overhead when not subscribed.
- **Isolated event streams stay isolated.** `RetrievalEvent` and `vercel-ai` callbacks (`OnStartEvent`, `OnStepFinishEvent`, `OnFinishEvent`, `OnErrorEvent`) are **not** bridged into `CoreEvent`. If you need cross-subsystem progress in the agent stream, add a single aggregate variant through an opt-in sink — don't bridge the full taxonomy.

## Specialized Documentation

Every crate in `coco-rs/` has its own `CLAUDE.md`. Links below.

### Common
- [types](coco-rs/common/types/CLAUDE.md)
- [config](coco-rs/common/config/CLAUDE.md)
- [error](coco-rs/common/error/CLAUDE.md) · [error codes README](coco-rs/common/error/README.md)
- [otel](coco-rs/common/otel/CLAUDE.md)
- [stack-trace-macro](coco-rs/common/stack-trace-macro/CLAUDE.md)

### Vercel AI
- [ai (high-level SDK)](coco-rs/vercel-ai/ai/CLAUDE.md)
- [provider](coco-rs/vercel-ai/provider/CLAUDE.md)
- [provider-utils](coco-rs/vercel-ai/provider-utils/CLAUDE.md)
- [openai](coco-rs/vercel-ai/openai/CLAUDE.md)
- [openai-compatible](coco-rs/vercel-ai/openai-compatible/CLAUDE.md)
- [google](coco-rs/vercel-ai/google/CLAUDE.md)
- [anthropic](coco-rs/vercel-ai/anthropic/CLAUDE.md)
- [bytedance](coco-rs/vercel-ai/bytedance/CLAUDE.md)

### Services
- [inference](coco-rs/services/inference/CLAUDE.md)
- [compact](coco-rs/services/compact/CLAUDE.md)
- [mcp](coco-rs/services/mcp/CLAUDE.md)
- [lsp](coco-rs/services/lsp/CLAUDE.md)

### Core
- [tool](coco-rs/core/tool/CLAUDE.md)
- [tools](coco-rs/core/tools/CLAUDE.md)
- [permissions](coco-rs/core/permissions/CLAUDE.md)
- [messages](coco-rs/core/messages/CLAUDE.md)
- [context](coco-rs/core/context/CLAUDE.md)

### Exec
- [shell](coco-rs/exec/shell/CLAUDE.md)
- [sandbox](coco-rs/exec/sandbox/CLAUDE.md)
- [process-hardening](coco-rs/exec/process-hardening/CLAUDE.md)
- [exec-server](coco-rs/exec/exec-server/CLAUDE.md)
- [apply-patch](coco-rs/exec/apply-patch/CLAUDE.md)

### Root Modules
- [commands](coco-rs/commands/CLAUDE.md)
- [skills](coco-rs/skills/CLAUDE.md)
- [hooks](coco-rs/hooks/CLAUDE.md)
- [tasks](coco-rs/tasks/CLAUDE.md)
- [memory](coco-rs/memory/CLAUDE.md)
- [plugins](coco-rs/plugins/CLAUDE.md)
- [keybindings](coco-rs/keybindings/CLAUDE.md)

### App
- [cli](coco-rs/app/cli/CLAUDE.md)
- [tui](coco-rs/app/tui/CLAUDE.md)
- [query](coco-rs/app/query/CLAUDE.md)
- [state](coco-rs/app/state/CLAUDE.md)
- [session](coco-rs/app/session/CLAUDE.md)

### Standalone
- [bridge](coco-rs/bridge/CLAUDE.md)
- [retrieval](coco-rs/retrieval/CLAUDE.md)

### Utils
- [absolute-path](coco-rs/utils/absolute-path/CLAUDE.md)
- [async-utils](coco-rs/utils/async-utils/CLAUDE.md)
- [cache](coco-rs/utils/cache/CLAUDE.md)
- [cargo-bin](coco-rs/utils/cargo-bin/CLAUDE.md)
- [common](coco-rs/utils/common/CLAUDE.md)
- [cursor](coco-rs/utils/cursor/CLAUDE.md)
- [file-encoding](coco-rs/utils/file-encoding/CLAUDE.md)
- [file-ignore](coco-rs/utils/file-ignore/CLAUDE.md)
- [file-search](coco-rs/utils/file-search/CLAUDE.md)
- [file-watch](coco-rs/utils/file-watch/CLAUDE.md)
- [frontmatter](coco-rs/utils/frontmatter/CLAUDE.md)
- [git](coco-rs/utils/git/CLAUDE.md)
- [image](coco-rs/utils/image/CLAUDE.md)
- [json-to-toml](coco-rs/utils/json-to-toml/CLAUDE.md)
- [keyring-store](coco-rs/utils/keyring-store/CLAUDE.md)
- [pty](coco-rs/utils/pty/CLAUDE.md)
- [readiness](coco-rs/utils/readiness/CLAUDE.md)
- [rustls-provider](coco-rs/utils/rustls-provider/CLAUDE.md)
- [secret-redact](coco-rs/utils/secret-redact/CLAUDE.md)
- [shell-parser](coco-rs/utils/shell-parser/CLAUDE.md)
- [sleep-inhibitor](coco-rs/utils/sleep-inhibitor/CLAUDE.md)
- [stdio-to-uds](coco-rs/utils/stdio-to-uds/CLAUDE.md)
- [stream-parser](coco-rs/utils/stream-parser/CLAUDE.md)
- [string](coco-rs/utils/string/CLAUDE.md)
- [symbol-search](coco-rs/utils/symbol-search/CLAUDE.md)

### User Docs
- [docs/](docs/) — getting-started.md, config.md, sandbox.md
