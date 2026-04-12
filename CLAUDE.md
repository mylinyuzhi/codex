# CLAUDE.md

Multi-provider LLM SDK and CLI. All development in `coco-rs/`.

> `cocode-rs/` and `codex-rs/` in this repo are reference implementations; active development is in `coco-rs/`.

## Commands

Run from `coco-rs/` directory:

```bash
just fmt          # After Rust changes (auto-approve)
just pre-commit   # REQUIRED before commit
just test         # If changed vercel-ai or core crates
just check        # Type-check all crates
just clippy       # Run clippy on all crates
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
- If a file exceeds ~800 LoC, add new functionality in a new module instead of extending the existing file
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
│  Services: inference, compact, mcp, rmcp-client, mcp-types, lsp     │
│  Exec: shell, sandbox, process-hardening                             │
│  Standalone: bridge, retrieval                                       │
├──────────────────────────────────────────────────────────────────────┤
│  Vercel AI: ai → openai, openai-compatible, google, anthropic,       │
│                  bytedance (on provider + provider-utils)              │
├──────────────────────────────────────────────────────────────────────┤
│  Common: types, config, error, otel, stack-trace-macro                │
│  Utils: 27 utility crates (see table below)                           │
└──────────────────────────────────────────────────────────────────────┘
```

## Key Data Flows

### Agent Turn Lifecycle (multi-turn cycle in `app/query`)

```
User input
  → ConversationContext.build()                    [context]
  → MessageHistory normalization                   [messages]
  → ApiClient.query(QueryParams)                   [inference → vercel-ai]
  → Parse response, extract tool calls             [query]
  → StreamingToolExecutor:
      safe tools → execute concurrently            [tool/tools]
      unsafe tools → queue, execute after stop     [tool/tools]
  → HookRegistry: PreToolUse / PostToolUse         [hooks]
  → Tool results → MessageHistory                  [messages]
  → Check stop conditions (ContinueReason)         [query]
  → If needs compaction → compact strategies       [compact]
  → Drain command queue + inject attachments        [query]
  → If tool calls exist → loop back to step 1
```

### Configuration Resolution

```
~/.coco/*provider.json + *model.json + config.json + env vars
  → ConfigLoader → Settings → GlobalConfig
  → ModelRoles + ModelAlias + FastModeState
  → Config snapshot → app/query → services/inference
```

### Shell Execution Flow

```
Bash tool → ShellExecutor.execute(command)
  → tokenize → parse → BashNode AST
  → security checks (23 check IDs)
  → read-only rules + destructive warnings
  → SandboxSettings check
  → spawn shell via tokio process
  → ShellProgress (stdout, stderr, exit_code)
```

### MCP Integration Flow

```
McpConnectionManager.connect(server_name)
  → McpConfigLoader → rmcp connection (stdio/HTTP/SSE)
  → OAuthConfig + OAuthTokens (auth)
  → DiscoveryCache (tools, resources)
  → Register tools in ToolRegistry
  → Tool calls → McpConnectionManager.call_tool() → rmcp → MCP server
```

## Crate Guide (68 crates)

### Common (5)

| Crate | Purpose | Key Types |
|-------|---------|-----------|
| `types` | Foundational types shared across all crates (zero internal deps) | `Message`, `UserMessage`, `AssistantMessage`, `PermissionMode`, `CommandBase` (41 tool variants), `HookEventType`, `SandboxMode`, `TokenUsage`, `ThinkingLevel`, `ProviderApi` |
| `config` | Layered config: JSON files + env + runtime overrides | `Settings`, `GlobalConfig`, `ModelInfo`, `ProviderInfo`, `ModelRoles`, `ModelAlias`, `FastModeState`, `SettingsWatcher`, `ConfigLoader` |
| `error` | Unified errors with StatusCode classification | `StatusCode` (5-digit `XX_YYY`), `ErrorExt` trait (`status_code()`, `is_retryable()`, `retry_after()`, `output_msg()`), snafu + snafu-virtstack |
| `otel` | OpenTelemetry tracing and metrics | `OtelManager` (counters, histograms, timers, session span) |
| `stack-trace-macro` | Proc macro for snafu error enums with virtual stack traces | `#[stack_trace_debug]` (proc macro attribute) |

