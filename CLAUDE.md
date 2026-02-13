# CLAUDE.md

Multi-provider LLM SDK and CLI. All development in `cocode-rs/`.

**Read `AGENTS.md` for Rust conventions.** This file covers architecture, key types, data flow, and crate navigation.

## Commands

Run from `cocode-rs/` directory:

```bash
just fmt          # After Rust changes (auto-approve)
just pre-commit   # REQUIRED before commit
just test         # If changed provider-sdks or core crates
just check        # Type-check all crates
just clippy       # Run clippy on all crates
just help         # All commands
```

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│  App: cli, tui, session                                         │
├─────────────────────────────────────────────────────────────────┤
│  Core: loop → executor → api                                    │
│            ↓        ↓                                           │
│         tools ← context ← prompt                                │
│            ↓                                                    │
│      message, system-reminder, subagent                         │
├─────────────────────────────────────────────────────────────────┤
│  Features: skill, hooks, plugin, plan-mode                      │
│  Exec: shell, sandbox, arg0                                     │
│  MCP: mcp-types, rmcp-client                                    │
│  Standalone: retrieval, lsp                                     │
├─────────────────────────────────────────────────────────────────┤
│  Provider SDKs: hyper-sdk → anthropic, openai, google-genai,    │
│                             volcengine-ark, z-ai                │
├─────────────────────────────────────────────────────────────────┤
│  Common: protocol, config, error, otel                          │
│  Utils: 20 utility crates (see table below)                     │
└─────────────────────────────────────────────────────────────────┘
```

## Key Data Flows

### Agent Turn Lifecycle (18-step cycle in `core/loop`)

```
User input
  → SystemReminderOrchestrator.generate_all()    [system-reminder]
  → SystemPromptBuilder.build()                   [prompt]
  → ToolRegistry.definitions_for_model()          [tools]
  → ApiClient.stream_request()                    [api → hyper-sdk]
  → StreamProcessor yields StreamEvents           [api]
  → StreamingToolExecutor:
      safe tools → execute concurrently           [tools]
      unsafe tools → queue, execute after stop    [tools]
  → HookRegistry: PreToolUse / PostToolUse        [hooks]
  → Tool results → MessageHistory.add_turn()      [message]
  → If tool calls exist → loop back to step 1
  → If needs compaction → micro-compact or session memory
  → Emit LoopEvent::TurnCompleted                 [protocol]