### Services (6)

| Crate | Purpose | Key Types |
|-------|---------|-----------|
| `inference` | LLM client via vercel-ai with retry, auth, rate limiting | `ApiClient` (wraps `Arc<dyn LanguageModelV4>`), `QueryParams` (prompt, max_tokens, thinking_level, fast_mode, tools), `QueryResult` (content, usage, model, stop_reason), `RetryConfig`, `UsageAccumulator` |
| `compact` | Context compaction: full/micro/API-level with auto-trigger | `CompactStrategy`, `CompactResult`, `MicroCompactBudgetConfig`, `compact_conversation()`, `micro_compact()`, `should_auto_compact()`, circuit breaker for reactive compaction |
| `mcp` | MCP server lifecycle and discovery | `McpConnectionManager` (`Arc<RwLock<HashMap<String, McpConnectionState>>>`), `McpConfigLoader`, `DiscoveryCache`, `DiscoveredTool`, `OAuthConfig`, `OAuthTokens` |
| `mcp-types` | Auto-generated MCP message types (from spec) | `InitializeRequest`/`Result`, `CallToolRequest`/`Result`, `ListToolsRequest`/`Result`, `ModelContextProtocolRequest` trait |
| `rmcp-client` | MCP client: stdio + HTTP/SSE transport, OAuth | `RmcpClient` (state machine: Connecting->Ready), dual transport, `OAuthPersistor` (auto-refresh), bidirectional type conversion with mcp-types |
| `lsp` | AI-friendly LSP client (query by name+kind, not position) | `LspServerManager` (multi-server lifecycle), `LspClient`, `SymbolKind`, `ResolvedSymbol`, `DiagnosticsStore` (debounce), `ServerLifecycle` (max restarts, exponential backoff) |

### Core (5)

| Crate | Purpose | Key Types |
|-------|---------|-----------|
| `tool` | Tool trait, executor, registry (interface layer) | `Tool` trait, `ToolUseContext`, `ToolError`, `StreamingToolExecutor` (safe=concurrent, unsafe=queued), `ToolRegistry`, `ToolBatch`, `ValidationResult`, `AgentHandle`, `AgentSpawnRequest` |
| `tools` | 42 built-in tool implementations (41 static + MCP dynamic) | File I/O: Bash/Read/Write/Edit/Glob/Grep/NotebookEdit; Web: WebFetch/WebSearch; Agent: Agent/Skill/SendMessage/TeamCreate/TeamDelete; Task: TaskCreate/TaskUpdate/TaskGet/TaskList; System: Config/ToolSearch/CronTool/RemoteTrigger |
| `permissions` | Permission evaluation with auto-mode classification | `AutoModeDecision`, `AutoModeInput`, `AutoModeState`, `ClassifyRequest`, `classify_for_auto_mode()` (2-stage XML LLM), `rule_compiler`, `denial_tracking`, shell rules |
| `messages` | Message creation, normalization, filtering, predicates | `MessageHistory`, `NormalizedMessage`, `CostTracker`, `TurnRecord`, 13 creation + 10 normalization + 11 filtering + 19 predicate functions |
| `context` | System context assembly, CLAUDE.md discovery, attachments | `ConversationContext`, `EnvironmentInfo`, `Platform`, `ShellKind`, git status, file history tracking, 46K CLAUDE.md discovery logic |

### Exec (3)

| Crate | Purpose | Key Types |
|-------|---------|-----------|
| `shell` | Shell execution with security analysis and CWD tracking | `ShellExecutor`, `ShellProgress` (stdout, stderr, exit_code), `BashNode` AST, `SafetyResult`, 23 security check IDs, destructive command warnings, read-only rules |
| `sandbox` | Sandbox config (disabled by default, three modes) | `SandboxMode` (None/ReadOnly/Strict), `SandboxConfig`, `SandboxSettings`, `PermissionChecker`, `SandboxPlatform` trait |
| `process-hardening` | OS-level security (prctl, ptrace deny, env sanitization) | Platform-specific hardening (macOS: PT_DENY_ATTACH, Linux: prctl) |

### Root Modules (7)

| Crate | Purpose | Key Types |
|-------|---------|-----------|
| `commands` | Slash command registry (~96 commands across v1/v2/v3) | `CommandHandler` trait (`execute() -> Result<String>`), `RegisteredCommand`, `CommandType`, commands: help/config/clear/diff/doctor/compact/session/login/mcp/plan/plugin/tasks/agents/skills |
| `skills` | Skill markdown workflows from bundled/project/user/plugin sources | `SkillDefinition` (name, description, prompt, source), `SkillContext` (Inline/Fork), `SkillManager` (`Arc<Mutex>`), `SkillSource`, file watcher |
| `hooks` | Pre/post event interception with scoped priority | `HookDefinition` (event + matcher + handler + priority + scope), `HookHandler` (Command/Prompt/Http/Agent), `HookScope`, `HookEventType`, orchestration module |
| `tasks` | Background task management | `TaskManager` (`Arc<RwLock<HashMap<String, TaskStateBase>>>`), `TaskOutput` (stdout, stderr, exit_code), todo module |
| `memory` | Persistent cross-session knowledge via per-project MEMORY.md | `MemoryEntry` (name, description, type, content, file_path), `MemoryEntryType` (User/Feedback/Project/Reference), 14 modules: auto_dream, classify, kairos, memdir, prefetch, scan, session_memory, staleness, team_sync |
| `plugins` | Plugin system via PLUGIN.toml manifests | `PluginManifest` (name, version, description, skills, hooks, mcp_servers), `LoadedPlugin`, `PluginLoader`, marketplace module |
| `keybindings` | Keyboard shortcuts with context-based resolution | `Keybinding` (key, action, context, when), 18 contexts, 73+ actions, chord support, `load_default_keybindings()` |

### App (5)

| Crate | Purpose | Key Types |
|-------|---------|-----------|
| `cli` | CLI entry point, transports (SSE/WS/NDJSON), server/daemon modes | `Cli` (clap), `Commands`, binary name `coco`, profile support |
| `tui` | Terminal UI using Elm architecture (TEA) | `App` (async run loop), `AppState` (model), `TuiEvent` (messages: keyboard/agent/file-search), `UserCommand` (TUI->Core: SubmitInput/Interrupt/ApprovalResponse), `Overlay` (permission prompts, model picker) |
| `session` | Session persistence and state aggregation | `Session` (id, timestamps, model, working_dir), `SessionState`, `SessionManager` (create/load/save/list), JSON persistence |
| `query` | Multi-turn agent loop driver (QueryEngine) | `QueryEngine` (orchestrates full agent loop), `QueryResult`, `QueryEngineConfig`, `ContinueReason` (NextTurn/ReactiveCompactRetry/MaxOutputTokensEscalate/TokenBudgetContinuation), `BudgetTracker`, `CommandQueue`, `Inbox`, `QueryGuard` |
| `state` | Central application state tree + swarm support | `AppState` (`Arc<RwLock>`, 80+ fields), swarm orchestration (21 modules: swarm_runner, swarm_agent_handle, swarm_backend, swarm_mailbox, swarm_prompt, swarm_spawn_utils, swarm_task, swarm_teammate) |

### Standalone (2)

| Crate | Purpose | Key Types |
|-------|---------|-----------|
| `bridge` | IDE bridge (VS Code/JetBrains) + REPL bridge | `BridgeInMessage`/`BridgeOutMessage` (protocol), `ReplBridge`, `BridgeServer`, `BridgeState` |
| `retrieval` | Code search: BM25 + vector + AST via Facade pattern | `RetrievalFacade` (search, build_index, generate_repomap), `SearchRequest` (fluent: `.bm25()`, `.vector()`, `.hybrid()`, `.limit()`), `CodeChunk`, `IndexManager`, `RepoMapGenerator` (PageRank), features: `local-embeddings`, `neural-reranker` |