```

### Configuration Resolution

```
~/.cocode/*provider.json + *model.json + config.json + env vars
  → ConfigLoader → ConfigResolver → ConfigManager
  → RuntimeOverrides (per-role ModelSpec + ThinkingLevel)
  → Config snapshot → core/executor → core/loop
```

### Provider Call Chain

```
ConfigManager.resolve_provider(name)
  → ProviderInfo (api_key, base_url, models, wire_api)
  → hyper-sdk: Provider::model(slug) → Arc<dyn Model>
  → model.stream(GenerateRequest) → StreamResponse
  → core/api: UnifiedStream wraps with retry, stall detection, fallback
```

### Shell Execution Flow

```
Bash tool → ShellExecutor.execute(CommandInput)
  → analyze_command_safety() [shell-parser: 15 analyzers]
  → SandboxSettings.is_sandboxed() check
  → spawn shell with CWD tracking markers
  → CommandResult (exit_code, stdout, stderr, new_cwd)
```

## Crate Guide (53 crates)

### Common (4)

| Crate | Purpose | Key Types |
|-------|---------|-----------|
| `protocol` | Foundational types shared across all crates | `ModelInfo`, `ProviderType` (6 providers), `ProviderInfo`, `ModelRole` (Main/Fast/Vision/Review/Plan/Explore), `PermissionMode`, `LoopEvent`, `LoopConfig`, `Feature` (14 toggleable features), `SecurityRisk`, `Capability` |
| `config` | Layered config: JSON files + env + runtime overrides | `ConfigManager` (thread-safe, `RwLock`), `ConfigLoader`, `ConfigResolver`, `Config` (complete snapshot), `RuntimeOverrides`, `RoleSelections` |
| `error` | Unified errors with StatusCode classification | `StatusCode` (5-digit `XX_YYY`), `ErrorExt` trait (`status_code()`, `is_retryable()`, `retry_after()`), snafu + snafu-virtstack |
| `otel` | OpenTelemetry tracing and metrics | `OtelManager` (counters, histograms, timers, session span) |

### Provider SDKs (6)

| Crate | Purpose | Key Types |
|-------|---------|-----------|
| `hyper-sdk` | **Main SDK**: Unified multi-provider client (standalone, no protocol deps) | `Provider` trait, `Model` trait (`generate()`, `stream()`, `embed()`), `Message`, `ContentBlock`, `GenerateRequest`/`Response`, `StreamResponse`/`Event`, `HookChain`, `HttpInterceptor` |
| `anthropic` | Anthropic Claude API | Implements `Provider`/`Model` for Claude models |
| `openai` | OpenAI Responses API | Implements `Provider`/`Model` for GPT models |
| `google-genai` | Google Gemini API | Implements `Provider`/`Model` for Gemini models |
| `volcengine-ark` | Volcengine Ark API | Implements `Provider`/`Model` with endpoint-ID aliasing |
| `z-ai` | ZhipuAI / Z.AI API | Implements `Provider`/`Model` for Z.AI models |

### Core (9)

| Crate | Purpose | Key Types |
|-------|---------|-----------|
| `api` | Provider-agnostic LLM client with retry, fallback, stall detection | `ApiClient` (wraps `hyper-sdk::Model`), `UnifiedStream` (streaming/non-streaming abstraction), `StreamingQueryResult`, `CollectedResponse`, `RetryContext` |
| `message` | Conversation history with turn tracking and compaction | `TrackedMessage` (uuid, turn_id, source, tombstoned), `MessageSource` (User/Assistant/System/Tool/Subagent/SystemReminder), `Turn` (user+assistant+tool_calls+usage), `MessageHistory` (turns, compaction, micro-compact) |
| `tools` | Tool trait, 5-stage pipeline, concurrent execution | `Tool` trait (validate→check_permission→execute→post_process→cleanup), `ToolContext` (cwd, permissions, shell_executor, lsp_manager, cancel_token, spawn_agent_fn), `StreamingToolExecutor` (safe=concurrent, unsafe=queued), `ToolRegistry`, `FileTracker` |
| `context` | Context window assembly and token budgeting | `ConversationContext` (environment, budget, tool_names, memory_files, injections), `ContextBudget` with `BudgetCategory` (SystemPrompt/Tools/Memory/Messages/Output/Reserved), `EnvironmentInfo` |
| `prompt` | System prompt from 14 embedded templates | `SystemPromptBuilder` (pure sync string assembly), `PromptSection` (Identity/ToolPolicy/Security/GitWorkflow/TaskManagement/MCP/Environment/Permission/MemoryFiles/Injections), injection positions: BeforeTools/AfterTools/EndOfPrompt |
| `system-reminder` | Per-turn dynamic context injection | `SystemReminderOrchestrator` (parallel generators), `SystemReminder` (tier: MainAgentOnly/UserPrompt/Always), generators: ChangedFiles/PlanMode/TodoReminders/LspDiagnostics/NestedMemory, injected as `is_meta: true` user messages with `<system-reminder>` XML tags |
| `loop` | Agent loop driver: multi-turn 18-step cycle | `AgentLoop` (`run(prompt) → LoopResult`), compaction (micro-compact: clear old tool results; session memory: background summarization agent), fallback (stream→non-stream, context overflow→reduce max_tokens 25%, model fallback), `AgentStatus` via `watch::Sender` |
| `subagent` | Isolated agent spawning for parallel tasks | `AgentDefinition` (tools, disallowed_tools, identity, max_turns), `SubagentManager` (register, spawn_full), `AgentInstance` (Running/Completed/Failed/Backgrounded), three-layer tool filtering, foreground (blocks) vs background (output_file) |
| `executor` | Top-level session driver wiring all components | `AgentExecutor` (`execute(prompt) → String`), `ExecutorBuilder` → `AgentExecutor` → `AgentLoop` → `StreamingToolExecutor`, permission pipeline: `PermissionRule` → `PermissionRuleEvaluator` → `ApprovalStore` |

### App (3)

| Crate | Purpose | Key Types |
|-------|---------|-----------|
| `cli` | CLI entry point via `arg0_dispatch_or_else(cli_main)` | `Cli` (clap), `Commands` (Chat/Config/Resume/Sessions/Status), profile support, `--no-tui` for REPL mode |
| `tui` | Terminal UI using Elm architecture (TEA) | `App` (async run loop), `AppState` (model), `TuiEvent` (messages: keyboard/agent/file-search), `UserCommand` (TUI→Core: SubmitInput/Interrupt/ApprovalResponse), `Overlay` (permission prompts, model picker), `tokio::select!` on agent_rx + events + file_search + symbol_search channels |
| `session` | Session persistence and state aggregation | `Session` (id, timestamps, model, working_dir), `SessionState` (message_history + tool_registry + hook_registry + executor), `SessionManager` (create/load/save/list), JSON persistence at `~/.cocode/sessions/` |

### Features (4)

| Crate | Purpose | Key Types |
|-------|---------|-----------|
| `skill` | `/command` slash commands from bundled/project/user/plugin sources | `SkillInterface` (name, description, prompt), `SkillManager` (loading, validation, dedup), `SkillScanner` (multi-source discovery), `BundledSkill` (SHA-256 fingerprint) |
| `hooks` | Pre/post event interception with scoped priority | `HookDefinition` (event + matcher + handler), `HookEventType` (BeforeToolCall/AfterToolCall/SessionStart), `HookHandler` (Command/Prompt/Agent/Webhook/Inline), `HookScope` (Skill > Plugin > Project > User > Global), `AsyncHookTracker` |
| `plugin` | Plugin system via PLUGIN.toml manifests | `PluginManifest` (PLUGIN.toml), `PluginContributions` (skills, hooks, agents, commands, MCP servers), `PluginScope` (Managed > User > Project), `PluginLoader`, `PluginRegistry` |
| `plan-mode` | Plan file management for plan-then-execute workflow | `PlanFileManager` (CRUD), `PlanModeState`, plans at `~/.cocode/plans/{adjective}-{action}-{noun}.md` |

### Exec (3)

| Crate | Purpose | Key Types |
|-------|---------|-----------|
| `shell` | Shell execution with CWD tracking, safety analysis, snapshotting | `ShellExecutor` (CWD tracking via markers, shell env snapshotting), `CommandResult` (exit_code, stdout, stderr, paths, new_cwd), `SafetyResult` (Safe/RequiresApproval/Denied), `BackgroundTaskRegistry`, `fork_for_subagent()` for isolated CWD |
| `sandbox` | Sandbox config (disabled by default, three modes) | `SandboxMode` (None/ReadOnly/Strict), `SandboxConfig`, `SandboxSettings` (enabled, auto_allow_bash_if_sandboxed), `PermissionChecker`, `SandboxPlatform` trait (Unix/Windows stubs) |
| `arg0` | Binary dispatcher and PATH setup | `arg0_dispatch_or_else(main_fn)`, argv[0] dispatch (apply_patch, linux-sandbox), `--cocode-run-as-apply-patch` flag, temp dir with symlinks for PATH, dotenv loading (filters `COCODE_*` vars) |

### MCP (2)

| Crate | Purpose | Key Types |
|-------|---------|-----------|
| `mcp-types` | Auto-generated MCP message types (from spec 2025-06-18) | `InitializeRequest`/`Result`, `CallToolRequest`/`Result`, `ListToolsRequest`/`Result`, `ModelContextProtocolRequest` trait, tagged enums with `#[serde(tag = "method")]` |
| `rmcp-client` | MCP client: stdio + HTTP/SSE transport, OAuth | `RmcpClient` (state machine: Connecting→Ready), dual transport (TokioChildProcess for stdio, StreamableHttp for SSE), `OAuthPersistor` (auto-refresh), bidirectional type conversion with mcp-types |

### Standalone (2)

| Crate | Purpose | Key Types |
|-------|---------|-----------|
| `retrieval` | Code search: BM25 + vector + AST via Facade pattern | `RetrievalFacade` (search, build_index, generate_repomap), `SearchRequest` (fluent: `.bm25()`, `.vector()`, `.hybrid()`, `.limit()`), `CodeChunk`, `IndexManager`, `RepoMapGenerator` (PageRank), presets: NONE/MINIMAL/STANDARD/FULL, LRU cache by workdir (max 16), SQLite + LanceDB storage |
| `lsp` | AI-friendly LSP client (query by name+kind, not position) | `LspServerManager` (multi-server lifecycle), `LspClient` (e.g. `client.definition(path, "Config", Some(SymbolKind::Struct))`), `SymbolKind`, `ResolvedSymbol`, `DiagnosticsStore` (300ms debounce), `ServerLifecycle` (max 5 restarts, exponential backoff), built-in: rust-analyzer, gopls, pyright, typescript-language-server |

### Utils (20)

| Crate | Purpose |
|-------|---------|
| `absolute-path` | Absolute path types with deserialization support |
| `apply-patch` | Unified diff/patch application with fuzzy matching |
| `async-utils` | Async runtime utilities and cancellation helpers |
| `cache` | LRU cache with Tokio mutex protection |
| `cargo-bin` | Cargo binary helpers for test harnesses |
| `common` | Shared cross-crate utility functions |
| `file-encoding` | File encoding and line-ending detection/preservation |
| `file-ignore` | .gitignore-aware file filtering |
| `file-search` | Fuzzy file search with nucleo and gitignore support |
| `file-watch` | Generic reusable file-watch infrastructure |
| `git` | Git operations wrapper |
| `image` | Image processing utilities |
| `json-to-toml` | JSON to TOML conversion |
| `keyring-store` | Secure credential storage using system keyring |
| `pty` | Pseudo-terminal handling |
| `readiness` | Readiness flag with token-based auth and async waiting |
| `shell-parser` | Shell command parsing and security analysis (15 analyzers: 7 Allow + 8 Ask) |
| `stdio-to-uds` | Bridge stdio streams to Unix domain sockets |
| `string` | String truncation and boundary utilities |
| `symbol-search` | Symbol search for code navigation |

## Key Design Patterns

| Pattern | Where | Details |
|---------|-------|---------|
| **Builder** | Most crates | `ExecutorBuilder`, `AgentLoop::new()`, `FacadeBuilder`, `SearchRequest` (fluent) |
| **Arc-heavy sharing** | core/ | Registries, managers, trackers: `Arc<Mutex<T>>` or `Arc<T>` |
| **Event-driven** | loop, tui | `mpsc::Sender<LoopEvent>` for UI updates, `tokio::select!` multiplexing |
| **Cancellation** | All async | `CancellationToken` threaded through all layers |
| **Callback decoupling** | tools, subagent | `SpawnAgentFn` callback avoids tools→subagent circular dependency; `context` stores tool names as `Vec<String>` to avoid tools dep |
| **Meta messages** | system-reminder | `is_meta: true` hides reminders from UI while keeping them in model context |
| **Permission pipeline** | executor, tools | `Tool.check_permission()` → `PermissionRuleEvaluator` (sorted by `RuleSource` priority: Session > Command > Cli > Flag > Local > Project > Policy > User) → `ApprovalStore` |
| **Facade** | retrieval | Single `RetrievalFacade` entry point hides SearchService + IndexService + RecentFilesService |
| **Elm (TEA)** | tui | Model (`AppState`) + Message (`TuiEvent`) + Update (`handle_command`) + View (`render`) |

## Error Handling

| Layer | Error Type |
|-------|------------|
| common/, core/ | `cocode-error` + snafu + snafu-virtstack (StatusCode `XX_YYY` classification, retryable flag) |
| features/ | snafu |
| provider-sdks/, utils/ | `anyhow::Result` |
| app/, exec/, mcp/, standalone | `anyhow::Result` |

StatusCode categories: General (00-05), Config (10), Provider (11), Resource (12). See [common/error/README.md](cocode-rs/common/error/README.md).

## Specialized Documentation

| Component | Guide |
|-----------|-------|
| TUI | [app/tui/CLAUDE.md](cocode-rs/app/tui/CLAUDE.md) |
| Retrieval | [retrieval/CLAUDE.md](cocode-rs/retrieval/CLAUDE.md) |
| LSP | [lsp/CLAUDE.md](cocode-rs/lsp/CLAUDE.md) |
| Hyper SDK | [provider-sdks/hyper-sdk/CLAUDE.md](cocode-rs/provider-sdks/hyper-sdk/CLAUDE.md) |
| Anthropic SDK | [provider-sdks/anthropic/CLAUDE.md](cocode-rs/provider-sdks/anthropic/CLAUDE.md) |
| OpenAI SDK | [provider-sdks/openai/CLAUDE.md](cocode-rs/provider-sdks/openai/CLAUDE.md) |
| Google GenAI SDK | [provider-sdks/google-genai/CLAUDE.md](cocode-rs/provider-sdks/google-genai/CLAUDE.md) |
| Volcengine Ark SDK | [provider-sdks/volcengine-ark/CLAUDE.md](cocode-rs/provider-sdks/volcengine-ark/CLAUDE.md) |
| Z.AI SDK | [provider-sdks/z-ai/CLAUDE.md](cocode-rs/provider-sdks/z-ai/CLAUDE.md) |
| File Ignore | [utils/file-ignore/CLAUDE.md](cocode-rs/utils/file-ignore/CLAUDE.md) |

## Design Decisions

| Decision | Rationale |
|----------|-----------|
| **No Prompt Caching** | Prompt caching (Anthropic's cache breakpoints feature) is not required for this project. Do not implement or plan for it. |
| **No Deprecated Code** | When refactoring or implementing features, remove obsolete code completely. Do not mark as deprecated or maintain backward compatibility - delete it outright to keep the codebase clean and avoid technical debt. |

## References

- **Code conventions**: `AGENTS.md`
- **Error codes**: `cocode-rs/common/error/README.md`
- **User docs**: `docs/` (getting-started.md, config.md, sandbox.md)