### Vercel AI (8)

| Crate | Purpose | Key Types |
|-------|---------|-----------|
| `vercel-ai-provider` | Standalone types matching @ai-sdk/provider v4 spec | `LanguageModelV4` trait, `EmbeddingModelV4` trait, `ImageModelV4` trait, `ProviderV4` trait, `LanguageModelV4Prompt`, `UserContentPart`/`AssistantContentPart`/`ToolContentPart` |
| `vercel-ai-provider-utils` | Utilities for implementing AI SDK v4 providers | `ApiResponse`, `ResponseHandler`/`JsonResponseHandler`/`StreamResponseHandler`, `ErrorHandler`, `Fetch`/`FetchOptions`, `Schema`/`ValidationError`, `ToolMapping` |
| `vercel-ai` | High-level SDK matching @ai-sdk/ai (generate_text, stream_text, embed) | `GenerateTextOptions`/`GenerateTextResult`, `StreamTextOptions`, `GenerateTextCallbacks`/`StreamTextCallbacks`, `GenerateObjectOptions`, `EmbedOptions`/`EmbedResult`, `OutputStrategy`, `LanguageModel` |
| `vercel-ai-openai` | OpenAI provider for Vercel AI SDK v4 | `OpenAIProvider`, `OpenAIProviderSettings`, `OpenAIChatLanguageModel`, `OpenAIResponsesLanguageModel`, `OpenAIEmbeddingModel`, `OpenAIImageModel` |
| `vercel-ai-openai-compatible` | Generic OpenAI-compatible provider (xAI, Groq, Together, etc.) | `OpenAICompatibleProvider`, `OpenAICompatibleProviderSettings`, `OpenAICompatibleChatLanguageModel`, `MetadataExtractor`, `StreamMetadataExtractor` |
| `vercel-ai-google` | Google Gemini provider for Vercel AI SDK v4 | `GoogleGenerativeAIProvider`, `GoogleGenerativeAIProviderSettings`, `GoogleGenerativeAILanguageModel`, `GoogleGenerativeAIEmbeddingModel`, `GoogleGenerativeAIImageModel` |
| `vercel-ai-anthropic` | Anthropic Claude provider for Vercel AI SDK v4 | `AnthropicProvider`, `AnthropicProviderSettings`, `AnthropicMessagesLanguageModel`, `AnthropicConfig`, `CacheControlValidator` |
| `vercel-ai-bytedance` | ByteDance video provider (Seedance) via ModelArk API | `ByteDanceProvider`, `ByteDanceProviderSettings`, `ByteDanceVideoModel`, `ByteDanceVideoModelConfig` |

### Utils (27)

| Crate | Purpose |
|-------|---------|
| `absolute-path` | Absolute path types with deserialization support |
| `apply-patch` | Unified diff/patch application with fuzzy matching |
| `async-utils` | Async runtime utilities and cancellation helpers |
| `cache` | LRU cache with Tokio mutex protection |
| `cargo-bin` | Cargo binary helpers for test harnesses |
| `common` | Shared cross-crate utility functions |
| `cursor` | Text cursor with kill ring (Ctrl+K/Y), word boundaries, UTF-8 safe |
| `file-encoding` | File encoding and line-ending detection/preservation |
| `file-ignore` | .gitignore-aware file filtering |
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
| `shell-parser` | Shell command parsing and security analysis (23 analyzers) |
| `sleep-inhibitor` | Cross-platform sleep prevention (macOS/Linux/Windows) |
| `stdio-to-uds` | Bridge stdio streams to Unix domain sockets |
| `stream-parser` | Stream parsing (text, citation, inline hidden tag, proposed plan, UTF-8) |
| `string` | String truncation and boundary utilities |
| `symbol-search` | Symbol search for code navigation |
| `test-harness` | Test harness utilities for integration tests |

## Key Design Patterns

| Pattern | Where | Details |
|---------|-------|---------|
| **Builder** | Most crates | `FacadeBuilder`, `QueryEngine::new()`, `SearchRequest` (fluent) |
| **Arc-heavy sharing** | core/, root modules, app/ | Registries, managers, trackers: `Arc<Mutex<T>>` or `Arc<RwLock<T>>` (e.g. `TaskManager`, `SkillManager`, `AppState`) |
| **Event-driven** | query, tui | `mpsc::Sender` for UI updates, `tokio::select!` multiplexing |
| **Cancellation** | All async | `CancellationToken` threaded through all layers (223+ usages) |
| **Registry** | core/tool, commands, skills, plugins, keybindings | `ToolRegistry`, `CommandRegistry`, `SkillManager`, `PluginManager`, `KeybindingResolver` |
| **State Machine** | query, permissions, mcp | `ContinueReason` (loop control), `AutoModeState`, `ConnectionState` |
| **Callback decoupling** | tool, tools | `AgentHandle` callback avoids tool->subagent circular dependency |
| **Permission pipeline** | permissions, tool | `Tool.check_permission()` -> 2-stage auto-mode classification -> `denial_tracking` |
| **Facade** | retrieval | Single `RetrievalFacade` entry point hides search + index + repomap services |
| **Elm (TEA)** | tui | Model (`AppState`) + Message (`TuiEvent`) + Update (`handle_command`) + View (`render`) |
| **Middleware** | vercel-ai | `FnOnce` + `BoxFuture` callbacks for `do_generate`/`do_stream` delegation |

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

### Tokio Runtime

- Use `tokio::task::spawn_blocking` for blocking operations
- Prefer `tokio::sync` primitives over `std::sync` in async contexts
- Add `Send + Sync` bounds to traits used with `Arc<dyn Trait>`

## Dependencies

### Adding Dependencies

- Prefer well-maintained crates with active development
- Check for security advisories before adding
- Use workspace dependencies when possible

### Common Dependencies

| Purpose | Crate |
|---------|-------|
| Async runtime | `tokio` |
| HTTP client | `reqwest` |
| JSON | `serde_json` |
| Error handling | `anyhow`, `snafu` |
| Logging | `tracing` |
| Testing | `pretty_assertions` |

## Error Handling

| Layer | Error Type |
|-------|------------|
| common/, core/, services/ | `coco-error` + snafu + snafu-virtstack (StatusCode `XX_YYY` classification, retryable flag) |
| root modules | snafu + `coco-error` |
| utils/ | `anyhow::Result` |
| vercel-ai/ | `thiserror` (standalone, no coco deps) |
| app/, exec/, standalone | `anyhow::Result` |

StatusCode categories: General (00-05), Config (10), Provider (11), Resource (12). See [common/error/README.md](coco-rs/common/error/README.md).

## Design Decisions

| Decision | Rationale |
|----------|-----------|
| **No Deprecated Code** | When refactoring or implementing features, remove obsolete code completely. Do not mark as deprecated or maintain backward compatibility - delete it outright to keep the codebase clean and avoid technical debt. |
| **No Inline Tests** | Never put `#[cfg(test)] mod tests { ... }` with test functions inline in source files. Always extract tests to a separate `<name>.test.rs` file and reference it with `#[cfg(test)] #[path = "<name>.test.rs"] mod tests;` at the end of the source file. |
| **No `unsafe` Code** | Never use `unsafe` blocks, `unsafe fn`, `unsafe impl`, or `unsafe trait` anywhere in the codebase. All code must be written in safe Rust. If a dependency requires `unsafe` at the boundary, wrap it in a safe abstraction within that dependency — never expose `unsafe` to coco-rs crates. If you believe `unsafe` is truly unavoidable, stop and discuss the design with the team first. |
| **No Hardcoded Tool/Enum Names** | Never use string literals like `"Read"`, `"Task"`, `"shutdown_request"` for tool names or enum identifiers. Always use the canonical enum's `as_str()` method: `CommandBase::Read.as_str()`. For const arrays use `const X: &[&str] = &[CommandBase::Xxx.as_str(), ...]`. When the canonical enum is in a crate you cannot depend on (e.g., cross-layer), define well-known constants in a shared module. This prevents silent breakage when enum variants are renamed. |
| **No Single-Use Helpers** | Do not create small helper methods that are referenced only once. Inline the logic at the call site. |

## Specialized Documentation

Every crate in `coco-rs/` has its own `CLAUDE.md` with crate-specific guidance. Key entry points:

| Component | Guide |
|-----------|-------|
| Types | [common/types/CLAUDE.md](coco-rs/common/types/CLAUDE.md) |
| Config | [common/config/CLAUDE.md](coco-rs/common/config/CLAUDE.md) |
| Error | [common/error/CLAUDE.md](coco-rs/common/error/CLAUDE.md) |
| Inference | [services/inference/CLAUDE.md](coco-rs/services/inference/CLAUDE.md) |
| MCP | [services/mcp/CLAUDE.md](coco-rs/services/mcp/CLAUDE.md) |
| Compact | [services/compact/CLAUDE.md](coco-rs/services/compact/CLAUDE.md) |
| LSP | [services/lsp/CLAUDE.md](coco-rs/services/lsp/CLAUDE.md) |
| Tool | [core/tool/CLAUDE.md](coco-rs/core/tool/CLAUDE.md) |
| Tools | [core/tools/CLAUDE.md](coco-rs/core/tools/CLAUDE.md) |
| Permissions | [core/permissions/CLAUDE.md](coco-rs/core/permissions/CLAUDE.md) |
| Messages | [core/messages/CLAUDE.md](coco-rs/core/messages/CLAUDE.md) |
| Context | [core/context/CLAUDE.md](coco-rs/core/context/CLAUDE.md) |
| Shell | [exec/shell/CLAUDE.md](coco-rs/exec/shell/CLAUDE.md) |
| Commands | [commands/CLAUDE.md](coco-rs/commands/CLAUDE.md) |
| Skills | [skills/CLAUDE.md](coco-rs/skills/CLAUDE.md) |
| Hooks | [hooks/CLAUDE.md](coco-rs/hooks/CLAUDE.md) |
| Tasks | [tasks/CLAUDE.md](coco-rs/tasks/CLAUDE.md) |
| Memory | [memory/CLAUDE.md](coco-rs/memory/CLAUDE.md) |
| Plugins | [plugins/CLAUDE.md](coco-rs/plugins/CLAUDE.md) |
| Keybindings | [keybindings/CLAUDE.md](coco-rs/keybindings/CLAUDE.md) |
| Query | [app/query/CLAUDE.md](coco-rs/app/query/CLAUDE.md) |
| State | [app/state/CLAUDE.md](coco-rs/app/state/CLAUDE.md) |
| TUI | [app/tui/CLAUDE.md](coco-rs/app/tui/CLAUDE.md) |
| CLI | [app/cli/CLAUDE.md](coco-rs/app/cli/CLAUDE.md) |
| Session | [app/session/CLAUDE.md](coco-rs/app/session/CLAUDE.md) |
| Bridge | [bridge/CLAUDE.md](coco-rs/bridge/CLAUDE.md) |
| Retrieval | [retrieval/CLAUDE.md](coco-rs/retrieval/CLAUDE.md) |
| Vercel AI SDK | [vercel-ai/ai/CLAUDE.md](coco-rs/vercel-ai/ai/CLAUDE.md) |
| Vercel AI Provider | [vercel-ai/provider/CLAUDE.md](coco-rs/vercel-ai/provider/CLAUDE.md) |
| Vercel AI Provider Utils | [vercel-ai/provider-utils/CLAUDE.md](coco-rs/vercel-ai/provider-utils/CLAUDE.md) |
| File Ignore | [utils/file-ignore/CLAUDE.md](coco-rs/utils/file-ignore/CLAUDE.md) |
| Frontmatter | [utils/frontmatter/CLAUDE.md](coco-rs/utils/frontmatter/CLAUDE.md) |
| Cursor | [utils/cursor/CLAUDE.md](coco-rs/utils/cursor/CLAUDE.md) |
| Error Codes | [common/error/README.md](coco-rs/common/error/README.md) |
| User Docs | [docs/](docs/) (getting-started.md, config.md, sandbox.md) |
